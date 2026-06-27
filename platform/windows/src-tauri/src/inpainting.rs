//! CPU mirror-blend inpaint helpers (port of `Sources/InpaintingEngine.swift`).
//!
//! The old base64 `data:image/png` "PatchPayload" path is DELETED: covers are
//! now drawn on the native layered overlay (`overlay.rs`) driven by the
//! pipeline worker (`pipeline.rs`), never through a webview/base64 channel.
//!
//! What remains is the pure, allocation-light mirror-blend that synthesizes a
//! content-aware BGRA patch for a region. It is kept as a reusable building
//! block for the GPU inpaint follow-up (the worker currently uses a cheaper
//! flat border-average cover); none of it touches the network or a webview.
//!
//! TODO(windows-port): D3D11 compute-shader path. WGC frames already arrive as
//! IDirect3DSurface; rendering the mirror reflection on the same device avoids a
//! CPU roundtrip (author Inpaint.hlsl with BlendVert/BlendHoriz/EdgeFill CSes,
//! compile to .cso, CreateComputeShader, dispatch, CopyResource). The CPU path
//! below is functionally complete; the GPU path is a perf optimization.

#![allow(dead_code)]

/// A content-aware BGRA patch (packed, row stride = width*4).
pub struct RawPatch {
    pub bytes: Vec<u8>,
    pub width: u32,
    pub height: u32,
}

/// Synthesize a content-aware fill for the pixel rect `(rx, ry, rw, rh)` of a
/// packed BGRA frame, using a mirror-blend on the better-suited axis and falling
/// back to a solid border-average. Top-left origin (Windows).
#[allow(clippy::too_many_arguments)]
pub fn render_region(
    bgra: &[u8],
    src_w: u32,
    src_h: u32,
    rx: u32,
    ry: u32,
    rw: u32,
    rh: u32,
) -> RawPatch {
    let aspect = rw as f32 / rh.max(1) as f32;
    let prefer_vertical = aspect >= 1.0;
    let order: [Axis; 2] = if prefer_vertical {
        [Axis::Vertical, Axis::Horizontal]
    } else {
        [Axis::Horizontal, Axis::Vertical]
    };
    for axis in order {
        if let Some(p) = mirror_blend(bgra, src_w, src_h, rx, ry, rw, rh, axis) {
            return p;
        }
    }
    let color = average_border_color(bgra, src_w, src_h, rx, ry, rw, rh);
    solid_fill(rw, rh, color)
}

#[derive(Copy, Clone)]
enum Axis {
    Vertical,
    Horizontal,
}

#[allow(clippy::too_many_arguments)]
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
                    let sy = near_y + (rh - 1 - dy);
                    (rx + dx, sy)
                }) as Box<dyn Fn(u32, u32) -> (u32, u32)>,
                Box::new(move |dx: u32, dy: u32| -> (u32, u32) {
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
