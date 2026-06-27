//! Pure-Rust CPU inpainting fills for LiveBlock.
//!
//! This crate ports the CPU-side pixel algorithms from the macOS
//! `Sources/InpaintingEngine.swift` (which used CoreImage) to platform-neutral
//! Rust that operates directly on a [`Frame`]'s BGRA/RGBA byte buffer. No GPU,
//! no CoreImage, no platform deps — it compiles and unit-tests anywhere
//! `cargo test` runs (Windows included).
//!
//! Three fills, in increasing quality / decreasing robustness:
//!
//! - [`fill_solid`] — an opaque single-color cover. This is the DRM-safe
//!   "PaintOver" fast path: it never reads the captured pixels, so there is
//!   nothing for content protection to black out. Mirrors
//!   `liveblock_core::Fill` / `paint_over_regions`.
//! - [`fill_edge_color`] — averages the ring of border pixels around the region
//!   and fills the whole patch with that mean color. This is the SAFE fallback
//!   (ported from Swift `averageBorderColor`): on a uniform page background it
//!   blends in; on busy content it is a neutral smudge but never erases identity.
//! - [`mirror_blend`] — reflects the band of frame adjacent to the region across
//!   the region edge, cross-fading two opposite reflections with a linear mask
//!   (ported from Swift `mirrorBlendPatch` / `reflectBand` / `blendMask`). Picks
//!   a blend axis from the region aspect ratio and falls back to `fill_edge_color`
//!   when no usable band is available (region against a screen edge).
//!
//! ## Coordinate space
//!
//! All region rects are [`NormRect`] in normalized `[0..1]`, origin **top-left**
//! — the core's canonical space (same as `paint_over_regions` /
//! `region_is_protected_black`). The Swift original worked in CoreVideo
//! bottom-left space; this port is top-left throughout, so "near band" for a
//! vertical blend is the band *above* the region (smaller y) and the patch is
//! produced top-to-bottom in raster order.
//!
//! ## Output format
//!
//! Every fill returns a tightly-packed **RGBA8** patch (`Vec<u8>`, length
//! `pw * ph * 4`, row-major, top-left origin), regardless of the source frame's
//! channel order. The overlay compositor positions it at the region's
//! [`PixelRect`]. RGBA (not the source order) is chosen so callers have one
//! predictable output layout for the native GPU surface.

use liveblock_core::{Fill, Frame, NormRect, PixelFormat};

/// A filled patch: tightly-packed RGBA8 bytes plus the integer pixel rect it
/// occupies in the frame (top-left origin).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Patch {
    /// Patch width in pixels.
    pub width: u32,
    /// Patch height in pixels.
    pub height: u32,
    /// Top-left x of the patch in the frame, pixels.
    pub x: u32,
    /// Top-left y of the patch in the frame, pixels.
    pub y: u32,
    /// `width * height * 4` RGBA8 bytes, row-major, top-left origin.
    pub rgba: Vec<u8>,
}

impl Patch {
    /// Read the RGBA tuple at patch-local `(px, py)`. Returns `None` out of bounds.
    pub fn pixel(&self, px: u32, py: u32) -> Option<[u8; 4]> {
        if px >= self.width || py >= self.height {
            return None;
        }
        let idx = ((py * self.width + px) * 4) as usize;
        Some([
            self.rgba[idx],
            self.rgba[idx + 1],
            self.rgba[idx + 2],
            self.rgba[idx + 3],
        ])
    }
}

/// Blend axis for [`mirror_blend`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MirrorAxis {
    /// Reflect the bands above/below the region across its horizontal edges.
    /// Best for wide regions (banner ads) on a uniform vertical background.
    Vertical,
    /// Reflect the bands left/right of the region across its vertical edges.
    /// Best for tall regions (sidebars).
    Horizontal,
}

/// An integer pixel rectangle, top-left origin, clamped inside a frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PixelRect {
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
}

impl PixelRect {
    fn x2(&self) -> u32 {
        self.x + self.width
    }
    fn y2(&self) -> u32 {
        self.y + self.height
    }
    fn is_empty(&self) -> bool {
        self.width == 0 || self.height == 0
    }
}

/// Convert a normalized top-left rect to an integer pixel rect, clamped to the
/// frame bounds. Returns `None` if the clamped rect is empty (off-screen or
/// degenerate).
pub fn norm_to_pixel_rect(rect: NormRect, frame_w: u32, frame_h: u32) -> Option<PixelRect> {
    if frame_w == 0 || frame_h == 0 {
        return None;
    }
    let fw = frame_w as f32;
    let fh = frame_h as f32;

    // Region edges in pixel space, then clamp to [0, frame].
    let x0 = (rect.x * fw).floor().clamp(0.0, fw);
    let y0 = (rect.y * fh).floor().clamp(0.0, fh);
    let x1 = ((rect.x + rect.width) * fw).ceil().clamp(0.0, fw);
    let y1 = ((rect.y + rect.height) * fh).ceil().clamp(0.0, fh);

    let x = x0 as u32;
    let y = y0 as u32;
    let width = (x1 - x0) as u32;
    let height = (y1 - y0) as u32;
    let r = PixelRect { x, y, width, height };
    if r.is_empty() {
        None
    } else {
        Some(r)
    }
}

// ===========================================================================
// Frame pixel access
// ===========================================================================

/// Bytes-per-pixel for the interleaved BGRA/RGBA formats this crate handles.
/// `None` for formats it can't sample directly (NV12).
fn bytes_per_pixel(format: PixelFormat) -> Option<usize> {
    match format {
        PixelFormat::Bgra8 | PixelFormat::Rgba8 => Some(4),
        PixelFormat::Nv12 => None,
    }
}

/// Read a frame pixel as RGBA, normalizing channel order from the source format.
/// Returns `None` for unsupported formats, out-of-bounds coords, or short buffers.
#[inline]
fn frame_rgba(frame: &Frame, x: u32, y: u32) -> Option<[u8; 4]> {
    let bpp = bytes_per_pixel(frame.format)?;
    let w = frame.width as usize;
    let h = frame.height as usize;
    if (x as usize) >= w || (y as usize) >= h {
        return None;
    }
    let stride = w * bpp;
    let idx = y as usize * stride + x as usize * bpp;
    if idx + 4 > frame.pixels.len() {
        return None;
    }
    let p = &frame.pixels;
    Some(match frame.format {
        // BGRA -> RGBA
        PixelFormat::Bgra8 => [p[idx + 2], p[idx + 1], p[idx], p[idx + 3]],
        // RGBA -> RGBA
        PixelFormat::Rgba8 => [p[idx], p[idx + 1], p[idx + 2], p[idx + 3]],
        PixelFormat::Nv12 => return None,
    })
}

// ===========================================================================
// 1. Solid fill — the DRM-safe PaintOver fast path.
// ===========================================================================

/// Fill `rect` with an opaque solid color. Does NOT read the frame: this is the
/// capture-free DRM path. `fill` is a [`liveblock_core::Fill`] so it stays in
/// lockstep with `paint_over_regions`.
///
/// Returns `None` only when the rect is fully off-screen / degenerate.
pub fn fill_solid(rect: NormRect, frame_w: u32, frame_h: u32, fill: Fill) -> Option<Patch> {
    let pr = norm_to_pixel_rect(rect, frame_w, frame_h)?;
    Some(solid_patch(pr, fill))
}

/// Build a solid-colored [`Patch`] for an already-resolved pixel rect.
fn solid_patch(pr: PixelRect, fill: Fill) -> Patch {
    let Fill::Solid { r, g, b, a } = fill;
    let n = (pr.width * pr.height) as usize;
    let mut rgba = Vec::with_capacity(n * 4);
    for _ in 0..n {
        rgba.push(r);
        rgba.push(g);
        rgba.push(b);
        rgba.push(a);
    }
    Patch {
        width: pr.width,
        height: pr.height,
        x: pr.x,
        y: pr.y,
        rgba,
    }
}

// ===========================================================================
// 2. Edge-color fill — the SAFE fallback (Swift `averageBorderColor`).
// ===========================================================================

/// Average the ring of border pixels around `rect` and fill the patch with that
/// mean color (opaque). Ported from Swift `averageBorderColor`: an inset-width
/// ring of four strips (top / bottom / left / right) is sampled and
/// area-weighted. Strips clipped off-screen contribute nothing.
///
/// When no border pixel is available at all (region fills the frame) the fill
/// degrades to opaque black, matching the Swift `CGColor(gray: 0)` fallback —
/// safe (never reveals erased content) but rarely hit in practice.
///
/// Returns `None` only when the rect is fully off-screen / degenerate.
pub fn fill_edge_color(frame: &Frame, rect: NormRect) -> Option<Patch> {
    let pr = norm_to_pixel_rect(rect, frame.width, frame.height)?;
    let color = average_border_color(frame, pr);
    Some(solid_patch(pr, color))
}

/// Compute the area-weighted mean of the border ring around `pr`. Pure compute;
/// exposed at crate-private scope and exercised by [`fill_edge_color`] tests.
fn average_border_color(frame: &Frame, pr: PixelRect) -> Fill {
    // inset = max(4, min(w,h) * 0.04), matching the Swift constant.
    let min_side = pr.width.min(pr.height) as f32;
    let inset = (min_side * 0.04).max(4.0).round() as u32;
    let inset = inset.max(1);

    let fw = frame.width;
    let fh = frame.height;

    // Four border strips in pixel space (top-left origin). Each is the slab of
    // frame just outside the corresponding region edge, `inset` px thick. Top
    // and bottom strips overhang the region width by `inset` on each side so the
    // corners are covered (matching the Swift strip geometry).
    let left = pr.x.saturating_sub(inset);
    let right = (pr.x2() + inset).min(fw);
    let top = pr.y.saturating_sub(inset);
    let bottom = (pr.y2() + inset).min(fh);

    let strips = [
        // Top strip (above the region).
        clamp_rect(left, top, right, pr.y, fw, fh),
        // Bottom strip (below the region).
        clamp_rect(left, pr.y2(), right, bottom, fw, fh),
        // Left strip (region height only, no corner overhang — matches Swift).
        clamp_rect(left, pr.y, pr.x, pr.y2(), fw, fh),
        // Right strip.
        clamp_rect(pr.x2(), pr.y, right, pr.y2(), fw, fh),
    ];

    let mut sum_r: f64 = 0.0;
    let mut sum_g: f64 = 0.0;
    let mut sum_b: f64 = 0.0;
    let mut weight: f64 = 0.0;

    for strip in strips.into_iter().flatten() {
        if let Some((r, g, b, n)) = strip_mean(frame, strip) {
            // Area-weight like the Swift version (sum over actual sampled px).
            sum_r += r * n;
            sum_g += g * n;
            sum_b += b * n;
            weight += n;
        }
    }

    if weight <= 0.0 {
        return Fill::opaque_black();
    }
    Fill::Solid {
        r: (sum_r / weight).round().clamp(0.0, 255.0) as u8,
        g: (sum_g / weight).round().clamp(0.0, 255.0) as u8,
        b: (sum_b / weight).round().clamp(0.0, 255.0) as u8,
        a: 255,
    }
}

/// Build a clamped pixel rect from inclusive-min / exclusive-max edges, returning
/// `None` if empty after clamping to the frame.
fn clamp_rect(x0: u32, y0: u32, x1: u32, y1: u32, fw: u32, fh: u32) -> Option<PixelRect> {
    let x0 = x0.min(fw);
    let y0 = y0.min(fh);
    let x1 = x1.min(fw);
    let y1 = y1.min(fh);
    if x1 <= x0 || y1 <= y0 {
        return None;
    }
    Some(PixelRect {
        x: x0,
        y: y0,
        width: x1 - x0,
        height: y1 - y0,
    })
}

/// Mean (r,g,b) over a strip plus the pixel count, all as f64. `None` if no
/// in-bounds pixel was readable.
fn strip_mean(frame: &Frame, strip: PixelRect) -> Option<(f64, f64, f64, f64)> {
    let mut sr: f64 = 0.0;
    let mut sg: f64 = 0.0;
    let mut sb: f64 = 0.0;
    let mut n: f64 = 0.0;
    for yy in strip.y..strip.y2() {
        for xx in strip.x..strip.x2() {
            if let Some([r, g, b, _]) = frame_rgba(frame, xx, yy) {
                sr += r as f64;
                sg += g as f64;
                sb += b as f64;
                n += 1.0;
            }
        }
    }
    if n <= 0.0 {
        None
    } else {
        Some((sr / n, sg / n, sb / n, n))
    }
}

// ===========================================================================
// 3. Mirror-blend — reflect adjacent bands + linear cross-fade.
//    Ported from Swift mirrorBlendPatch / reflectBand / blendMask.
// ===========================================================================

/// Reflect the frame band(s) adjacent to `rect` across the region edge and
/// cross-fade two opposite reflections with a linear gradient — a content
/// extrapolation that "continues" page chrome through the region.
///
/// `axis` is normally chosen by aspect ratio (see [`auto_inpaint`]); pass an
/// explicit axis to force one. If no band of sufficient coverage is available on
/// either side (region against a screen edge), this returns `None` so the caller
/// can fall back; [`auto_inpaint`] does that fallback automatically.
///
/// Algorithm (top-left space):
///  - Vertical axis: "near" band = the `height`-tall slab directly above the
///    region; "far" band = the slab directly below. Each is reflected across the
///    adjacent region edge (so the row touching the edge maps to itself) and the
///    two reflections are cross-faded — near dominant at the top of the patch,
///    far dominant at the bottom.
///  - Horizontal axis: near = band to the left, far = band to the right,
///    reflected across the left/right edges and cross-faded across the width.
///
/// A band counts as "available" only if at least 60% of it lies inside the frame
/// (matching the Swift `bandAvailable` threshold).
pub fn mirror_blend(frame: &Frame, rect: NormRect, axis: MirrorAxis) -> Option<Patch> {
    // Reject formats we can't sample (NV12) before doing any geometry work.
    bytes_per_pixel(frame.format)?;
    let pr = norm_to_pixel_rect(rect, frame.width, frame.height)?;
    mirror_blend_px(frame, pr, axis)
}

fn mirror_blend_px(frame: &Frame, pr: PixelRect, axis: MirrorAxis) -> Option<Patch> {
    let w = pr.width;
    let h = pr.height;
    if w == 0 || h == 0 {
        return None;
    }

    // Coverage fraction of a band of the same size as the region, offset by one
    // region-extent in the near/far direction. We don't materialize the band;
    // we just test how much of it is inside the frame.
    let fw = frame.width;
    let fh = frame.height;

    let (near_cov, far_cov) = match axis {
        MirrorAxis::Vertical => {
            // near = above (rows [y-h, y)), far = below (rows [y+h?])  in top-left
            let near = band_coverage_vertical(pr, fh, BandSide::Near);
            let far = band_coverage_vertical(pr, fh, BandSide::Far);
            (near, far)
        }
        MirrorAxis::Horizontal => {
            let near = band_coverage_horizontal(pr, fw, BandSide::Near);
            let far = band_coverage_horizontal(pr, fw, BandSide::Far);
            (near, far)
        }
    };

    let near_ok = near_cov > 0.6;
    let far_ok = far_cov > 0.6;
    if !near_ok && !far_ok {
        return None;
    }

    let n = (w * h) as usize;
    let mut rgba = vec![0u8; n * 4];

    for py in 0..h {
        for px in 0..w {
            // Sample the near and far reflections for this patch pixel.
            let near = if near_ok {
                sample_reflection(frame, pr, px, py, axis, BandSide::Near)
            } else {
                None
            };
            let far = if far_ok {
                sample_reflection(frame, pr, px, py, axis, BandSide::Far)
            } else {
                None
            };

            let color = match (near, far) {
                (Some(n), Some(f)) => {
                    // Linear cross-fade. blend_mask: 1.0 = fully near, 0.0 = fully far.
                    let t = blend_weight(px, py, w, h, axis);
                    lerp_rgba(n, f, t)
                }
                (Some(n), None) => n,
                (None, Some(f)) => f,
                // Reflection sample fell outside the frame even though the band
                // had >60% coverage (corner clipping). Leave the gap to be
                // filled by whichever side is available; if neither, use the
                // border average as a local safe value.
                (None, None) => border_pixel_fallback(frame, pr),
            };

            let idx = ((py * w + px) * 4) as usize;
            rgba[idx] = color[0];
            rgba[idx + 1] = color[1];
            rgba[idx + 2] = color[2];
            rgba[idx + 3] = color[3];
        }
    }

    Some(Patch {
        width: w,
        height: h,
        x: pr.x,
        y: pr.y,
        rgba,
    })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum BandSide {
    Near,
    Far,
}

/// Fraction of a region-sized band inside the frame, vertical axis.
/// Near = band directly above the region; Far = directly below.
fn band_coverage_vertical(pr: PixelRect, fh: u32, side: BandSide) -> f32 {
    let h = pr.height as i64;
    let (y0, y1) = match side {
        BandSide::Near => (pr.y as i64 - h, pr.y as i64),
        BandSide::Far => (pr.y2() as i64, pr.y2() as i64 + h),
    };
    let inside = (y1.min(fh as i64) - y0.max(0)).max(0);
    if h <= 0 {
        0.0
    } else {
        inside as f32 / h as f32
    }
}

/// Fraction of a region-sized band inside the frame, horizontal axis.
/// Near = band directly left of the region; Far = directly right.
fn band_coverage_horizontal(pr: PixelRect, fw: u32, side: BandSide) -> f32 {
    let w = pr.width as i64;
    let (x0, x1) = match side {
        BandSide::Near => (pr.x as i64 - w, pr.x as i64),
        BandSide::Far => (pr.x2() as i64, pr.x2() as i64 + w),
    };
    let inside = (x1.min(fw as i64) - x0.max(0)).max(0);
    if w <= 0 {
        0.0
    } else {
        inside as f32 / w as f32
    }
}

/// Sample the reflected band for patch-local pixel `(px, py)`.
///
/// "Reflect across the adjacent region edge" means: the patch row/col touching
/// the edge samples the frame row/col just outside the edge, and stepping deeper
/// into the patch steps further out into the band. This mirrors the band so the
/// content appears continuous across the seam.
fn sample_reflection(
    frame: &Frame,
    pr: PixelRect,
    px: u32,
    py: u32,
    axis: MirrorAxis,
    side: BandSide,
) -> Option<[u8; 4]> {
    let abs_x = pr.x + px;
    let abs_y = pr.y + py;
    match axis {
        MirrorAxis::Vertical => {
            // Reflection across a horizontal edge: x unchanged, y mirrored.
            let sy: i64 = match side {
                // Near edge = top of region (y = pr.y). The row just above it is
                // pr.y - 1; patch row 0 maps there, row 1 to pr.y - 2, etc.
                BandSide::Near => pr.y as i64 - 1 - py as i64,
                // Far edge = bottom of region (y = pr.y2()). The row just below
                // it is pr.y2(); patch's bottom row maps there.
                BandSide::Far => pr.y2() as i64 + (pr.height as i64 - 1 - py as i64),
            };
            if sy < 0 || sy >= frame.height as i64 {
                return None;
            }
            frame_rgba(frame, abs_x, sy as u32)
        }
        MirrorAxis::Horizontal => {
            let sx: i64 = match side {
                BandSide::Near => pr.x as i64 - 1 - px as i64,
                BandSide::Far => pr.x2() as i64 + (pr.width as i64 - 1 - px as i64),
            };
            if sx < 0 || sx >= frame.width as i64 {
                return None;
            }
            frame_rgba(frame, sx as u32, abs_y)
        }
    }
}

/// Linear cross-fade weight in [0,1] for patch pixel `(px,py)`. 1.0 = fully the
/// "near" reflection, 0.0 = fully "far". Matches the Swift `blendMask` gradient:
/// near edge is white (weight 1), far edge black (weight 0). With a single
/// interior pixel the weight is the midpoint (0.5).
fn blend_weight(px: u32, py: u32, w: u32, h: u32, axis: MirrorAxis) -> f32 {
    match axis {
        MirrorAxis::Vertical => {
            // Near = top (py = 0) -> 1.0; far = bottom (py = h-1) -> 0.0.
            if h <= 1 {
                0.5
            } else {
                1.0 - (py as f32 / (h - 1) as f32)
            }
        }
        MirrorAxis::Horizontal => {
            // Near = left (px = 0) -> 1.0; far = right (px = w-1) -> 0.0.
            if w <= 1 {
                0.5
            } else {
                1.0 - (px as f32 / (w - 1) as f32)
            }
        }
    }
}

/// Linear interpolate two RGBA colors. `t` in [0,1]: `t=1` -> `a`, `t=0` -> `b`.
fn lerp_rgba(a: [u8; 4], b: [u8; 4], t: f32) -> [u8; 4] {
    let t = t.clamp(0.0, 1.0);
    let mix = |ca: u8, cb: u8| -> u8 {
        (ca as f32 * t + cb as f32 * (1.0 - t)).round().clamp(0.0, 255.0) as u8
    };
    [
        mix(a[0], b[0]),
        mix(a[1], b[1]),
        mix(a[2], b[2]),
        mix(a[3], b[3]),
    ]
}

/// Local safe value when a reflection sample is unavailable mid-patch: the
/// border average (opaque). Cheap-ish but only hit for corner-clipped pixels.
fn border_pixel_fallback(frame: &Frame, pr: PixelRect) -> [u8; 4] {
    match average_border_color(frame, pr) {
        Fill::Solid { r, g, b, a } => [r, g, b, a],
    }
}

// ===========================================================================
// Orchestrator: aspect-aware axis pick + graceful fallback (Swift renderFill).
// ===========================================================================

/// Full content-extrapolation fill matching the Swift `renderFill`:
///  1. Pick a blend axis from the region aspect (wide -> vertical, tall ->
///     horizontal; square prefers vertical).
///  2. Try the preferred axis, then the other.
///  3. Fall back to [`fill_edge_color`] when neither axis has a usable band.
///
/// Always returns a patch for an on-screen region (the edge-color fallback can
/// always produce one), so the only `None` is a fully off-screen / degenerate
/// rect.
pub fn auto_inpaint(frame: &Frame, rect: NormRect) -> Option<Patch> {
    // Resolve once so the aspect test uses the clamped pixel rect.
    let pr = norm_to_pixel_rect(rect, frame.width, frame.height)?;
    let aspect = pr.width as f32 / pr.height.max(1) as f32;
    let prefer_vertical = aspect >= 1.0;
    let axes = if prefer_vertical {
        [MirrorAxis::Vertical, MirrorAxis::Horizontal]
    } else {
        [MirrorAxis::Horizontal, MirrorAxis::Vertical]
    };

    for axis in axes {
        if let Some(patch) = mirror_blend_px(frame, pr, axis) {
            return Some(patch);
        }
    }

    // Graceful fallback: edge-color fill.
    Some(solid_patch(pr, average_border_color(frame, pr)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    /// Build an RGBA8 frame from a per-pixel closure returning [r,g,b,a].
    fn make_frame_rgba<F: Fn(u32, u32) -> [u8; 4]>(w: u32, h: u32, f: F) -> Frame {
        let mut px = Vec::with_capacity((w * h * 4) as usize);
        for y in 0..h {
            for x in 0..w {
                px.extend_from_slice(&f(x, y));
            }
        }
        Frame {
            width: w,
            height: h,
            format: PixelFormat::Rgba8,
            index: 0,
            pixels: Arc::new(px),
        }
    }

    /// Build a BGRA8 frame from a per-pixel closure returning logical [r,g,b,a]
    /// (the bytes are stored B,G,R,A).
    fn make_frame_bgra<F: Fn(u32, u32) -> [u8; 4]>(w: u32, h: u32, f: F) -> Frame {
        let mut px = Vec::with_capacity((w * h * 4) as usize);
        for y in 0..h {
            for x in 0..w {
                let [r, g, b, a] = f(x, y);
                px.extend_from_slice(&[b, g, r, a]);
            }
        }
        Frame {
            width: w,
            height: h,
            format: PixelFormat::Bgra8,
            index: 0,
            pixels: Arc::new(px),
        }
    }

    fn nr(x: f32, y: f32, w: f32, h: f32) -> NormRect {
        NormRect { x, y, width: w, height: h }
    }

    // ---- norm_to_pixel_rect ------------------------------------------------

    #[test]
    fn norm_rect_maps_center_block() {
        // 100x200 frame, region covering x in [0.25,0.75), y in [0.5,0.75).
        let r = norm_to_pixel_rect(nr(0.25, 0.5, 0.5, 0.25), 100, 200).unwrap();
        assert_eq!(r, PixelRect { x: 25, y: 100, width: 50, height: 50 });
    }

    #[test]
    fn norm_rect_clamps_offscreen_to_none() {
        assert!(norm_to_pixel_rect(nr(2.0, 2.0, 0.5, 0.5), 100, 100).is_none());
        assert!(norm_to_pixel_rect(nr(0.0, 0.0, 0.0, 0.0), 100, 100).is_none());
        assert!(norm_to_pixel_rect(nr(0.0, 0.0, 0.5, 0.5), 0, 0).is_none());
    }

    #[test]
    fn norm_rect_clamps_partial_overhang() {
        // Region runs off the right/bottom; width/height are clamped to frame.
        let r = norm_to_pixel_rect(nr(0.5, 0.5, 1.0, 1.0), 100, 100).unwrap();
        assert_eq!(r, PixelRect { x: 50, y: 50, width: 50, height: 50 });
    }

    // ---- fill_solid --------------------------------------------------------

    #[test]
    fn solid_fill_is_uniform_opaque() {
        let frame = make_frame_rgba(40, 40, |_, _| [123, 45, 67, 255]);
        let fill = Fill::Solid { r: 10, g: 20, b: 30, a: 255 };
        let patch = fill_solid(nr(0.25, 0.25, 0.5, 0.5), frame.width, frame.height, fill).unwrap();
        assert_eq!(patch.width, 20);
        assert_eq!(patch.height, 20);
        assert_eq!(patch.x, 10);
        assert_eq!(patch.y, 10);
        // Every pixel exactly the fill color.
        for py in 0..patch.height {
            for px in 0..patch.width {
                assert_eq!(patch.pixel(px, py).unwrap(), [10, 20, 30, 255]);
            }
        }
    }

    #[test]
    fn solid_fill_opaque_black_helper() {
        let patch = fill_solid(nr(0.0, 0.0, 0.5, 0.5), 10, 10, Fill::opaque_black()).unwrap();
        assert!(patch.rgba.chunks(4).all(|c| c == [0, 0, 0, 255]));
    }

    // ---- fill_edge_color ---------------------------------------------------

    #[test]
    fn edge_color_uniform_background() {
        // Uniform gray frame; border average must equal the background exactly.
        let frame = make_frame_rgba(60, 60, |_, _| [80, 90, 100, 255]);
        let patch = fill_edge_color(&frame, nr(0.25, 0.25, 0.5, 0.5)).unwrap();
        for py in 0..patch.height {
            for px in 0..patch.width {
                assert_eq!(patch.pixel(px, py).unwrap(), [80, 90, 100, 255]);
            }
        }
    }

    #[test]
    fn edge_color_normalizes_bgra_channel_order() {
        // BGRA frame storing logical red. Edge average must come back as red in
        // RGBA output (proves channel normalization, not byte passthrough).
        let frame = make_frame_bgra(60, 60, |_, _| [200, 0, 0, 255]);
        let patch = fill_edge_color(&frame, nr(0.25, 0.25, 0.5, 0.5)).unwrap();
        assert_eq!(patch.pixel(0, 0).unwrap(), [200, 0, 0, 255]);
    }

    #[test]
    fn edge_color_two_tone_average() {
        // Left half value 100, right half value 200. The border ring straddles
        // both, so the mean lands strictly between the two tones.
        let frame = make_frame_rgba(80, 80, |x, _| {
            let v = if x < 40 { 100 } else { 200 };
            [v, v, v, 255]
        });
        let patch = fill_edge_color(&frame, nr(0.25, 0.25, 0.5, 0.5)).unwrap();
        let [r, _, _, a] = patch.pixel(0, 0).unwrap();
        assert_eq!(a, 255);
        assert!(r > 100 && r < 200, "expected blended gray, got {r}");
    }

    #[test]
    fn edge_color_offscreen_is_none() {
        let frame = make_frame_rgba(20, 20, |_, _| [0, 0, 0, 255]);
        assert!(fill_edge_color(&frame, nr(5.0, 5.0, 0.1, 0.1)).is_none());
    }

    // ---- mirror_blend ------------------------------------------------------

    #[test]
    fn mirror_blend_vertical_continues_horizontal_stripes() {
        // A frame whose color depends only on a vertical gradient of y. A region
        // in the middle, reflected vertically, should reproduce values close to
        // the surrounding gradient (the reflection of a smooth field).
        let frame = make_frame_rgba(40, 120, |_, y| {
            let v = (y * 2).min(255) as u8;
            [v, v, v, 255]
        });
        // Wide-ish region in the vertical middle.
        let rect = nr(0.1, 0.4, 0.8, 0.2); // y in [48,72), x in [4,36)
        let patch = mirror_blend(&frame, rect, MirrorAxis::Vertical).unwrap();
        assert_eq!(patch.height, 24);
        // The reflected/blended values must stay within the band's value range,
        // never collapse to a single flat color (that would prove no sampling).
        let mut min = 255u8;
        let mut max = 0u8;
        for py in 0..patch.height {
            let [r, _, _, a] = patch.pixel(0, py).unwrap();
            assert_eq!(a, 255);
            min = min.min(r);
            max = max.max(r);
        }
        assert!(max > min, "blend produced a flat patch (min={min} max={max})");
    }

    #[test]
    fn mirror_blend_reflects_edge_row_from_just_outside() {
        // Row y has value = y. Frame height 128 and y in [0.25,0.5) map to exact
        // integer pixel edges (32..64) free of f32 rounding ambiguity, so the
        // reflected rows are deterministic.
        let frame = make_frame_rgba(8, 128, |_, y| {
            let v = (y % 256) as u8;
            [v, v, v, 255]
        });
        let rect = nr(0.0, 0.25, 1.0, 0.25); // y in [32, 64)
        let pr = norm_to_pixel_rect(rect, 8, 128).unwrap();
        assert_eq!(pr, PixelRect { x: 0, y: 32, width: 8, height: 32 });

        let patch = mirror_blend(&frame, rect, MirrorAxis::Vertical).unwrap();
        // Top patch row (py=0): weight 1.0 -> pure NEAR reflection = the frame
        // row just above the top edge (pr.y - 1 = 31).
        assert_eq!(patch.pixel(0, 0).unwrap(), [31, 31, 31, 255]);
        // Bottom patch row (py=h-1): weight 0.0 -> pure FAR reflection = the
        // frame row just below the bottom edge (pr.y2() = 64).
        let last = patch.height - 1;
        assert_eq!(patch.pixel(0, last).unwrap(), [64, 64, 64, 255]);
    }

    #[test]
    fn mirror_blend_horizontal_reflects_columns() {
        // Column x has value x; exact pixel edges via x in [0.25,0.5) of width 128.
        let frame = make_frame_rgba(128, 8, |x, _| {
            let v = (x % 256) as u8;
            [v, v, v, 255]
        });
        let rect = nr(0.25, 0.0, 0.25, 1.0); // x in [32, 64)
        let pr = norm_to_pixel_rect(rect, 128, 8).unwrap();
        assert_eq!(pr, PixelRect { x: 32, y: 0, width: 32, height: 8 });

        let patch = mirror_blend(&frame, rect, MirrorAxis::Horizontal).unwrap();
        // Left column (px=0): weight 1.0 -> pure NEAR = col just left of edge (31).
        assert_eq!(patch.pixel(0, 0).unwrap(), [31, 31, 31, 255]);
        // Right column (px=w-1): weight 0.0 -> pure FAR = col just right (64).
        let last = patch.width - 1;
        assert_eq!(patch.pixel(last, 0).unwrap(), [64, 64, 64, 255]);
    }

    #[test]
    fn mirror_blend_none_when_no_band_either_side() {
        // Region spans the full height: no vertical band above or below -> None.
        let frame = make_frame_rgba(40, 40, |_, _| [50, 50, 50, 255]);
        let rect = nr(0.25, 0.0, 0.5, 1.0); // full-height
        assert!(mirror_blend(&frame, rect, MirrorAxis::Vertical).is_none());
    }

    #[test]
    fn mirror_blend_single_side_when_against_top_edge() {
        // Region flush to the top: no near (above) band, but a far (below) band
        // exists -> single-side reflection, still Some.
        let frame = make_frame_rgba(32, 128, |_, y| {
            let v = ((y + 10) % 256) as u8;
            [v, v, v, 255]
        });
        let rect = nr(0.25, 0.0, 0.5, 0.25); // y in [0, 32): no band above
        let pr = norm_to_pixel_rect(rect, 32, 128).unwrap();
        assert_eq!(pr.height, 32);
        let patch = mirror_blend(&frame, rect, MirrorAxis::Vertical).unwrap();
        assert_eq!(patch.height, 32);
        // Pure FAR reflection (no near band). The bottom patch row (py=h-1)
        // reflects the frame row just below the region (y=32, value 32+10=42).
        let last = patch.height - 1;
        assert_eq!(patch.pixel(0, last).unwrap(), [42, 42, 42, 255]);
        // Every pixel opaque.
        assert!(patch.rgba.chunks(4).all(|c| c[3] == 255));
    }

    #[test]
    fn mirror_blend_rejects_nv12() {
        let frame = Frame {
            width: 40,
            height: 40,
            format: PixelFormat::Nv12,
            index: 0,
            pixels: Arc::new(vec![0u8; 40 * 40 * 2]),
        };
        assert!(mirror_blend(&frame, nr(0.25, 0.25, 0.5, 0.5), MirrorAxis::Vertical).is_none());
    }

    // ---- auto_inpaint ------------------------------------------------------

    #[test]
    fn auto_inpaint_wide_region_uses_vertical_and_blends() {
        // Smooth vertical gradient, wide region -> vertical mirror succeeds.
        let frame = make_frame_rgba(120, 120, |_, y| {
            let v = (y * 2).min(255) as u8;
            [v, v, v, 255]
        });
        let rect = nr(0.1, 0.45, 0.8, 0.1); // wide, middle
        let patch = auto_inpaint(&frame, rect).unwrap();
        assert!(patch.width > patch.height);
        assert!(patch.rgba.chunks(4).all(|c| c[3] == 255));
    }

    #[test]
    fn auto_inpaint_falls_back_to_edge_color_when_no_band() {
        // Region fills the whole frame in both axes -> no band anywhere ->
        // edge-color fallback. With the region == frame there is no border ring,
        // so it degrades to opaque black (the documented last resort).
        let frame = make_frame_rgba(30, 30, |_, _| [70, 80, 90, 255]);
        let rect = nr(0.0, 0.0, 1.0, 1.0);
        let patch = auto_inpaint(&frame, rect).unwrap();
        assert_eq!(patch.width, 30);
        assert_eq!(patch.height, 30);
        assert!(patch.rgba.chunks(4).all(|c| c == [0, 0, 0, 255]));
    }

    #[test]
    fn auto_inpaint_offscreen_is_none() {
        let frame = make_frame_rgba(30, 30, |_, _| [0, 0, 0, 255]);
        assert!(auto_inpaint(&frame, nr(3.0, 3.0, 0.2, 0.2)).is_none());
    }

    #[test]
    fn patch_pixel_out_of_bounds_is_none() {
        let frame = make_frame_rgba(10, 10, |_, _| [1, 2, 3, 255]);
        let patch = fill_solid(nr(0.0, 0.0, 0.5, 0.5), 10, 10, Fill::opaque_black()).unwrap();
        assert!(patch.pixel(patch.width, 0).is_none());
        assert!(patch.pixel(0, patch.height).is_none());
        let _ = frame;
    }
}
