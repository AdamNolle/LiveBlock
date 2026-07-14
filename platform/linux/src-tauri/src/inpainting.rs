//! Mirror-blend inpainter for the Linux port.
//!
//! Algorithm (mirrors `Sources/InpaintingEngine.swift` and the Windows port):
//!  1. Pick blend axis from the region's aspect ratio: wide → vertical mirror,
//!     tall → horizontal mirror, square-ish → vertical first.
//!  2. Sample a band of pixels above/below (or left/right), reflect across the
//!     region's adjacent edge, cross-fade with a linear gradient mask.
//!  3. If neither side has a band wide/tall enough, fall back to a solid fill
//!     using the average of the available border pixels.
//!
//! A bounded wgpu compute path batches mirror-blend patches through Vulkan or
//! GL. Unsupported regions, unavailable adapters, validation/map failures, and
//! timeouts fail over to the byte-compatible CPU implementation.

use crate::regions::NormalizedRegion;
use anyhow::{anyhow, Context, Result};
use base64::{engine::general_purpose::STANDARD, Engine as _};
use bytemuck::{Pod, Zeroable};
use parking_lot::Mutex;
use serde::Serialize;
use std::collections::HashMap;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    mpsc, Arc,
};
use std::thread;
use std::time::{Duration, Instant};
use wgpu::util::DeviceExt;

/// Patch payload schema must stay byte-identical with the Windows port (the
/// frontend `ipc.ts` is shared). `png_data_url` is a `data:image/png;base64,...`
/// URL the frontend can drop straight into an `<img src>`.
#[derive(Debug, Clone, Serialize)]
pub struct PatchPayload {
    pub id: String,
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
    pub png_data_url: String,
}

#[derive(Clone)]
struct CachedPatch {
    png_data_url: String,
    rendered: Instant,
}

pub struct Inpainter {
    cache: Mutex<HashMap<String, CachedPatch>>,
    max_age: Duration,
    gpu: Option<Arc<GpuInpainter>>,
    backend_status: String,
}

impl Inpainter {
    pub fn new() -> Self {
        let (sender, receiver) = mpsc::sync_channel(1);
        thread::spawn(move || {
            let result = pollster::block_on(GpuInpainter::new()).map(Arc::new);
            let _ = sender.send(result);
        });
        let (gpu, backend_status) = match receiver.recv_timeout(Duration::from_millis(750)) {
            Ok(Ok(gpu)) => {
                let status = format!("wgpu_{}_experimental", gpu.backend_name);
                (Some(gpu), status)
            }
            Ok(Err(error)) => {
                tracing::warn!("Linux wgpu inpainting unavailable; using CPU: {error}");
                (None, "cpu_fallback".into())
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {
                tracing::warn!("Linux wgpu initialization exceeded 750 ms; using CPU");
                (None, "cpu_fallback_after_gpu_init_timeout".into())
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                tracing::warn!("Linux wgpu initialization worker stopped; using CPU");
                (None, "cpu_fallback_after_gpu_init_error".into())
            }
        };
        Self {
            cache: Mutex::new(HashMap::new()),
            max_age: Duration::from_millis(400),
            gpu,
            backend_status,
        }
    }

    pub fn backend_status(&self) -> &str {
        if self
            .gpu
            .as_ref()
            .is_some_and(|gpu| gpu.failed.load(Ordering::Acquire))
        {
            "wgpu_failed_pending_cpu_fallback"
        } else {
            &self.backend_status
        }
    }

    pub fn inpaint(
        &mut self,
        frame_bgra: &[u8],
        frame_w: u32,
        frame_h: u32,
        regions: &[NormalizedRegion],
    ) -> Result<Vec<PatchPayload>> {
        if let Some(gpu) = self.gpu.as_ref() {
            match inpaint_gpu(
                gpu,
                &self.cache,
                self.max_age,
                frame_bgra,
                frame_w,
                frame_h,
                regions,
            ) {
                Ok(patches) => return Ok(patches),
                Err(error) => {
                    tracing::warn!("Linux wgpu inpainting failed; disabling GPU path: {error}");
                    self.gpu = None;
                    self.backend_status = "cpu_fallback_after_gpu_error".into();
                    self.cache.lock().clear();
                }
            }
        }
        self.inpaint_cpu(frame_bgra, frame_w, frame_h, regions)
    }

    fn inpaint_cpu(
        &mut self,
        bgra: &[u8],
        width: u32,
        height: u32,
        regions: &[NormalizedRegion],
    ) -> Result<Vec<PatchPayload>> {
        if regions.is_empty() || width == 0 || height == 0 {
            return Ok(Vec::new());
        }

        let mut payloads: Vec<PatchPayload> = Vec::with_capacity(regions.len());
        let mut seen: Vec<String> = Vec::with_capacity(regions.len());

        for r in regions {
            let Some(pixels) = pixel_region(r, width, height) else {
                continue;
            };
            let (px, py, pw, ph) = (pixels.x, pixels.y, pixels.width, pixels.height);
            let key = format!("{px}-{py}-{pw}-{ph}");
            seen.push(key.clone());

            // Cached path — short max-age so motion in the source eventually flushes.
            let cached = {
                let mut cache = self.cache.lock();
                if let Some(c) = cache.get(&key) {
                    if c.rendered.elapsed() < self.max_age {
                        Some(c.png_data_url.clone())
                    } else {
                        cache.remove(&key);
                        None
                    }
                } else {
                    None
                }
            };
            if let Some(data_url) = cached {
                payloads.push(PatchPayload {
                    id: r.id.to_string(),
                    x: r.x,
                    y: r.y,
                    width: r.width,
                    height: r.height,
                    png_data_url: data_url,
                });
                continue;
            }

            let rendered = render_one(bgra, width, height, px, py, pw, ph);
            let png = encode_png_bgra(&rendered.bytes, rendered.width, rendered.height)?;
            let data_url = format!("data:image/png;base64,{}", STANDARD.encode(&png));
            self.cache.lock().insert(
                key.clone(),
                CachedPatch {
                    png_data_url: data_url.clone(),
                    rendered: Instant::now(),
                },
            );
            payloads.push(PatchPayload {
                id: r.id.to_string(),
                x: r.x,
                y: r.y,
                width: r.width,
                height: r.height,
                png_data_url: data_url,
            });
        }

        // Evict stale entries after every render so the cache doesn't leak.
        {
            let mut cache = self.cache.lock();
            cache.retain(|k, v| seen.iter().any(|s| s == k) && v.rendered.elapsed() < self.max_age);
        }

        Ok(payloads)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
struct PixelRegion {
    x: u32,
    y: u32,
    width: u32,
    height: u32,
}

fn pixel_region(region: &NormalizedRegion, frame_w: u32, frame_h: u32) -> Option<PixelRegion> {
    if frame_w == 0 || frame_h == 0 {
        return None;
    }
    let x = (region.x * f64::from(frame_w))
        .round()
        .clamp(0.0, f64::from(frame_w)) as u32;
    let y = (region.y * f64::from(frame_h))
        .round()
        .clamp(0.0, f64::from(frame_h)) as u32;
    if x >= frame_w || y >= frame_h {
        return None;
    }
    let width = (region.width * f64::from(frame_w))
        .round()
        .clamp(0.0, f64::from(frame_w - x)) as u32;
    let height = (region.height * f64::from(frame_h))
        .round()
        .clamp(0.0, f64::from(frame_h - y)) as u32;
    (width >= 2 && height >= 2).then_some(PixelRegion {
        x,
        y,
        width,
        height,
    })
}

fn inpaint_gpu(
    gpu: &Arc<GpuInpainter>,
    cache: &Mutex<HashMap<String, CachedPatch>>,
    max_age: Duration,
    frame_bgra: &[u8],
    frame_w: u32,
    frame_h: u32,
    regions: &[NormalizedRegion],
) -> Result<Vec<PatchPayload>> {
    if regions.is_empty() || frame_w == 0 || frame_h == 0 {
        return Ok(Vec::new());
    }

    struct Prepared<'a> {
        region: &'a NormalizedRegion,
        pixels: PixelRegion,
        key: String,
        cached: Option<String>,
        gpu_request: Option<GpuPatchRequest>,
    }

    let mut prepared = Vec::with_capacity(regions.len());
    let mut gpu_requests = Vec::new();
    for region in regions {
        let Some(pixels) = pixel_region(region, frame_w, frame_h) else {
            continue;
        };
        let key = format!(
            "{}-{}-{}-{}",
            pixels.x, pixels.y, pixels.width, pixels.height
        );
        let cached = {
            let mut cache = cache.lock();
            match cache.get(&key) {
                Some(value) if value.rendered.elapsed() < max_age => {
                    Some(value.png_data_url.clone())
                }
                Some(_) => {
                    cache.remove(&key);
                    None
                }
                None => None,
            }
        };
        let gpu_request = if cached.is_none() {
            select_gpu_axis(pixels, frame_w, frame_h).map(|(axis, far_available)| {
                let request = GpuPatchRequest {
                    pixels,
                    axis,
                    far_available,
                };
                gpu_requests.push(request);
                request
            })
        } else {
            None
        };
        prepared.push(Prepared {
            region,
            pixels,
            key,
            cached,
            gpu_request,
        });
    }

    let gpu_patches = render_gpu_bounded(gpu.clone(), frame_bgra, frame_w, frame_h, &gpu_requests)?;
    let mut rendered = HashMap::new();
    for (request, patch) in gpu_requests.iter().zip(gpu_patches) {
        rendered.insert(request.pixels, patch);
    }
    for item in &prepared {
        if item.cached.is_none() && item.gpu_request.is_none() {
            rendered.insert(
                item.pixels,
                render_one(
                    frame_bgra,
                    frame_w,
                    frame_h,
                    item.pixels.x,
                    item.pixels.y,
                    item.pixels.width,
                    item.pixels.height,
                ),
            );
        }
    }

    let mut payloads = Vec::with_capacity(prepared.len());
    let mut seen = Vec::with_capacity(prepared.len());
    for item in prepared {
        seen.push(item.key.clone());
        let data_url = match item.cached {
            Some(value) => value,
            None => {
                let patch = rendered
                    .get(&item.pixels)
                    .context("missing rendered wgpu patch")?;
                let png = encode_png_bgra(&patch.bytes, patch.width, patch.height)?;
                let data_url = format!("data:image/png;base64,{}", STANDARD.encode(&png));
                cache.lock().insert(
                    item.key.clone(),
                    CachedPatch {
                        png_data_url: data_url.clone(),
                        rendered: Instant::now(),
                    },
                );
                data_url
            }
        };
        payloads.push(PatchPayload {
            id: item.region.id.to_string(),
            x: item.region.x,
            y: item.region.y,
            width: item.region.width,
            height: item.region.height,
            png_data_url: data_url,
        });
    }
    cache.lock().retain(|key, value| {
        seen.iter().any(|seen_key| seen_key == key) && value.rendered.elapsed() < max_age
    });
    Ok(payloads)
}

fn render_gpu_bounded(
    gpu: Arc<GpuInpainter>,
    frame_bgra: &[u8],
    frame_w: u32,
    frame_h: u32,
    requests: &[GpuPatchRequest],
) -> Result<Vec<RawPatch>> {
    if requests.is_empty() {
        return Ok(Vec::new());
    }
    let frame = frame_bgra.to_vec();
    let requests = requests.to_vec();
    let (sender, receiver) = mpsc::sync_channel(1);
    thread::spawn(move || {
        let result = gpu.render(&frame, frame_w, frame_h, &requests);
        let _ = sender.send(result);
    });
    match receiver.recv_timeout(Duration::from_millis(300)) {
        Ok(result) => result,
        Err(mpsc::RecvTimeoutError::Timeout) => {
            Err(anyhow!("wgpu dispatch/readback worker exceeded 300 ms"))
        }
        Err(mpsc::RecvTimeoutError::Disconnected) => {
            Err(anyhow!("wgpu dispatch/readback worker stopped"))
        }
    }
}

#[derive(Debug, Copy, Clone)]
enum Axis {
    Vertical,
    Horizontal,
}

struct RawPatch {
    bytes: Vec<u8>, // BGRA, packed
    width: u32,
    height: u32,
}

fn render_one(bgra: &[u8], src_w: u32, src_h: u32, rx: u32, ry: u32, rw: u32, rh: u32) -> RawPatch {
    let aspect = rw as f32 / rh.max(1) as f32;
    let prefer_vertical = aspect >= 1.0;
    let order: &[Axis] = if prefer_vertical {
        &[Axis::Vertical, Axis::Horizontal]
    } else {
        &[Axis::Horizontal, Axis::Vertical]
    };
    for &axis in order {
        if let Some(p) = mirror_blend(bgra, src_w, src_h, rx, ry, rw, rh, axis) {
            return p;
        }
    }
    let color = average_border_color(bgra, src_w, src_h, rx, ry, rw, rh);
    solid_fill(rw, rh, color)
}

fn mirror_blend(
    bgra: &[u8],
    src_w: u32,
    src_h: u32,
    rx: u32,
    ry: u32,
    rw: u32,
    rh: u32,
    axis: Axis,
) -> Option<RawPatch> {
    let (near_avail, far_avail, sample_near, sample_far): (
        bool,
        bool,
        Box<dyn Fn(u32, u32) -> (u32, u32)>,
        Box<dyn Fn(u32, u32) -> (u32, u32)>,
    ) = match axis {
        Axis::Vertical => {
            let near_y = ry.checked_sub(rh)?;
            let far_y = ry + rh;
            let near_avail = near_y + rh <= src_h;
            let far_avail = far_y + rh <= src_h;
            (
                near_avail,
                far_avail,
                Box::new(move |dx: u32, dy: u32| {
                    let sy = near_y + (rh - 1 - dy);
                    (rx + dx, sy)
                }),
                Box::new(move |dx: u32, dy: u32| {
                    let sy = far_y.saturating_add(rh.saturating_sub(1).saturating_sub(dy));
                    (rx + dx, sy.min(src_h - 1))
                }),
            )
        }
        Axis::Horizontal => {
            let near_x = rx.checked_sub(rw)?;
            let far_x = rx + rw;
            let near_avail = near_x + rw <= src_w;
            let far_avail = far_x + rw <= src_w;
            (
                near_avail,
                far_avail,
                Box::new(move |dx: u32, dy: u32| {
                    let sx = near_x + (rw - 1 - dx);
                    (sx, ry + dy)
                }),
                Box::new(move |dx: u32, dy: u32| {
                    let sx = far_x.saturating_add(rw.saturating_sub(1).saturating_sub(dx));
                    (sx.min(src_w - 1), ry + dy)
                }),
            )
        }
    };

    if !near_avail && !far_avail {
        return None;
    }

    let mut out = vec![0u8; (rw * rh * 4) as usize];
    let row_src = (src_w * 4) as usize;
    for dy in 0..rh {
        for dx in 0..rw {
            // Linear gradient mask: 1.0 at near edge → 0.0 at far edge.
            let t: f32 = match axis {
                Axis::Vertical => 1.0 - (dy as f32 / (rh.max(1) as f32 - 1.0).max(1.0)),
                Axis::Horizontal => 1.0 - (dx as f32 / (rw.max(1) as f32 - 1.0).max(1.0)),
            };
            let near = if near_avail {
                let (sx, sy) = sample_near(dx, dy);
                sample_bgra(bgra, sx, sy, src_w, row_src)
            } else {
                [0, 0, 0, 255]
            };
            let far = if far_avail {
                let (sx, sy) = sample_far(dx, dy);
                sample_bgra(bgra, sx, sy, src_w, row_src)
            } else {
                [0, 0, 0, 255]
            };
            let pixel = if near_avail && far_avail {
                blend(&near, &far, t)
            } else if near_avail {
                near
            } else {
                far
            };
            let off = ((dy * rw + dx) * 4) as usize;
            out[off..off + 4].copy_from_slice(&pixel);
        }
    }
    Some(RawPatch {
        bytes: out,
        width: rw,
        height: rh,
    })
}

fn sample_bgra(bgra: &[u8], x: u32, y: u32, _src_w: u32, row_src: usize) -> [u8; 4] {
    let off = (y as usize) * row_src + (x as usize) * 4;
    if off + 4 > bgra.len() {
        return [0, 0, 0, 255];
    }
    [bgra[off], bgra[off + 1], bgra[off + 2], 255]
}

fn blend(a: &[u8; 4], b: &[u8; 4], t: f32) -> [u8; 4] {
    let lerp = |x: u8, y: u8| -> u8 {
        let r = x as f32 * t + y as f32 * (1.0 - t);
        r.clamp(0.0, 255.0) as u8
    };
    [lerp(a[0], b[0]), lerp(a[1], b[1]), lerp(a[2], b[2]), 255]
}

fn average_border_color(
    bgra: &[u8],
    src_w: u32,
    src_h: u32,
    rx: u32,
    ry: u32,
    rw: u32,
    rh: u32,
) -> [u8; 4] {
    let inset = (rw.min(rh) / 25).max(4);
    let mut sum = [0u64; 3];
    let mut count: u64 = 0;
    let row_src = (src_w * 4) as usize;
    for sy in ry.saturating_sub(inset)..ry {
        for sx in rx..(rx + rw).min(src_w) {
            let p = sample_bgra(bgra, sx, sy, src_w, row_src);
            sum[0] += p[0] as u64;
            sum[1] += p[1] as u64;
            sum[2] += p[2] as u64;
            count += 1;
        }
    }
    for sy in (ry + rh)..(ry + rh + inset).min(src_h) {
        for sx in rx..(rx + rw).min(src_w) {
            let p = sample_bgra(bgra, sx, sy, src_w, row_src);
            sum[0] += p[0] as u64;
            sum[1] += p[1] as u64;
            sum[2] += p[2] as u64;
            count += 1;
        }
    }
    if count == 0 {
        return [0, 0, 0, 255];
    }
    [
        (sum[0] / count) as u8,
        (sum[1] / count) as u8,
        (sum[2] / count) as u8,
        255,
    ]
}

fn solid_fill(width: u32, height: u32, color: [u8; 4]) -> RawPatch {
    let mut bytes = Vec::with_capacity((width * height * 4) as usize);
    for _ in 0..(width * height) {
        bytes.extend_from_slice(&color);
    }
    RawPatch {
        bytes,
        width,
        height,
    }
}

/// PNG-encode a packed BGRA buffer. Swaps to RGBA for the encoder.
fn encode_png_bgra(bgra: &[u8], width: u32, height: u32) -> Result<Vec<u8>> {
    let mut rgba = Vec::with_capacity(bgra.len());
    for c in bgra.chunks_exact(4) {
        rgba.extend_from_slice(&[c[2], c[1], c[0], c[3]]);
    }
    let mut buf: Vec<u8> = Vec::new();
    {
        let mut cursor = std::io::Cursor::new(&mut buf);
        let img = image::ImageBuffer::<image::Rgba<u8>, _>::from_raw(width, height, rgba)
            .ok_or_else(|| anyhow::anyhow!("invalid raw buffer"))?;
        image::DynamicImage::ImageRgba8(img).write_to(&mut cursor, image::ImageFormat::Png)?;
    }
    Ok(buf)
}

#[derive(Debug, Clone, Copy)]
struct GpuPatchRequest {
    pixels: PixelRegion,
    axis: Axis,
    far_available: bool,
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct GpuParams {
    frame_width: u32,
    frame_height: u32,
    region_count: u32,
    _padding: u32,
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct GpuRegion {
    x: u32,
    y: u32,
    width: u32,
    height: u32,
    output_offset: u32,
    axis: u32,
    far_available: u32,
    _padding: u32,
}

struct GpuInpainter {
    device: wgpu::Device,
    queue: wgpu::Queue,
    pipeline: wgpu::ComputePipeline,
    bind_group_layout: wgpu::BindGroupLayout,
    max_storage_binding_size: u64,
    max_workgroups_per_dimension: u32,
    backend_name: String,
    failed: Arc<AtomicBool>,
}

impl GpuInpainter {
    async fn new() -> Result<Self> {
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
            backends: wgpu::Backends::VULKAN | wgpu::Backends::GL,
            ..Default::default()
        });
        let adapter = instance
            .request_adapter(&wgpu::RequestAdapterOptions {
                power_preference: wgpu::PowerPreference::HighPerformance,
                force_fallback_adapter: false,
                compatible_surface: None,
            })
            .await
            .context("no Vulkan/GL adapter")?;
        let adapter_info = adapter.get_info();
        let limits = adapter.limits();
        if limits.max_storage_buffers_per_shader_stage < 3
            || limits.max_compute_workgroups_per_dimension == 0
        {
            return Err(anyhow!(
                "adapter lacks required compute storage-buffer limits"
            ));
        }
        let (device, queue) = adapter
            .request_device(&wgpu::DeviceDescriptor::default(), None)
            .await
            .context("request wgpu compute device")?;
        let device_limits = device.limits();
        let failed = Arc::new(AtomicBool::new(false));
        let uncaptured_failure = failed.clone();
        device.on_uncaptured_error(Box::new(move |error| {
            uncaptured_failure.store(true, Ordering::Release);
            tracing::error!("uncaptured Linux wgpu inpainting error: {error}");
        }));
        let lost_failure = failed.clone();
        device.set_device_lost_callback(move |reason, message| {
            lost_failure.store(true, Ordering::Release);
            tracing::error!("Linux wgpu device lost ({reason:?}): {message}");
        });
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("LiveBlock mirror-blend WGSL"),
            source: wgpu::ShaderSource::Wgsl(include_str!("inpainting.wgsl").into()),
        });
        let bind_group_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("LiveBlock inpainting bind group"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::COMPUTE,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::COMPUTE,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Storage { read_only: true },
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::COMPUTE,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Storage { read_only: true },
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 3,
                    visibility: wgpu::ShaderStages::COMPUTE,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Storage { read_only: false },
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
            ],
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("LiveBlock inpainting pipeline layout"),
            bind_group_layouts: &[&bind_group_layout],
            push_constant_ranges: &[],
        });
        let pipeline = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
            label: Some("LiveBlock mirror-blend pipeline"),
            layout: Some(&pipeline_layout),
            module: &shader,
            entry_point: "inpaint",
            compilation_options: Default::default(),
            cache: None,
        });
        let _ = device.poll(wgpu::Maintain::Poll);
        if failed.load(Ordering::Acquire) {
            return Err(anyhow!("wgpu inpainting pipeline validation failed"));
        }
        Ok(Self {
            device,
            queue,
            pipeline,
            bind_group_layout,
            max_storage_binding_size: u64::from(device_limits.max_storage_buffer_binding_size)
                .min(device_limits.max_buffer_size),
            max_workgroups_per_dimension: device_limits.max_compute_workgroups_per_dimension,
            backend_name: format!("{:?}", adapter_info.backend).to_lowercase(),
            failed,
        })
    }

    fn render(
        &self,
        frame_bgra: &[u8],
        frame_w: u32,
        frame_h: u32,
        requests: &[GpuPatchRequest],
    ) -> Result<Vec<RawPatch>> {
        if requests.is_empty() {
            return Ok(Vec::new());
        }
        if self.failed.load(Ordering::Acquire) {
            return Err(anyhow!("wgpu device is in a failed state"));
        }
        let expected_frame_bytes = u64::from(frame_w)
            .checked_mul(u64::from(frame_h))
            .and_then(|pixels| pixels.checked_mul(4))
            .context("frame byte size overflow")?;
        if expected_frame_bytes != frame_bgra.len() as u64
            || expected_frame_bytes > self.max_storage_binding_size
        {
            return Err(anyhow!("frame exceeds wgpu storage-buffer limits"));
        }
        if requests.len() as u32 > self.max_workgroups_per_dimension {
            return Err(anyhow!("region count exceeds wgpu dispatch limits"));
        }

        let mut gpu_regions = Vec::with_capacity(requests.len());
        let mut output_pixels = 0u64;
        let mut max_width = 0u32;
        let mut max_height = 0u32;
        for request in requests {
            let patch_pixels = u64::from(request.pixels.width)
                .checked_mul(u64::from(request.pixels.height))
                .context("patch size overflow")?;
            let output_offset =
                u32::try_from(output_pixels).context("wgpu output offset overflow")?;
            gpu_regions.push(GpuRegion {
                x: request.pixels.x,
                y: request.pixels.y,
                width: request.pixels.width,
                height: request.pixels.height,
                output_offset,
                axis: match request.axis {
                    Axis::Vertical => 0,
                    Axis::Horizontal => 1,
                },
                far_available: u32::from(request.far_available),
                _padding: 0,
            });
            output_pixels = output_pixels
                .checked_add(patch_pixels)
                .context("combined patch size overflow")?;
            max_width = max_width.max(request.pixels.width);
            max_height = max_height.max(request.pixels.height);
        }
        let regions_bytes = u64::try_from(gpu_regions.len())
            .ok()
            .and_then(|count| count.checked_mul(std::mem::size_of::<GpuRegion>() as u64))
            .context("region buffer size overflow")?;
        if regions_bytes > self.max_storage_binding_size {
            return Err(anyhow!(
                "region metadata exceeds wgpu storage-buffer limits"
            ));
        }
        let output_bytes = output_pixels
            .checked_mul(4)
            .context("output byte size overflow")?;
        if output_bytes == 0 || output_bytes > self.max_storage_binding_size {
            return Err(anyhow!("patch output exceeds wgpu storage-buffer limits"));
        }
        let dispatch_x = max_width.div_ceil(8);
        let dispatch_y = max_height.div_ceil(8);
        if dispatch_x > self.max_workgroups_per_dimension
            || dispatch_y > self.max_workgroups_per_dimension
        {
            return Err(anyhow!("patch dimensions exceed wgpu dispatch limits"));
        }

        let params = GpuParams {
            frame_width: frame_w,
            frame_height: frame_h,
            region_count: requests.len() as u32,
            _padding: 0,
        };
        let params_buffer = self
            .device
            .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("LiveBlock inpainting params"),
                contents: bytemuck::bytes_of(&params),
                usage: wgpu::BufferUsages::UNIFORM,
            });
        let source_buffer = self
            .device
            .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("LiveBlock BGRA source"),
                contents: frame_bgra,
                usage: wgpu::BufferUsages::STORAGE,
            });
        let regions_buffer = self
            .device
            .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("LiveBlock inpainting regions"),
                contents: bytemuck::cast_slice(&gpu_regions),
                usage: wgpu::BufferUsages::STORAGE,
            });
        let output_buffer = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("LiveBlock inpainting output"),
            size: output_bytes,
            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC,
            mapped_at_creation: false,
        });
        let readback = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("LiveBlock inpainting readback"),
            size: output_bytes,
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        let bind_group = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("LiveBlock inpainting bindings"),
            layout: &self.bind_group_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: params_buffer.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: source_buffer.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: regions_buffer.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 3,
                    resource: output_buffer.as_entire_binding(),
                },
            ],
        });
        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("LiveBlock inpainting commands"),
            });
        {
            let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
                label: Some("LiveBlock mirror-blend dispatch"),
                timestamp_writes: None,
            });
            pass.set_pipeline(&self.pipeline);
            pass.set_bind_group(0, &bind_group, &[]);
            pass.dispatch_workgroups(dispatch_x, dispatch_y, requests.len() as u32);
        }
        encoder.copy_buffer_to_buffer(&output_buffer, 0, &readback, 0, output_bytes);
        self.queue.submit([encoder.finish()]);

        let slice = readback.slice(..);
        let (sender, receiver) = mpsc::sync_channel(1);
        slice.map_async(wgpu::MapMode::Read, move |result| {
            let _ = sender.send(result);
        });
        let deadline = Instant::now() + Duration::from_millis(250);
        let map_result = loop {
            let _ = self.device.poll(wgpu::Maintain::Poll);
            match receiver.try_recv() {
                Ok(result) => break result,
                Err(mpsc::TryRecvError::Disconnected) => {
                    return Err(anyhow!("wgpu map callback disconnected"));
                }
                Err(mpsc::TryRecvError::Empty) if Instant::now() >= deadline => {
                    return Err(anyhow!("wgpu readback exceeded 250 ms"));
                }
                Err(mpsc::TryRecvError::Empty) => thread::sleep(Duration::from_millis(1)),
            }
        };
        map_result.context("map wgpu inpainting output")?;
        if self.failed.load(Ordering::Acquire) {
            return Err(anyhow!("wgpu device failed during inpainting dispatch"));
        }
        let mapped = slice.get_mapped_range();
        if mapped.len() != output_bytes as usize {
            return Err(anyhow!("wgpu readback length mismatch"));
        }
        let output = mapped.to_vec();
        drop(mapped);
        readback.unmap();

        let mut patches = Vec::with_capacity(requests.len());
        let mut offset = 0usize;
        for request in requests {
            let length = request.pixels.width as usize * request.pixels.height as usize * 4;
            let end = offset
                .checked_add(length)
                .context("patch readback offset overflow")?;
            let bytes = output
                .get(offset..end)
                .context("patch readback was truncated")?
                .to_vec();
            patches.push(RawPatch {
                bytes,
                width: request.pixels.width,
                height: request.pixels.height,
            });
            offset = end;
        }
        Ok(patches)
    }
}

fn select_gpu_axis(pixels: PixelRegion, frame_w: u32, frame_h: u32) -> Option<(Axis, bool)> {
    let aspect = pixels.width as f32 / pixels.height.max(1) as f32;
    let order = if aspect >= 1.0 {
        [Axis::Vertical, Axis::Horizontal]
    } else {
        [Axis::Horizontal, Axis::Vertical]
    };
    for axis in order {
        match axis {
            Axis::Vertical if pixels.y >= pixels.height => {
                let far_available = pixels
                    .y
                    .checked_add(pixels.height.saturating_mul(2))
                    .is_some_and(|end| end <= frame_h);
                return Some((axis, far_available));
            }
            Axis::Horizontal if pixels.x >= pixels.width => {
                let far_available = pixels
                    .x
                    .checked_add(pixels.width.saturating_mul(2))
                    .is_some_and(|end| end <= frame_w);
                return Some((axis, far_available));
            }
            _ => {}
        }
    }
    None
}

pub type SharedInpainter = Arc<Mutex<Inpainter>>;

#[cfg(test)]
mod tests {
    use super::*;
    use uuid::Uuid;

    fn synthetic_frame(width: u32, height: u32) -> Vec<u8> {
        let mut frame = Vec::with_capacity((width * height * 4) as usize);
        for y in 0..height {
            for x in 0..width {
                frame.extend_from_slice(&[
                    (x * 13 + y * 3) as u8,
                    (x * 5 + y * 11) as u8,
                    (x * 7 + y * 17) as u8,
                    255,
                ]);
            }
        }
        frame
    }

    #[test]
    fn wgsl_shader_parses_and_validates() {
        let module = naga::front::wgsl::parse_str(include_str!("inpainting.wgsl"))
            .expect("inpainting WGSL must parse");
        naga::valid::Validator::new(
            naga::valid::ValidationFlags::all(),
            naga::valid::Capabilities::all(),
        )
        .validate(&module)
        .expect("inpainting WGSL must validate");
    }

    #[test]
    fn pixel_regions_clamp_untrusted_deserialized_values() {
        let region = NormalizedRegion {
            id: Uuid::nil(),
            x: 0.75,
            y: 0.75,
            width: 0.75,
            height: 0.75,
        };
        assert_eq!(
            pixel_region(&region, 100, 80),
            Some(PixelRegion {
                x: 75,
                y: 60,
                width: 25,
                height: 20,
            })
        );
    }

    #[test]
    fn gpu_axis_selection_matches_cpu_fallback_order() {
        let wide = PixelRegion {
            x: 4,
            y: 4,
            width: 4,
            height: 2,
        };
        let (axis, far) = select_gpu_axis(wide, 12, 12).expect("vertical band");
        assert!(matches!(axis, Axis::Vertical));
        assert!(far);

        let edge = PixelRegion {
            x: 1,
            y: 1,
            width: 4,
            height: 4,
        };
        assert!(select_gpu_axis(edge, 12, 12).is_none());
    }

    #[test]
    fn available_gpu_matches_cpu_mirror_blend() {
        let Ok(gpu) = pollster::block_on(GpuInpainter::new()) else {
            // Headless builders without Vulkan/GL exercise the mandatory CPU fallback.
            return;
        };
        let frame = synthetic_frame(12, 12);
        let pixels = [
            PixelRegion {
                x: 4,
                y: 4,
                width: 4,
                height: 2,
            },
            PixelRegion {
                x: 4,
                y: 4,
                width: 2,
                height: 4,
            },
            PixelRegion {
                x: 9,
                y: 4,
                width: 2,
                height: 4,
            },
        ];
        let requests: Vec<_> = pixels
            .iter()
            .map(|pixels| {
                let (axis, far_available) = select_gpu_axis(*pixels, 12, 12).unwrap();
                GpuPatchRequest {
                    pixels: *pixels,
                    axis,
                    far_available,
                }
            })
            .collect();
        assert!(!requests[2].far_available);
        let gpu_patches = gpu.render(&frame, 12, 12, &requests).unwrap();
        for (pixels, gpu_patch) in pixels.iter().zip(gpu_patches) {
            let cpu_patch = render_one(
                &frame,
                12,
                12,
                pixels.x,
                pixels.y,
                pixels.width,
                pixels.height,
            );
            assert_eq!(gpu_patch.bytes.len(), cpu_patch.bytes.len());
            for (index, (gpu_byte, cpu_byte)) in
                gpu_patch.bytes.iter().zip(&cpu_patch.bytes).enumerate()
            {
                if index % 4 == 3 {
                    assert_eq!(*gpu_byte, 255);
                } else {
                    assert!(gpu_byte.abs_diff(*cpu_byte) <= 1);
                }
            }
        }
        assert!(gpu.render(&frame[..8], 12, 12, &requests).is_err());
    }
}
