//! Mirror-blend inpainting with bounded D3D11 compute and CPU fallback.
//!
//! Compute runs on an isolated D3D11 device using a CPU upload so a hung shader
//! cannot retain the WGC immediate context. Raw patches are read back for
//! PNG/webview composition; this is neither zero-copy nor texture inference.

use anyhow::Result;
use base64::{engine::general_purpose::STANDARD, Engine as _};
use parking_lot::Mutex;
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

#[cfg(windows)]
use crate::capture::FrameView;
use crate::regions::NormalizedRegion;

#[cfg(windows)]
use anyhow::{anyhow, Context};
#[cfg(windows)]
use std::sync::OnceLock;
#[cfg(windows)]
use windows::core::s;
#[cfg(windows)]
use windows::Win32::Foundation::HMODULE;
#[cfg(windows)]
use windows::Win32::Graphics::Direct3D::Fxc::{
    D3DCompile, D3DCOMPILE_ENABLE_STRICTNESS, D3DCOMPILE_OPTIMIZATION_LEVEL3,
    D3DCOMPILE_WARNINGS_ARE_ERRORS,
};
#[cfg(all(windows, test))]
use windows::Win32::Graphics::Direct3D::D3D_DRIVER_TYPE_WARP;
#[cfg(windows)]
use windows::Win32::Graphics::Direct3D::{
    ID3DBlob, D3D_DRIVER_TYPE, D3D_DRIVER_TYPE_HARDWARE, D3D_FEATURE_LEVEL_11_0,
};
#[cfg(windows)]
use windows::Win32::Graphics::Direct3D11::{
    D3D11CreateDevice, ID3D11Buffer, ID3D11ComputeShader, ID3D11Device, ID3D11DeviceContext,
    ID3D11ShaderResourceView, ID3D11Texture2D, ID3D11UnorderedAccessView,
    D3D11_BIND_CONSTANT_BUFFER, D3D11_BIND_SHADER_RESOURCE, D3D11_BIND_UNORDERED_ACCESS,
    D3D11_BUFFER_DESC, D3D11_CPU_ACCESS_READ, D3D11_CREATE_DEVICE_BGRA_SUPPORT,
    D3D11_MAPPED_SUBRESOURCE, D3D11_MAP_READ, D3D11_SDK_VERSION, D3D11_SUBRESOURCE_DATA,
    D3D11_TEXTURE2D_DESC, D3D11_USAGE_DEFAULT, D3D11_USAGE_IMMUTABLE, D3D11_USAGE_STAGING,
};
#[cfg(windows)]
use windows::Win32::Graphics::Dxgi::Common::{
    DXGI_FORMAT_B8G8R8A8_UNORM, DXGI_FORMAT_R8G8B8A8_UNORM, DXGI_SAMPLE_DESC,
};

#[derive(Debug, Clone, serde::Serialize)]
pub struct PatchPayload {
    pub id: String,
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
    /// data:image/png;base64,...
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
    gpu_attempted: AtomicBool,
    gpu_active: AtomicBool,
    gpu_disabled: AtomicBool,
}

impl Inpainter {
    pub fn new() -> Self {
        Self {
            cache: Mutex::new(HashMap::new()),
            max_age: Duration::from_millis(400),
            gpu_attempted: AtomicBool::new(false),
            gpu_active: AtomicBool::new(false),
            gpu_disabled: AtomicBool::new(false),
        }
    }

    #[cfg(windows)]
    pub fn backend_status(&self) -> &'static str {
        if self.gpu_active.load(Ordering::Acquire) {
            "d3d11_compute_readback"
        } else if self.gpu_disabled.load(Ordering::Acquire) {
            "cpu_fallback_after_d3d11_failure"
        } else if self.gpu_attempted.load(Ordering::Acquire) {
            "d3d11_initializing"
        } else {
            "cpu_until_d3d11_first_patch"
        }
    }

    /// Prefer a bounded D3D11 compute/readback attempt on an isolated device.
    /// The packed CPU frame is uploaded so a stuck compute driver cannot hold
    /// the WGC capture device's immediate context. Any 300 ms caller timeout
    /// permanently disables GPU work for this inpainter instance and rerenders
    /// on CPU. At most one timed-out detached GPU call can remain in flight.
    #[cfg(windows)]
    pub fn render_frame(
        &self,
        frame: &FrameView,
        regions: &[NormalizedRegion],
    ) -> Result<Vec<PatchPayload>> {
        if self.gpu_disabled.load(Ordering::Acquire) {
            return self.render(&frame.bytes, frame.width, frame.height, regions);
        }
        if let Some(cached) = self.cached_payloads(frame.width, frame.height, regions) {
            return Ok(cached);
        }

        self.gpu_attempted.store(true, Ordering::Release);
        let bgra = frame.bytes.clone();
        let regions_owned = regions.to_vec();
        let width = frame.width;
        let height = frame.height;
        let (sender, receiver) = std::sync::mpsc::sync_channel(1);
        if let Err(error) = std::thread::Builder::new()
            .name("liveblock-d3d11-inpaint".into())
            .spawn(move || {
                let _ = sender.send(render_gpu_regions(
                    &bgra,
                    width,
                    height,
                    &regions_owned,
                    D3D_DRIVER_TYPE_HARDWARE,
                ));
            })
        {
            self.disable_gpu();
            tracing::warn!("D3D11 inpainting disabled after worker start failure: {error}");
            return self.render(&frame.bytes, frame.width, frame.height, regions);
        }

        match receiver.recv_timeout(Duration::from_millis(300)) {
            Ok(Ok(patches)) => {
                self.gpu_active.store(true, Ordering::Release);
                self.payloads_from_raw(frame, regions, patches)
            }
            Ok(Err(error)) => {
                self.disable_gpu();
                tracing::warn!("D3D11 inpainting disabled after failure: {error}");
                self.render(&frame.bytes, frame.width, frame.height, regions)
            }
            Err(error) => {
                self.disable_gpu();
                tracing::warn!("D3D11 inpainting disabled after caller timeout: {error}");
                self.render(&frame.bytes, frame.width, frame.height, regions)
            }
        }
    }

    #[cfg(windows)]
    fn disable_gpu(&self) {
        self.gpu_active.store(false, Ordering::Release);
        self.gpu_disabled.store(true, Ordering::Release);
        self.cache.lock().clear();
    }

    #[cfg(windows)]
    fn cached_payloads(
        &self,
        width: u32,
        height: u32,
        regions: &[NormalizedRegion],
    ) -> Option<Vec<PatchPayload>> {
        let cache = self.cache.lock();
        let mut payloads = Vec::with_capacity(regions.len());
        for region in regions {
            let pixels = region_pixels(region, width, height)?;
            let key = pixels.cache_key();
            let cached = cache.get(&key)?;
            if cached.rendered.elapsed() >= self.max_age {
                return None;
            }
            payloads.push(PatchPayload {
                id: region.id.to_string(),
                x: region.x,
                y: region.y,
                width: region.width,
                height: region.height,
                png_data_url: cached.png_data_url.clone(),
            });
        }
        Some(payloads)
    }

    #[cfg(windows)]
    fn payloads_from_raw(
        &self,
        frame: &FrameView,
        regions: &[NormalizedRegion],
        patches: Vec<Option<RawPatch>>,
    ) -> Result<Vec<PatchPayload>> {
        let mut payloads = Vec::with_capacity(regions.len());
        let mut seen = Vec::with_capacity(regions.len());
        for (region, gpu_patch) in regions.iter().zip(patches) {
            let Some(pixels) = region_pixels(region, frame.width, frame.height) else {
                continue;
            };
            let key = pixels.cache_key();
            seen.push(key.clone());
            let patch = match gpu_patch {
                Some(patch) => patch,
                None => self.render_one(
                    &frame.bytes,
                    frame.width,
                    frame.height,
                    pixels.x,
                    pixels.y,
                    pixels.width,
                    pixels.height,
                )?,
            };
            let png = encode_png_bgra(&patch.bytes, patch.width, patch.height)?;
            let data_url = format!("data:image/png;base64,{}", STANDARD.encode(&png));
            self.cache.lock().insert(
                key,
                CachedPatch {
                    png_data_url: data_url.clone(),
                    rendered: Instant::now(),
                },
            );
            payloads.push(PatchPayload {
                id: region.id.to_string(),
                x: region.x,
                y: region.y,
                width: region.width,
                height: region.height,
                png_data_url: data_url,
            });
        }
        self.cache.lock().retain(|key, value| {
            seen.iter().any(|seen_key| seen_key == key) && value.rendered.elapsed() < self.max_age
        });
        Ok(payloads)
    }

    /// Render one PatchPayload per region. BGRA frame, top-left origin.
    pub fn render(
        &self,
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
            let px = (r.x * width as f64).round() as i64;
            let py = (r.y * height as f64).round() as i64;
            let pw = (r.width * width as f64).round() as i64;
            let ph = (r.height * height as f64).round() as i64;
            if pw < 2 || ph < 2 {
                continue;
            }
            let key = format!("{px}-{py}-{pw}-{ph}");
            seen.push(key.clone());

            let payload = {
                let mut cache = self.cache.lock();
                if let Some(c) = cache.get(&key) {
                    if c.rendered.elapsed() < self.max_age {
                        Some(PatchPayload {
                            id: r.id.to_string(),
                            x: r.x,
                            y: r.y,
                            width: r.width,
                            height: r.height,
                            png_data_url: c.png_data_url.clone(),
                        })
                    } else {
                        cache.remove(&key);
                        None
                    }
                } else {
                    None
                }
            };
            if let Some(p) = payload {
                payloads.push(p);
                continue;
            }

            // Render new patch.
            let rendered = self.render_one(
                bgra, width, height, px as u32, py as u32, pw as u32, ph as u32,
            )?;
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

        // Evict.
        {
            let mut cache = self.cache.lock();
            cache.retain(|k, v| seen.iter().any(|s| s == k) && v.rendered.elapsed() < self.max_age);
        }

        Ok(payloads)
    }

    fn render_one(
        &self,
        bgra: &[u8],
        src_w: u32,
        src_h: u32,
        rx: u32,
        ry: u32,
        rw: u32,
        rh: u32,
    ) -> Result<RawPatch> {
        // Pick axis from aspect ratio (matches macOS).
        let aspect = rw as f32 / rh.max(1) as f32;
        let prefer_vertical = aspect >= 1.0;

        // Try near/far reflections on preferred axis, then the other axis.
        for &axis in if prefer_vertical {
            &[Axis::Vertical, Axis::Horizontal][..]
        } else {
            &[Axis::Horizontal, Axis::Vertical][..]
        } {
            if let Some(p) = mirror_blend(bgra, src_w, src_h, rx, ry, rw, rh, axis) {
                return Ok(p);
            }
        }

        // Last resort: solid fill from average border colour.
        let color = average_border_color(bgra, src_w, src_h, rx, ry, rw, rh);
        Ok(solid_fill(rw, rh, color))
    }
}

#[derive(Copy, Clone)]
enum Axis {
    Vertical,
    Horizontal,
}

struct RawPatch {
    bytes: Vec<u8>, // BGRA, packed
    width: u32,
    height: u32,
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
    // Top-left origin (Windows). Bands:
    //   Vertical: near = above region, far = below region
    //   Horizontal: near = left of region, far = right of region
    let (near_avail, far_avail, sample_near, sample_far) = match axis {
        Axis::Vertical => {
            let near_y = ry.checked_sub(rh)?;
            let far_y = ry + rh;
            let near_avail = near_y + rh <= src_h;
            let far_avail = far_y + rh <= src_h;
            (
                near_avail,
                far_avail,
                Box::new(move |dx: u32, dy: u32| -> (u32, u32) {
                    // Reflect vertically across region's top edge.
                    let sy = near_y + (rh - 1 - dy);
                    (rx + dx, sy)
                }) as Box<dyn Fn(u32, u32) -> (u32, u32)>,
                Box::new(move |dx: u32, dy: u32| -> (u32, u32) {
                    // Reflect across region's bottom edge.
                    let sy = far_y.saturating_add(rh.saturating_sub(1).saturating_sub(dy));
                    (rx + dx, sy.min(src_h - 1))
                }) as Box<dyn Fn(u32, u32) -> (u32, u32)>,
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
                Box::new(move |dx: u32, dy: u32| -> (u32, u32) {
                    // Reflect horizontally across region's left edge.
                    let sx = near_x + (rw - 1 - dx);
                    (sx, ry + dy)
                }) as Box<dyn Fn(u32, u32) -> (u32, u32)>,
                Box::new(move |dx: u32, dy: u32| -> (u32, u32) {
                    let sx = far_x.saturating_add(rw.saturating_sub(1).saturating_sub(dx));
                    (sx.min(src_w - 1), ry + dy)
                }) as Box<dyn Fn(u32, u32) -> (u32, u32)>,
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
            // Linear blend mask: 1.0 at near edge → 0.0 at far edge.
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
struct RegionPixels {
    x: u32,
    y: u32,
    width: u32,
    height: u32,
}

impl RegionPixels {
    fn cache_key(self) -> String {
        format!("{}-{}-{}-{}", self.x, self.y, self.width, self.height)
    }
}

fn region_pixels(
    region: &NormalizedRegion,
    source_width: u32,
    source_height: u32,
) -> Option<RegionPixels> {
    if source_width == 0
        || source_height == 0
        || !region.x.is_finite()
        || !region.y.is_finite()
        || !region.width.is_finite()
        || !region.height.is_finite()
        || region.x < 0.0
        || region.y < 0.0
        || region.width <= 0.0
        || region.height <= 0.0
    {
        return None;
    }
    let x = (region.x * f64::from(source_width)).round() as u32;
    let y = (region.y * f64::from(source_height)).round() as u32;
    let width = (region.width * f64::from(source_width)).round() as u32;
    let height = (region.height * f64::from(source_height)).round() as u32;
    if x >= source_width || y >= source_height {
        return None;
    }
    let width = width.min(source_width - x);
    let height = height.min(source_height - y);
    if width < 2 || height < 2 {
        return None;
    }
    Some(RegionPixels {
        x,
        y,
        width,
        height,
    })
}

#[cfg(windows)]
#[repr(C)]
#[derive(Clone, Copy)]
struct ShaderConstants {
    source_width: u32,
    source_height: u32,
    region_x: u32,
    region_y: u32,
    region_width: u32,
    region_height: u32,
    axis: u32,
    near_available: u32,
    far_available: u32,
    reserved: [u32; 3],
}

#[cfg(windows)]
fn gpu_region(
    region: &NormalizedRegion,
    source_width: u32,
    source_height: u32,
) -> Option<(RegionPixels, ShaderConstants)> {
    let pixels = region_pixels(region, source_width, source_height)?;
    let prefer_vertical = pixels.width as f32 / pixels.height as f32 >= 1.0;
    let axes = if prefer_vertical {
        [0u32, 1u32]
    } else {
        [1u32, 0u32]
    };
    for axis in axes {
        let (near_available, far_available) = if axis == 0 {
            (
                pixels.y >= pixels.height,
                pixels
                    .y
                    .checked_add(pixels.height.saturating_mul(2))
                    .is_some_and(|end| end <= source_height),
            )
        } else {
            (
                pixels.x >= pixels.width,
                pixels
                    .x
                    .checked_add(pixels.width.saturating_mul(2))
                    .is_some_and(|end| end <= source_width),
            )
        };
        if near_available || far_available {
            return Some((
                pixels,
                ShaderConstants {
                    source_width,
                    source_height,
                    region_x: pixels.x,
                    region_y: pixels.y,
                    region_width: pixels.width,
                    region_height: pixels.height,
                    axis,
                    near_available: u32::from(near_available),
                    far_available: u32::from(far_available),
                    reserved: [0; 3],
                },
            ));
        }
    }
    None
}

#[cfg(windows)]
static SHADER_BYTECODE: OnceLock<std::result::Result<Vec<u8>, String>> = OnceLock::new();

#[cfg(windows)]
fn shader_bytecode() -> Result<&'static [u8]> {
    let result =
        SHADER_BYTECODE.get_or_init(|| compile_shader().map_err(|error| error.to_string()));
    result.as_deref().map_err(|error| anyhow!(error.clone()))
}

#[cfg(windows)]
fn compile_shader() -> Result<Vec<u8>> {
    let source = include_bytes!("inpainting.hlsl");
    let mut code: Option<ID3DBlob> = None;
    let mut errors: Option<ID3DBlob> = None;
    let flags = D3DCOMPILE_ENABLE_STRICTNESS
        | D3DCOMPILE_OPTIMIZATION_LEVEL3
        | D3DCOMPILE_WARNINGS_ARE_ERRORS;
    let compile_result = unsafe {
        D3DCompile(
            source.as_ptr().cast(),
            source.len(),
            s!("liveblock-inpainting.hlsl"),
            None,
            None::<&windows::Win32::Graphics::Direct3D::ID3DInclude>,
            s!("main"),
            s!("cs_5_0"),
            flags,
            0,
            &mut code,
            Some(&mut errors),
        )
    };
    if let Err(error) = compile_result {
        let details = errors
            .as_ref()
            .map(|blob| unsafe {
                let bytes = std::slice::from_raw_parts(
                    blob.GetBufferPointer().cast::<u8>(),
                    blob.GetBufferSize(),
                );
                String::from_utf8_lossy(bytes).into_owned()
            })
            .unwrap_or_default();
        return Err(anyhow!(
            "compile D3D11 inpainting shader: {error}; {details}"
        ));
    }
    let code = code.context("D3DCompile returned no shader bytecode")?;
    let bytes = unsafe {
        std::slice::from_raw_parts(code.GetBufferPointer().cast::<u8>(), code.GetBufferSize())
    };
    Ok(bytes.to_vec())
}

#[cfg(windows)]
struct ComputeResource {
    texture: ID3D11Texture2D,
    device: ID3D11Device,
    context: ID3D11DeviceContext,
}

#[cfg(windows)]
fn create_compute_resource(
    bgra: &[u8],
    source_width: u32,
    source_height: u32,
    driver_type: D3D_DRIVER_TYPE,
) -> Result<ComputeResource> {
    let expected_len = (source_width as usize)
        .checked_mul(source_height as usize)
        .and_then(|pixels| pixels.checked_mul(4))
        .context("D3D11 source dimensions overflow")?;
    if source_width == 0
        || source_height == 0
        || source_width > 16_384
        || source_height > 16_384
        || bgra.len() != expected_len
    {
        return Err(anyhow!("unsupported D3D11 source frame"));
    }
    let mut device = None;
    let mut context = None;
    unsafe {
        D3D11CreateDevice(
            None,
            driver_type,
            HMODULE::default(),
            D3D11_CREATE_DEVICE_BGRA_SUPPORT,
            Some(&[D3D_FEATURE_LEVEL_11_0]),
            D3D11_SDK_VERSION,
            Some(&mut device),
            None,
            Some(&mut context),
        )?;
    }
    let device = device.context("D3D11CreateDevice returned no compute device")?;
    let context = context.context("D3D11CreateDevice returned no compute context")?;
    let source_desc = D3D11_TEXTURE2D_DESC {
        Width: source_width,
        Height: source_height,
        MipLevels: 1,
        ArraySize: 1,
        Format: DXGI_FORMAT_B8G8R8A8_UNORM,
        SampleDesc: DXGI_SAMPLE_DESC {
            Count: 1,
            Quality: 0,
        },
        Usage: D3D11_USAGE_IMMUTABLE,
        BindFlags: D3D11_BIND_SHADER_RESOURCE.0 as u32,
        CPUAccessFlags: 0,
        MiscFlags: 0,
    };
    let source_data = D3D11_SUBRESOURCE_DATA {
        pSysMem: bgra.as_ptr().cast(),
        SysMemPitch: source_width * 4,
        SysMemSlicePitch: source_width * source_height * 4,
    };
    let mut texture = None;
    unsafe {
        device.CreateTexture2D(&source_desc, Some(&source_data), Some(&mut texture))?;
    }
    Ok(ComputeResource {
        texture: texture.context("CreateTexture2D returned no compute source")?,
        device,
        context,
    })
}

#[cfg(windows)]
fn render_gpu_regions(
    bgra: &[u8],
    source_width: u32,
    source_height: u32,
    regions: &[NormalizedRegion],
    driver_type: D3D_DRIVER_TYPE,
) -> Result<Vec<Option<RawPatch>>> {
    let resource = create_compute_resource(bgra, source_width, source_height, driver_type)?;
    let bytecode = shader_bytecode()?;
    let mut shader: Option<ID3D11ComputeShader> = None;
    unsafe {
        resource
            .device
            .CreateComputeShader(bytecode, None, Some(&mut shader))?;
    }
    let shader = shader.context("CreateComputeShader returned no shader")?;
    let mut source_view: Option<ID3D11ShaderResourceView> = None;
    unsafe {
        resource.device.CreateShaderResourceView(
            &resource.texture,
            None,
            Some(&mut source_view),
        )?;
    }
    let source_view = source_view.context("CreateShaderResourceView returned no source view")?;

    let mut output = Vec::with_capacity(regions.len());
    for region in regions {
        let Some((pixels, constants)) = gpu_region(region, source_width, source_height) else {
            output.push(None);
            continue;
        };
        output.push(Some(dispatch_region(
            &resource,
            &shader,
            &source_view,
            pixels,
            constants,
        )?));
    }
    Ok(output)
}

#[cfg(windows)]
fn dispatch_region(
    resource: &ComputeResource,
    shader: &ID3D11ComputeShader,
    source_view: &ID3D11ShaderResourceView,
    pixels: RegionPixels,
    constants: ShaderConstants,
) -> Result<RawPatch> {
    let output_desc = D3D11_TEXTURE2D_DESC {
        Width: pixels.width,
        Height: pixels.height,
        MipLevels: 1,
        ArraySize: 1,
        Format: DXGI_FORMAT_R8G8B8A8_UNORM,
        SampleDesc: DXGI_SAMPLE_DESC {
            Count: 1,
            Quality: 0,
        },
        Usage: D3D11_USAGE_DEFAULT,
        BindFlags: D3D11_BIND_UNORDERED_ACCESS.0 as u32,
        CPUAccessFlags: 0,
        MiscFlags: 0,
    };
    let mut output_texture: Option<ID3D11Texture2D> = None;
    unsafe {
        resource
            .device
            .CreateTexture2D(&output_desc, None, Some(&mut output_texture))?;
    }
    let output_texture = output_texture.context("CreateTexture2D returned no compute output")?;
    let mut output_view: Option<ID3D11UnorderedAccessView> = None;
    unsafe {
        resource
            .device
            .CreateUnorderedAccessView(&output_texture, None, Some(&mut output_view))?;
    }
    let output_view = output_view.context("CreateUnorderedAccessView returned no output view")?;

    let constant_desc = D3D11_BUFFER_DESC {
        ByteWidth: std::mem::size_of::<ShaderConstants>() as u32,
        Usage: D3D11_USAGE_IMMUTABLE,
        BindFlags: D3D11_BIND_CONSTANT_BUFFER.0 as u32,
        CPUAccessFlags: 0,
        MiscFlags: 0,
        StructureByteStride: 0,
    };
    let constant_data = D3D11_SUBRESOURCE_DATA {
        pSysMem: (&constants as *const ShaderConstants).cast(),
        SysMemPitch: 0,
        SysMemSlicePitch: 0,
    };
    let mut constant_buffer: Option<ID3D11Buffer> = None;
    unsafe {
        resource.device.CreateBuffer(
            &constant_desc,
            Some(&constant_data),
            Some(&mut constant_buffer),
        )?;
    }
    let constant_buffer = constant_buffer.context("CreateBuffer returned no constants")?;

    let source_views = [Some(source_view.clone())];
    let constant_buffers = [Some(constant_buffer)];
    let output_views = [Some(output_view)];
    unsafe {
        resource.context.CSSetShader(shader, None);
        resource
            .context
            .CSSetShaderResources(0, Some(&source_views));
        resource
            .context
            .CSSetConstantBuffers(0, Some(&constant_buffers));
        resource
            .context
            .CSSetUnorderedAccessViews(0, 1, Some(output_views.as_ptr()), None);
        resource
            .context
            .Dispatch(pixels.width.div_ceil(8), pixels.height.div_ceil(8), 1);
        let empty_resources: [Option<ID3D11ShaderResourceView>; 1] = [None];
        let empty_outputs: [Option<ID3D11UnorderedAccessView>; 1] = [None];
        resource
            .context
            .CSSetShaderResources(0, Some(&empty_resources));
        resource
            .context
            .CSSetUnorderedAccessViews(0, 1, Some(empty_outputs.as_ptr()), None);
        resource
            .context
            .CSSetShader(None::<&ID3D11ComputeShader>, None);
    }

    let staging_desc = D3D11_TEXTURE2D_DESC {
        BindFlags: 0,
        Usage: D3D11_USAGE_STAGING,
        CPUAccessFlags: D3D11_CPU_ACCESS_READ.0 as u32,
        ..output_desc
    };
    let mut staging: Option<ID3D11Texture2D> = None;
    unsafe {
        resource
            .device
            .CreateTexture2D(&staging_desc, None, Some(&mut staging))?;
    }
    let staging = staging.context("CreateTexture2D returned no patch staging texture")?;
    unsafe {
        resource.context.CopyResource(&staging, &output_texture);
    }
    let mut mapped = D3D11_MAPPED_SUBRESOURCE::default();
    unsafe {
        resource
            .context
            .Map(&staging, 0, D3D11_MAP_READ, 0, Some(&mut mapped))?;
    }
    let row_bytes = pixels.width as usize * 4;
    let mut bgra = vec![0u8; row_bytes * pixels.height as usize];
    unsafe {
        let source = mapped.pData.cast::<u8>();
        for row in 0..pixels.height as usize {
            let source_row = source.add(row * mapped.RowPitch as usize);
            let destination_row = &mut bgra[row * row_bytes..(row + 1) * row_bytes];
            for column in 0..pixels.width as usize {
                let source_pixel = source_row.add(column * 4);
                let destination = &mut destination_row[column * 4..column * 4 + 4];
                destination.copy_from_slice(&[
                    *source_pixel.add(2),
                    *source_pixel.add(1),
                    *source_pixel,
                    255,
                ]);
            }
        }
        resource.context.Unmap(&staging, 0);
    }
    Ok(RawPatch {
        bytes: bgra,
        width: pixels.width,
        height: pixels.height,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn region_pixel_conversion_clamps_to_source() {
        let region = NormalizedRegion::new(0.8, 0.8, 0.4, 0.4);
        let pixels = region_pixels(&region, 100, 50).expect("valid clipped region");
        assert_eq!((pixels.x, pixels.y), (80, 40));
        assert_eq!((pixels.width, pixels.height), (20, 10));
    }

    #[test]
    fn malformed_regions_do_not_reach_gpu() {
        let mut region = NormalizedRegion::new(0.1, 0.1, 0.2, 0.2);
        region.x = f64::NAN;
        assert!(region_pixels(&region, 100, 100).is_none());
    }

    #[cfg(windows)]
    #[test]
    fn shader_compiles_for_d3d11_compute() {
        let bytecode = shader_bytecode().expect("D3D11 shader should compile");
        assert!(!bytecode.is_empty());
    }

    #[cfg(windows)]
    #[test]
    fn warp_dispatch_matches_cpu_mirror_policy() {
        let width = 16u32;
        let height = 16u32;
        let mut frame = Vec::with_capacity((width * height * 4) as usize);
        for y in 0..height {
            for x in 0..width {
                frame.extend_from_slice(&[
                    (x * 11) as u8,
                    (y * 13) as u8,
                    ((x + y) * 7) as u8,
                    255,
                ]);
            }
        }

        let region = NormalizedRegion::new(0.25, 0.25, 0.25, 0.25);
        let gpu = render_gpu_regions(
            &frame,
            width,
            height,
            &[region.clone()],
            D3D_DRIVER_TYPE_WARP,
        )
        .expect("WARP dispatch")
        .remove(0)
        .expect("GPU mirror patch");
        let pixels = region_pixels(&region, width, height).expect("region pixels");
        let cpu = mirror_blend(
            &frame,
            width,
            height,
            pixels.x,
            pixels.y,
            pixels.width,
            pixels.height,
            Axis::Vertical,
        )
        .expect("CPU mirror patch");
        assert_eq!((gpu.width, gpu.height), (cpu.width, cpu.height));
        assert_eq!(gpu.bytes.len(), cpu.bytes.len());
        for (gpu_byte, cpu_byte) in gpu.bytes.iter().zip(&cpu.bytes) {
            assert!(
                gpu_byte.abs_diff(*cpu_byte) <= 1,
                "GPU byte {gpu_byte} diverged from CPU byte {cpu_byte}"
            );
        }
    }
}
