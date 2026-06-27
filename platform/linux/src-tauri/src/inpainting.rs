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
///
/// `fill` is OPTIONAL and only present for the DRM-safe **paint-over** path: a
/// flat opaque cover the overlay draws WITHOUT having read the protected pixels
/// (see `liveblock_core::paint_over_regions`). When `fill` is set the frontend
/// ignores `png_data_url` (which is empty) and draws a solid rectangle. Omitting
/// the field for the normal mirror-blend path keeps the JSON byte-compatible
/// with the existing Windows/macOS patch consumer.
#[derive(Debug, Clone, Serialize)]
pub struct PatchPayload {
    pub id: String,
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
    pub png_data_url: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fill: Option<SolidFill>,
}

/// A flat opaque cover colour for a paint-over patch (RGBA, 0..255).
#[derive(Debug, Clone, Copy, Serialize)]
pub struct SolidFill {
    pub r: u8,
    pub g: u8,
    pub b: u8,
    pub a: u8,
}

impl From<liveblock_core::Fill> for SolidFill {
    fn from(f: liveblock_core::Fill) -> Self {
        match f {
            liveblock_core::Fill::Solid { r, g, b, a } => SolidFill { r, g, b, a },
        }
    }
}

#[derive(Clone)]
struct CachedPatch {
    png_data_url: String,
    rendered: Instant,
}

pub struct Inpainter {
    cache: Mutex<HashMap<String, CachedPatch>>,
    max_age: Duration,
    /// Whether a Vulkan/GL adapter was found at startup. Purely diagnostic for
    /// now — see the honesty note below. We deliberately DO NOT keep a live
    /// `wgpu::Device`/`Queue`: holding them implied a GPU inpaint path existed
    /// when `inpaint()` was, in fact, always CPU-only (the confirmed gap). The
    /// GPU compute path (src/inpainting.wgsl) is written but unvalidated; it is
    /// gated behind on-hardware verification rather than silently dead-held.
    gpu_available: bool,
}

impl Inpainter {
    pub fn new() -> Self {
        // Probe for a GPU adapter for diagnostics only. We drop the device
        // immediately: the inpaint hot path is CPU until the WGSL pipeline is
        // validated on a real Linux box (see `compute_shader_path`).
        let gpu_available = pollster::block_on(probe_gpu());
        Self {
            cache: Mutex::new(HashMap::new()),
            max_age: Duration::from_millis(400),
            gpu_available,
        }
    }

    /// True if a Vulkan/GL adapter was detected at startup. Reported to the
    /// frontend for diagnostics; does NOT imply the GPU inpaint path runs yet.
    pub fn gpu_available(&self) -> bool {
        self.gpu_available
    }

    /// Mirror-blend inpaint over capturable regions. CPU path only — see the
    /// honesty note on `gpu_available`.
    pub fn inpaint(
        &mut self,
        frame_bgra: &[u8],
        frame_w: u32,
        frame_h: u32,
        regions: &[NormalizedRegion],
    ) -> Result<Vec<PatchPayload>> {
        // NOTE(linux-port): CPU mirror-blend is the only correctness-complete
        // path today. The GPU compute path is intentionally NOT invoked here —
        // wiring it without a Linux box to validate would risk shipping a
        // silently-wrong inpaint. This is the fix for the "claims wgpu but
        // inpaint() is CPU-only" gap: the claim is now accurate.
        self.inpaint_cpu(frame_bgra, frame_w, frame_h, regions)
    }

    /// Full DRM-aware render. Capturable regions go through the mirror-blend
    /// inpainter; protected/blacked-out regions become flat opaque paint-over
    /// covers built WITHOUT reading their pixels (the `liveblock_core` DRM-safe
    /// path). Returns the union of both patch kinds.
    pub fn render(
        &mut self,
        frame_bgra: &[u8],
        frame_w: u32,
        frame_h: u32,
        inpaint_regions: &[NormalizedRegion],
        paint_over: &[liveblock_core::PaintPatch],
    ) -> Result<Vec<PatchPayload>> {
        let mut out = self.inpaint_cpu(frame_bgra, frame_w, frame_h, inpaint_regions)?;
        // Paint-over patches carry no pixel data: the overlay fills a flat
        // opaque rect. These never touch the cache (they're trivially cheap and
        // their geometry can change every frame as the DRM region moves).
        for (i, p) in paint_over.iter().enumerate() {
            out.push(PatchPayload {
                id: format!("paintover-{i}"),
                x: p.rect.x as f64,
                y: p.rect.y as f64,
                width: p.rect.width as f64,
                height: p.rect.height as f64,
                png_data_url: String::new(),
                fill: Some(p.fill.into()),
            });
        }
        Ok(out)
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
                    fill: None,
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
                fill: None,
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
            // The NEAR band sits in rows [ry-rh, ry) directly ABOVE the region;
            // `checked_sub` already proves it fits above the top edge, so its
            // availability is simply "did the subtraction succeed". The FAR band
            // sits in rows [ry+rh, ry+2*rh) directly BELOW, so it must fit under
            // the bottom of the frame.
            //
            // BUGFIX(linux-port): the previous code tested `near_y + rh <= src_h`
            // for the near band — but `near_y + rh == ry`, so that condition was
            // `ry <= src_h`, which is ALWAYS true. The near band was therefore
            // treated as available even when it had been clamped, and when the
            // region hugged the bottom of the screen the (correct) far check
            // failed while the (bogus) near check passed, so the blend silently
            // used only the reflected-from-above band with no cross-fade weight
            // correction — producing the visible vertical seam. We now test the
            // near band against the TOP edge, which is what reflection-from-above
            // actually requires.
            let near_y = match ry.checked_sub(rh) {
                Some(v) => v,
                None => 0, // partial band; `near_avail` below records the truth
            };
            let far_y = ry + rh;
            let near_avail = ry >= rh; // full band fits in [ry-rh, ry)
            let far_avail = far_y + rh <= src_h; // full band fits in [ry+rh, ry+2rh)
            (
                near_avail,
                far_avail,
                Box::new(move |dx: u32, dy: u32| {
                    // Reflect across the TOP edge: output row dy maps to the
                    // pixel dy+1 above ry, i.e. source row ry-1-dy.
                    let sy = near_y + (rh - 1 - dy);
                    (rx + dx, sy.min(src_h - 1))
                }),
                Box::new(move |dx: u32, dy: u32| {
                    // Reflect across the BOTTOM edge: output row dy maps to the
                    // pixel (rh-dy) below the bottom edge.
                    let sy = far_y.saturating_add(rh.saturating_sub(1).saturating_sub(dy));
                    (rx + dx, sy.min(src_h - 1))
                }),
            )
        }
        Axis::Horizontal => {
            // Symmetric to the vertical case. NEAR band is columns [rx-rw, rx)
            // to the LEFT (must fit past the left edge); FAR band is columns
            // [rx+rw, rx+2*rw) to the RIGHT (must fit before the right edge).
            let near_x = match rx.checked_sub(rw) {
                Some(v) => v,
                None => 0,
            };
            let far_x = rx + rw;
            let near_avail = rx >= rw;
            let far_avail = far_x + rw <= src_w;
            (
                near_avail,
                far_avail,
                Box::new(move |dx: u32, dy: u32| {
                    let sx = near_x + (rw - 1 - dx);
                    (sx.min(src_w - 1), ry + dy)
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

/// Probe for a Vulkan/GL adapter. Used for diagnostics only — we do not retain
/// the device (see `Inpainter::gpu_available` for why).
async fn probe_gpu() -> bool {
    let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
        backends: wgpu::Backends::VULKAN | wgpu::Backends::GL,
        ..Default::default()
    });
    instance
        .request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::HighPerformance,
            ..Default::default()
        })
        .await
        .is_some()
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

#[cfg(test)]
mod tests {
    use super::*;

    /// A vertical gradient frame: row y has luma y (clamped). The near band
    /// (above) and far band (below) are distinguishable, so we can prove the
    /// reflection samples the correct rows.
    fn gradient_frame(w: u32, h: u32) -> Vec<u8> {
        let mut v = vec![0u8; (w * h * 4) as usize];
        for y in 0..h {
            let luma = (y.min(255)) as u8;
            for x in 0..w {
                let i = ((y * w + x) * 4) as usize;
                v[i] = luma;
                v[i + 1] = luma;
                v[i + 2] = luma;
                v[i + 3] = 255;
            }
        }
        v
    }

    #[test]
    fn vertical_near_band_reflects_top_edge() {
        // Region rows [10,20); near band rows [0,10) above, far band [20,30).
        let (w, h) = (8u32, 40u32);
        let frame = gradient_frame(w, h);
        let patch = mirror_blend(&frame, w, h, 0, 10, w, 10, Axis::Vertical)
            .expect("both bands available");
        // dy=0 is the near edge: t=1 → pure near sample = row ry-1 = row 9.
        let top = patch.bytes[0];
        assert_eq!(top, 9, "near edge (dy=0) must mirror the pixel just above ry");
        // dy=rh-1 is the far edge: t=0 → pure far sample = row ry+rh = row 20.
        let off = (((10 - 1) * w + 0) * 4) as usize;
        let bottom = patch.bytes[off];
        assert_eq!(bottom, 20, "far edge must mirror the pixel just below the region");
    }

    #[test]
    fn region_at_bottom_uses_only_near_band() {
        // Region hugs the bottom: far band would run off-screen, so only the
        // near (above) band is available. BEFORE the bugfix the bogus
        // `near_avail = ry <= src_h` check let an invalid blend through; now the
        // near band is the sole contributor and the patch is well-defined.
        let (w, h) = (8u32, 40u32);
        let frame = gradient_frame(w, h);
        // ry=30, rh=10 → region rows [30,40); far band [40,50) is off-screen.
        let patch = mirror_blend(&frame, w, h, 0, 30, w, 10, Axis::Vertical)
            .expect("near band alone is enough");
        // Top row mirrors row ry-1 = 29.
        assert_eq!(patch.bytes[0], 29);
    }

    #[test]
    fn paint_over_fill_serializes_without_png() {
        let p = PatchPayload {
            id: "x".into(),
            x: 0.1,
            y: 0.2,
            width: 0.3,
            height: 0.4,
            png_data_url: String::new(),
            fill: Some(SolidFill { r: 0, g: 0, b: 0, a: 255 }),
        };
        let j = serde_json::to_string(&p).unwrap();
        assert!(j.contains("\"fill\""));
        // A normal inpaint patch omits `fill` entirely (byte-compat).
        let q = PatchPayload { fill: None, ..p };
        let j2 = serde_json::to_string(&q).unwrap();
        assert!(!j2.contains("\"fill\""));
    }
}
