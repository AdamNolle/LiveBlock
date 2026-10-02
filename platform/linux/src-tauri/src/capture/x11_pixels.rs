//! Convert the X server's negotiated visual into checked, opaque BGRA8.
#[derive(Clone, Copy)]
pub struct X11PixelFormat {
    pub bits_per_pixel: u8,
    pub scanline_pad: u8,
    pub little_endian: bool,
    pub masks: [u32; 3],
}

impl X11PixelFormat {
    pub fn stride(self, width: u32) -> Result<usize, &'static str> {
        if ![8, 16, 24, 32].contains(&self.bits_per_pixel)
            || ![8, 16, 32, 64].contains(&self.scanline_pad)
        {
            return Err("Unsupported X11 pixel format");
        }
        let mut occupied = 0;
        for mask in self.masks {
            if mask == 0
                || occupied & mask != 0
                || (self.bits_per_pixel < 32 && mask >> self.bits_per_pixel != 0)
            {
                return Err("Invalid X11 visual color masks");
            }
            let shifted = mask >> mask.trailing_zeros();
            if shifted & shifted.wrapping_add(1) != 0 {
                return Err("Noncontiguous X11 visual color mask");
            }
            occupied |= mask;
        }
        let bits = (width as usize)
            .checked_mul(self.bits_per_pixel as usize)
            .ok_or("X11 row too large")?;
        let pad = self.scanline_pad as usize;
        Ok(bits.checked_add(pad - 1).ok_or("X11 row too large")? / pad * (pad / 8))
    }

    pub fn buffer_len(self, width: u32, height: u32) -> Result<usize, &'static str> {
        let pixels = (width as usize)
            .checked_mul(height as usize)
            .ok_or("X11 frame too large")?;
        if pixels == 0 || pixels > 256 * 1024 * 1024 / 4 {
            return Err("X11 frame dimensions unsupported");
        }
        self.stride(width)?
            .checked_mul(height as usize)
            .ok_or("X11 frame too large")
    }

    pub fn decode(self, src: &[u8], width: u32, height: u32) -> Result<Vec<u8>, &'static str> {
        let required = self.buffer_len(width, height)?;
        if src.len() < required {
            return Err("Truncated X11 pixel buffer");
        }
        let stride = self.stride(width)?;
        let bytes = self.bits_per_pixel as usize / 8;
        let mut output = vec![0; width as usize * height as usize * 4];
        if self.little_endian
            && self.masks == [0xff0000, 0xff00, 0xff]
            && [24, 32].contains(&self.bits_per_pixel)
        {
            for y in 0..height as usize {
                let source = &src[y * stride..y * stride + width as usize * bytes];
                let destination = &mut output[y * width as usize * 4..(y + 1) * width as usize * 4];
                for (pixel, bgra) in source
                    .chunks_exact(bytes)
                    .zip(destination.chunks_exact_mut(4))
                {
                    bgra[..3].copy_from_slice(&pixel[..3]);
                    bgra[3] = 255;
                }
            }
            return Ok(output);
        }
        for y in 0..height as usize {
            for x in 0..width as usize {
                let offset = y * stride + x * bytes;
                let source = &src[offset..offset + bytes];
                let pixel = if self.little_endian {
                    source.iter().rev().fold(0u32, |v, b| (v << 8) | *b as u32)
                } else {
                    source.iter().fold(0u32, |v, b| (v << 8) | *b as u32)
                };
                let channel = |mask: u32| {
                    let shift = mask.trailing_zeros();
                    let maximum = (mask >> shift) as u64;
                    ((((pixel & mask) >> shift) as u64 * 255 + maximum / 2) / maximum) as u8
                };
                let destination = (y * width as usize + x) * 4;
                output[destination..destination + 4].copy_from_slice(&[
                    channel(self.masks[2]),
                    channel(self.masks[1]),
                    channel(self.masks[0]),
                    255,
                ]);
            }
        }
        Ok(output)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn rgb(bits: u8, little: bool) -> X11PixelFormat {
        X11PixelFormat {
            bits_per_pixel: bits,
            scanline_pad: 32,
            little_endian: little,
            masks: [0xff0000, 0xff00, 0xff],
        }
    }
    #[test]
    fn padded_24_bit_rows_and_32_bit_endianness() {
        assert_eq!(
            rgb(24, true)
                .decode(&[3, 2, 1, 0, 6, 5, 4, 0], 1, 2)
                .unwrap(),
            [3, 2, 1, 255, 6, 5, 4, 255]
        );
        assert_eq!(
            rgb(32, true).decode(&[3, 2, 1, 0], 1, 1).unwrap(),
            [3, 2, 1, 255]
        );
        assert_eq!(
            rgb(32, false).decode(&[0, 1, 2, 3], 1, 1).unwrap(),
            [3, 2, 1, 255]
        );
    }
    #[test]
    fn rgb565_and_padding() {
        let f = X11PixelFormat {
            masks: [0xf800, 0x7e0, 0x1f],
            ..rgb(16, true)
        };
        assert_eq!(f.decode(&[0, 0xf8, 0, 0], 1, 1).unwrap(), [0, 0, 255, 255]);
        assert_eq!(
            X11PixelFormat {
                little_endian: false,
                ..f
            }
            .decode(&[0xf8, 0, 0, 0], 1, 1)
            .unwrap(),
            [0, 0, 255, 255]
        );
        assert_eq!(f.decode(&[0xe0, 7, 0, 0], 1, 1).unwrap(), [0, 255, 0, 255]);
    }
    #[test]
    fn ten_bit_channels_scale_to_eight() {
        let f = X11PixelFormat {
            masks: [0x3ff00000, 0xffc00, 0x3ff],
            ..rgb(32, true)
        };
        assert_eq!(
            f.decode(&0x20000000u32.to_le_bytes(), 1, 1).unwrap(),
            [0, 0, 128, 255]
        );
    }
    #[test]
    fn malformed_and_short_buffers_fail() {
        assert!(rgb(24, true).decode(&[0, 0, 0], 1, 1).is_err());
        assert!(X11PixelFormat {
            masks: [0, 0xff00, 0xff],
            ..rgb(32, true)
        }
        .decode(&[0; 4], 1, 1)
        .is_err());
        assert!(X11PixelFormat {
            masks: [0xff, 0xff, 0xff],
            ..rgb(32, true)
        }
        .decode(&[0; 4], 1, 1)
        .is_err());
        assert!(rgb(32, true).buffer_len(u32::MAX, u32::MAX).is_err());
    }
}
