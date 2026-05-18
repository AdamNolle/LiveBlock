//! CPU mirror-blend port of `Sources/InpaintingEngine.swift`.
//! D3D11 compute shader path is stubbed — see `compute_shader_path` TODO.

use anyhow::Result;
use base64::{engine::general_purpose::STANDARD, Engine as _};
use parking_lot::Mutex;
use std::collections::HashMap;
use std::time::{Duration, Instant};

use crate::regions::NormalizedRegion;

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
}

impl Inpainter {
    pub fn new() -> Self {
        Self {
            cache: Mutex::new(HashMap::new()),
            max_age: Duration::from_millis(400),
        }
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
                CachedPatch { png_data_url: data_url.clone(), rendered: Instant::now() },
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
        bgra: &[u8], src_w: u32, src_h: u32,
        rx: u32, ry: u32, rw: u32, rh: u32,
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
enum Axis { Vertical, Horizontal }

struct RawPatch {
    bytes: Vec<u8>, // BGRA, packed
    width: u32,
    height: u32,
}

fn mirror_blend(
    bgra: &[u8], src_w: u32, src_h: u32,
    rx: u32, ry: u32, rw: u32, rh: u32,
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
                near_avail, far_avail,
                Box::new(move |dx: u32, dy: u32| -> (u32, u32) {
                    // Reflect vertically across region's top edge.
                    let sy = near_y + (rh - 1 - dy);
                    (rx + dx, sy)
                }) as Box<dyn Fn(u32, u32) -> (u32, u32)>,
                Box::new(move |dx: u32, dy: u32| -> (u32, u32) {
                    // Reflect across region's bottom edge.
                    let sy = far_y + dy;
                    let sy = far_y + (rh - 1) - (sy - far_y);
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
                near_avail, far_avail,
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
            } else { [0, 0, 0, 255] };

            let far = if far_avail {
                let (sx, sy) = sample_far(dx, dy);
                sample_bgra(bgra, sx, sy, src_w, row_src)
            } else { [0, 0, 0, 255] };

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

    Some(RawPatch { bytes: out, width: rw, height: rh })
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
    bgra: &[u8], src_w: u32, src_h: u32,
    rx: u32, ry: u32, rw: u32, rh: u32,
) -> [u8; 4] {
    let inset = (rw.min(rh) / 25).max(4);
    let mut sum = [0u64; 3];
    let mut count: u64 = 0;
    let row_src = (src_w * 4) as usize;
    for sy in ry.saturating_sub(inset)..ry {
        for sx in rx..(rx + rw).min(src_w) {
            let p = sample_bgra(bgra, sx, sy, src_w, row_src);
            sum[0] += p[0] as u64; sum[1] += p[1] as u64; sum[2] += p[2] as u64;
            count += 1;
        }
    }
    for sy in (ry + rh)..(ry + rh + inset).min(src_h) {
        for sx in rx..(rx + rw).min(src_w) {
            let p = sample_bgra(bgra, sx, sy, src_w, row_src);
            sum[0] += p[0] as u64; sum[1] += p[1] as u64; sum[2] += p[2] as u64;
            count += 1;
        }
    }
    if count == 0 { return [0, 0, 0, 255]; }
    [(sum[0] / count) as u8, (sum[1] / count) as u8, (sum[2] / count) as u8, 255]
}

fn solid_fill(width: u32, height: u32, color: [u8; 4]) -> RawPatch {
    let mut bytes = Vec::with_capacity((width * height * 4) as usize);
    for _ in 0..(width * height) {
        bytes.extend_from_slice(&color);
    }
    RawPatch { bytes, width, height }
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

// TODO(windows-port): D3D11 compute shader path. WGC frames already arrive as
// IDirect3DSurface; rendering the mirror reflection on the same device avoids a
// CPU roundtrip. Plan:
//   1. Author Inpaint.hlsl with three CSes (BlendVert, BlendHoriz, EdgeFill).
//   2. Compile via fxc/dxc at build time; embed .cso bytes via include_bytes!.
//   3. ID3D11Device::CreateComputeShader, bind ID3D11ShaderResourceView for
//      the source frame texture, ID3D11UnorderedAccessView for one staging
//      texture per region. Dispatch (ceil(rw/8), ceil(rh/8), 1).
//   4. CopyResource to a CPU-readable texture, Map, encode PNG.
// The CPU path above is functionally complete and visually matches macOS for
// real desktop content; the GPU path is a perf optimization, not a correctness
// fix.
#[allow(dead_code)]
pub fn compute_shader_path() {}
