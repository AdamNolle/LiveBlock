//! Checked conversion of PipeWire memory chunks into packed, opaque BGRA.
use std::borrow::Cow;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SourceFourcc {
    Bgrx,
    Bgra,
    Rgbx,
    Rgba,
    Yuy2,
    Nv12,
}

pub(crate) struct Plane<'a> {
    bytes: Cow<'a, [u8]>,
    stride: usize,
}

impl<'a> Plane<'a> {
    pub(crate) fn new(
        memory: &'a [u8],
        offset: u32,
        size: u32,
        stride: i32,
        corrupted: bool,
    ) -> Option<Self> {
        if memory.is_empty() || size == 0 || stride < 0 || corrupted {
            return None;
        }
        // SPA defines offset modulo maxsize and size clamped to maxsize.
        let offset = offset as usize % memory.len();
        let size = (size as usize).min(memory.len());
        let end = offset.checked_add(size)?;
        let bytes = if end <= memory.len() {
            Cow::Borrowed(&memory[offset..end])
        } else {
            let mut wrapped = Vec::with_capacity(size);
            wrapped.extend_from_slice(&memory[offset..]);
            wrapped.extend_from_slice(&memory[..end - memory.len()]);
            Cow::Owned(wrapped)
        };
        Some(Self {
            bytes,
            stride: stride as usize,
        })
    }

    fn checked_stride(&self, row_bytes: usize, rows: usize) -> Option<usize> {
        let stride = if self.stride == 0 {
            row_bytes
        } else {
            self.stride
        };
        if stride < row_bytes || rows == 0 {
            return None;
        }
        let required = (rows - 1).checked_mul(stride)?.checked_add(row_bytes)?;
        (required <= self.bytes.len()).then_some(stride)
    }
}

pub(crate) fn decode(
    format: SourceFourcc,
    width: u32,
    height: u32,
    planes: &[Plane<'_>],
) -> Option<Vec<u8>> {
    let (w, h) = (width as usize, height as usize);
    if w == 0 || h == 0 {
        return None;
    }
    let size = w.checked_mul(h)?.checked_mul(4)?;
    // Bound malformed format announcements before allocating output memory.
    if size > 256 * 1024 * 1024 {
        return None;
    }
    let first = planes.first()?;
    match format {
        SourceFourcc::Bgrx | SourceFourcc::Bgra | SourceFourcc::Rgbx | SourceFourcc::Rgba => {
            let row = w.checked_mul(4)?;
            let stride = first.checked_stride(row, h)?;
            let swap = matches!(format, SourceFourcc::Rgbx | SourceFourcc::Rgba);
            let mut out = vec![0; size];
            for y in 0..h {
                for x in 0..w {
                    let s = y * stride + x * 4;
                    let d = (y * w + x) * 4;
                    out[d] = first.bytes[s + if swap { 2 } else { 0 }];
                    out[d + 1] = first.bytes[s + 1];
                    out[d + 2] = first.bytes[s + if swap { 0 } else { 2 }];
                    out[d + 3] = 255;
                }
            }
            Some(out)
        }
        SourceFourcc::Yuy2 => {
            let row = w.div_ceil(2).checked_mul(4)?;
            let stride = first.checked_stride(row, h)?;
            let mut out = vec![0; size];
            for y in 0..h {
                for x in 0..w {
                    let s = y * stride + (x / 2) * 4;
                    write_yuv(
                        &mut out,
                        (y * w + x) * 4,
                        first.bytes[s + (x % 2) * 2],
                        first.bytes[s + 1],
                        first.bytes[s + 3],
                    );
                }
            }
            Some(out)
        }
        SourceFourcc::Nv12 => {
            let y_stride = first.checked_stride(w, h)?;
            let uv_row = w.div_ceil(2).checked_mul(2)?;
            let uv_height = h.div_ceil(2);
            let (uv, uv_stride) = if let Some(second) = planes.get(1) {
                (
                    second.bytes.as_ref(),
                    second.checked_stride(uv_row, uv_height)?,
                )
            } else {
                let start = y_stride.checked_mul(h)?;
                let uv = first.bytes.get(start..)?;
                let stride = y_stride.max(uv_row);
                let required = (uv_height - 1).checked_mul(stride)?.checked_add(uv_row)?;
                if uv.len() < required {
                    return None;
                }
                (uv, stride)
            };
            let mut out = vec![0; size];
            for y in 0..h {
                for x in 0..w {
                    let c = (y / 2) * uv_stride + (x / 2) * 2;
                    write_yuv(
                        &mut out,
                        (y * w + x) * 4,
                        first.bytes[y * y_stride + x],
                        uv[c],
                        uv[c + 1],
                    );
                }
            }
            Some(out)
        }
    }
}

// Preserve the existing BT.601 full-range conversion. Packed RGB capture does
// not use this path; negotiated YUV colorimetry needs separate runtime checks.
fn write_yuv(dst: &mut [u8], off: usize, y: u8, u: u8, v: u8) {
    let (y, u, v) = (y as f32, u as f32 - 128.0, v as f32 - 128.0);
    dst[off] = (y + 1.772 * u).clamp(0.0, 255.0) as u8;
    dst[off + 1] = (y - 0.344136 * u - 0.714136 * v).clamp(0.0, 255.0) as u8;
    dst[off + 2] = (y + 1.402 * v).clamp(0.0, 255.0) as u8;
    dst[off + 3] = 255;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn offset_padding_and_rgba_channels_are_respected() {
        let source = [99, 99, 1, 2, 3, 0, 88, 88, 4, 5, 6, 0, 77, 77];
        let plane = Plane::new(&source, 2, 10, 6, false).unwrap();
        assert_eq!(
            decode(SourceFourcc::Rgba, 1, 2, &[plane]).unwrap(),
            [3, 2, 1, 255, 6, 5, 4, 255]
        );
    }

    #[test]
    fn wrapped_chunks_follow_spa_offset_and_size_rules() {
        let memory = [3, 0, 1, 2];
        let plane = Plane::new(&memory, 6, 100, 4, false).unwrap();
        assert_eq!(
            decode(SourceFourcc::Bgra, 1, 1, &[plane]).unwrap(),
            [1, 2, 3, 255]
        );
    }

    #[test]
    fn truncated_or_corrupt_chunks_do_not_fabricate_black_frames() {
        let memory = [0; 16];
        let short = Plane::new(&memory, 0, 4, 4, false).unwrap();
        assert!(decode(SourceFourcc::Bgra, 1, 2, &[short]).is_none());
        assert!(Plane::new(&memory, 0, 16, 4, true).is_none());
        assert!(Plane::new(&memory, 0, 0, 4, false).is_none());
        assert!(Plane::new(&memory, 0, 16, -4, false).is_none());
        let bad_stride = Plane::new(&memory, 0, 16, 3, false).unwrap();
        assert!(decode(SourceFourcc::Bgra, 1, 2, &[bad_stride]).is_none());
    }

    #[test]
    fn nv12_uses_independent_chroma_offset_stride_and_odd_edges() {
        let y = [91, 10, 20, 30, 90, 40, 50, 60];
        let uv = [92, 128, 128, 128, 128, 93];
        let planes = [
            Plane::new(&y, 1, 7, 4, false).unwrap(),
            Plane::new(&uv, 1, 4, 6, false).unwrap(),
        ];
        let output = decode(SourceFourcc::Nv12, 3, 2, &planes).unwrap();
        let values: Vec<_> = output.chunks_exact(4).map(|p| p[0]).collect();
        assert_eq!(values, [10, 20, 30, 40, 50, 60]);
        assert!(output
            .chunks_exact(4)
            .all(|p| p[0] == p[1] && p[1] == p[2] && p[3] == 255));
    }

    #[test]
    fn contiguous_nv12_and_odd_yuy2_decode_every_pixel() {
        let nv12 = [10, 20, 30, 40, 128, 128];
        assert_eq!(
            decode(
                SourceFourcc::Nv12,
                2,
                2,
                &[Plane::new(&nv12, 0, 6, 2, false).unwrap()]
            )
            .unwrap()[12],
            40
        );
        let yuy2 = [10, 128, 20, 128, 30, 128, 99, 128];
        let result = decode(
            SourceFourcc::Yuy2,
            3,
            1,
            &[Plane::new(&yuy2, 0, 8, 8, false).unwrap()],
        )
        .unwrap();
        assert_eq!([result[0], result[4], result[8]], [10, 20, 30]);
    }
}
