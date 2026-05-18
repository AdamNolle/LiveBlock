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
//! The GPU compute path stays stubbed for now — wgpu wiring needs a Linux box
//! to validate. The CPU path here is byte-for-byte equivalent to the Windows
//! port so cross-platform output stays consistent.

use crate::regions::NormalizedRegion;
use anyhow::Result;
use base64::{engine::general_purpose::STANDARD, Engine as _};
use parking_lot::Mutex;
use serde::Serialize;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

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
    device: Option<wgpu::Device>,
    #[allow(dead_code)]
    queue: Option<wgpu::Queue>,
}

impl Inpainter {
    pub fn new() -> Self {
        let (device, queue) = pollster::block_on(init_wgpu()).unwrap_or((None, None));
        Self {
            cache: Mutex::new(HashMap::new()),
            max_age: Duration::from_millis(400),
            device,
            queue,
        }
    }

    pub fn inpaint(
        &mut self,
        frame_bgra: &[u8],
        frame_w: u32,
        frame_h: u32,
        regions: &[NormalizedRegion],
    ) -> Result<Vec<PatchPayload>> {
        if self.device.is_some() {
            // GPU path is intentionally a no-op for now; falls through to CPU.
            // See compute_shader_path note at the bottom.
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
            let px = (r.x * width as f64).round() as i64;
            let py = (r.y * height as f64).round() as i64;
            let pw = (r.width * width as f64).round() as i64;
            let ph = (r.height * height as f64).round() as i64;
            if pw < 2 || ph < 2 {
                continue;
            }
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

            let rendered = render_one(
                bgra, width, height, px as u32, py as u32, pw as u32, ph as u32,
            );
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

fn sample_bgra(bgra: &[u8], x: u32, y: u32, src_w: u32, row_src: usize) -> [u8; 4] {
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

async fn init_wgpu() -> Option<(Option<wgpu::Device>, Option<wgpu::Queue>)> {
    let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
        backends: wgpu::Backends::VULKAN | wgpu::Backends::GL,
        ..Default::default()
    });
    let adapter = instance
        .request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::HighPerformance,
            ..Default::default()
        })
        .await?;
    let (device, queue) = adapter
        .request_device(&wgpu::DeviceDescriptor::default(), None)
        .await
        .ok()?;
    Some((Some(device), Some(queue)))
}

pub type SharedInpainter = Arc<Mutex<Inpainter>>;

// TODO(linux-port-gpu): bind the WGSL pipeline at src/inpainting.wgsl, upload
// the BGRA frame as a storage texture, dispatch one workgroup per region, read
// back the per-region mirror-blend result. The CPU path above is correctness-
// complete and visually matches macOS for real content; the GPU path is a
// perf optimization. Validation needs a Linux box with a Vulkan-capable
// driver.
#[allow(dead_code)]
pub fn compute_shader_path() {}
