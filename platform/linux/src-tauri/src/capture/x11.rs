//! X11 root capture via XComposite capability detection and MIT-SHM images.
//! We do not claim root redirection ownership because the desktop compositor
//! may already own it; root GetImage reads the final composited desktop.

use super::{CaptureEvent, CaptureSource, FrameView};
use anyhow::{anyhow, Context, Result};
use memmap2::{MmapMut, MmapOptions};
use std::fs::File;
use std::sync::Arc;
use std::time::{Duration, Instant};
use x11rb::connection::Connection;
use x11rb::protocol::composite::ConnectionExt as _;
use x11rb::protocol::shm::ConnectionExt as _;
use x11rb::protocol::xproto::ConnectionExt as _;
use x11rb::protocol::xproto::{ImageFormat, ImageOrder, Screen};
use x11rb::rust_connection::RustConnection;

const ROOT_GEOMETRY_CHECK_INTERVAL: Duration = Duration::from_millis(500);

#[derive(Debug, Clone, Copy)]
struct ColorMasks {
    red: u32,
    green: u32,
    blue: u32,
}

pub struct X11Capture {
    conn: RustConnection,
    root: u32,
    width: u16,
    height: u16,
    depth: u8,
    stride: usize,
    bits_per_pixel: u8,
    scanline_pad: u8,
    lsb_first: bool,
    color_masks: ColorMasks,
    shm_seg: u32,
    mapping: MmapMut,
    shm_attached: bool,
    last_geometry_check: Instant,
}

fn geometry_check_due(last_check: Instant, now: Instant) -> bool {
    now.saturating_duration_since(last_check) >= ROOT_GEOMETRY_CHECK_INTERVAL
}

fn packed_stride(width: u16, bits_per_pixel: u8, scanline_pad: u8) -> Result<usize> {
    let pad_bits = usize::from(scanline_pad);
    if width == 0 || bits_per_pixel == 0 || pad_bits == 0 || pad_bits % 8 != 0 {
        return Err(anyhow!(
            "unsupported X11 packed layout width={width} bpp={bits_per_pixel} pad={scanline_pad}"
        ));
    }
    let row_bits = usize::from(width)
        .checked_mul(usize::from(bits_per_pixel))
        .ok_or_else(|| anyhow!("X11 row size overflow"))?;
    let padded_units = row_bits
        .checked_add(pad_bits - 1)
        .ok_or_else(|| anyhow!("X11 padded row size overflow"))?
        / pad_bits;
    padded_units
        .checked_mul(pad_bits / 8)
        .ok_or_else(|| anyhow!("X11 packed stride overflow"))
}

fn create_shm_mapping(conn: &RustConnection, stride: usize, height: u16) -> Result<(u32, MmapMut)> {
    if height == 0 {
        return Err(anyhow!("X11 root height is zero"));
    }
    let byte_len = stride
        .checked_mul(usize::from(height))
        .and_then(|value| u32::try_from(value).ok())
        .ok_or_else(|| anyhow!("X11 capture dimensions overflow shared-memory size"))?;
    let shm_seg = conn.generate_id().context("allocate XShm segment id")?;
    let reply = conn
        .shm_create_segment(shm_seg, byte_len, false)?
        .reply()
        .context("XShm create_segment (MIT-SHM 1.2 fd transport required)")?;
    let file = File::from(reply.shm_fd);
    match unsafe { MmapOptions::new().len(byte_len as usize).map_mut(&file) } {
        Ok(mapping) => Ok((shm_seg, mapping)),
        Err(error) => {
            if let Ok(cookie) = conn.shm_detach(shm_seg) {
                let _ = cookie.check();
            }
            let _ = conn.flush();
            Err(error).context("map XShm capture segment")
        }
    }
}

impl X11Capture {
    pub fn new() -> Result<Self> {
        let (conn, screen_num) = x11rb::connect(None).context("X11 connect")?;
        let (
            root,
            width,
            height,
            depth,
            stride,
            bits_per_pixel,
            scanline_pad,
            lsb_first,
            color_masks,
        ) = {
            let setup = conn.setup();
            let screen: &Screen = &setup.roots[screen_num];
            let format = setup
                .pixmap_formats
                .iter()
                .find(|format| format.depth == screen.root_depth)
                .ok_or_else(|| anyhow!("X11 root depth has no pixmap format"))?;
            let visual = screen
                .allowed_depths
                .iter()
                .find(|allowed| allowed.depth == screen.root_depth)
                .and_then(|allowed| {
                    allowed
                        .visuals
                        .iter()
                        .find(|visual| visual.visual_id == screen.root_visual)
                })
                .ok_or_else(|| anyhow!("X11 root visual metadata is unavailable"))?;
            let stride = packed_stride(
                screen.width_in_pixels,
                format.bits_per_pixel,
                format.scanline_pad,
            )?;
            (
                screen.root,
                screen.width_in_pixels,
                screen.height_in_pixels,
                screen.root_depth,
                stride,
                format.bits_per_pixel,
                format.scanline_pad,
                setup.image_byte_order == ImageOrder::LSB_FIRST,
                ColorMasks {
                    red: visual.red_mask,
                    green: visual.green_mask,
                    blue: visual.blue_mask,
                },
            )
        };
        if !matches!(bits_per_pixel, 24 | 32) {
            return Err(anyhow!(
                "unsupported X11 root pixel width {bits_per_pixel} at depth {depth}"
            ));
        }

        conn.composite_query_version(0, 4)?
            .reply()
            .context("XComposite query_version")?;

        let (shm_seg, mapping) = create_shm_mapping(&conn, stride, height)?;

        Ok(Self {
            conn,
            root,
            width,
            height,
            depth,
            stride,
            bits_per_pixel,
            scanline_pad,
            lsb_first,
            color_masks,
            shm_seg,
            mapping,
            shm_attached: true,
            last_geometry_check: Instant::now(),
        })
    }

    fn refresh_root_geometry(&mut self) -> Result<bool> {
        let geometry = self
            .conn
            .get_geometry(self.root)?
            .reply()
            .context("query current X11 root geometry")?;
        if geometry.depth != self.depth {
            return Err(anyhow!(
                "X11 root depth changed from {} to {}",
                self.depth,
                geometry.depth
            ));
        }
        if geometry.width == self.width && geometry.height == self.height {
            return Ok(false);
        }
        let stride = packed_stride(geometry.width, self.bits_per_pixel, self.scanline_pad)?;
        let (new_shm_seg, new_mapping) = create_shm_mapping(&self.conn, stride, geometry.height)?;
        let detach_result = (|| -> Result<()> {
            self.conn
                .shm_detach(self.shm_seg)?
                .check()
                .context("detach previous XShm capture segment")?;
            self.conn.flush().context("flush XShm resize")?;
            Ok(())
        })();
        if let Err(error) = detach_result {
            if let Ok(cookie) = self.conn.shm_detach(new_shm_seg) {
                let _ = cookie.check();
            }
            let _ = self.conn.flush();
            return Err(error);
        }
        self.shm_seg = new_shm_seg;
        self.mapping = new_mapping;
        self.width = geometry.width;
        self.height = geometry.height;
        self.stride = stride;
        Ok(true)
    }

    fn capture_frame(&mut self) -> Result<FrameView> {
        self.conn
            .shm_get_image(
                self.root,
                0,
                0,
                self.width,
                self.height,
                u32::MAX,
                ImageFormat::Z_PIXMAP.into(),
                self.shm_seg,
                0,
            )?
            .reply()
            .context("XShm get_image")?;
        let packed = convert_native_to_bgra(
            &self.mapping,
            self.width as u32,
            self.height as u32,
            self.stride,
            self.bits_per_pixel,
            self.lsb_first,
            self.color_masks,
        )?;
        Ok(FrameView {
            pixels: Arc::from(packed),
            width: self.width as u32,
            height: self.height as u32,
            stride: self.width as u32 * 4,
        })
    }

    fn cleanup(&mut self) {
        if self.shm_attached {
            let _ = self.conn.shm_detach(self.shm_seg);
            let _ = self.conn.flush();
            self.shm_attached = false;
        }
    }
}

impl Drop for X11Capture {
    fn drop(&mut self) {
        self.cleanup();
    }
}

#[async_trait::async_trait]
impl CaptureSource for X11Capture {
    async fn next_event(&mut self) -> Result<CaptureEvent> {
        let now = Instant::now();
        if geometry_check_due(self.last_geometry_check, now) {
            self.last_geometry_check = now;
            if self.refresh_root_geometry()? {
                return Ok(CaptureEvent::Reset);
            }
        }
        match self.capture_frame() {
            Ok(frame) => Ok(CaptureEvent::Frame(frame)),
            Err(frame_error) => match self.refresh_root_geometry() {
                Ok(true) => Ok(CaptureEvent::Reset),
                Ok(false) => Err(frame_error),
                Err(geometry_error) => Err(anyhow!(
                    "X11 frame failed ({frame_error}); geometry recovery failed ({geometry_error})"
                )),
            },
        }
    }
    async fn stop(&mut self) {
        self.cleanup();
    }
}

fn convert_native_to_bgra(
    source: &[u8],
    width: u32,
    height: u32,
    stride: usize,
    bits_per_pixel: u8,
    lsb_first: bool,
    masks: ColorMasks,
) -> Result<Vec<u8>> {
    let bytes_per_pixel = usize::from(bits_per_pixel / 8);
    let row_bytes = width as usize * bytes_per_pixel;
    let required = stride
        .checked_mul(height as usize)
        .ok_or_else(|| anyhow!("X11 frame dimensions overflow"))?;
    if stride < row_bytes || source.len() < required {
        return Err(anyhow!("truncated or invalid X11 shared-memory frame"));
    }
    let mut output = vec![0u8; width as usize * height as usize * 4];
    for y in 0..height as usize {
        let row = &source[y * stride..y * stride + row_bytes];
        for x in 0..width as usize {
            let pixel = &row[x * bytes_per_pixel..(x + 1) * bytes_per_pixel];
            let native = if lsb_first {
                pixel.iter().enumerate().fold(0u32, |value, (index, byte)| {
                    value | (u32::from(*byte) << (index * 8))
                })
            } else {
                pixel
                    .iter()
                    .fold(0u32, |value, byte| (value << 8) | u32::from(*byte))
            };
            let r = expand_mask(native, masks.red)?;
            let g = expand_mask(native, masks.green)?;
            let b = expand_mask(native, masks.blue)?;
            let offset = (y * width as usize + x) * 4;
            output[offset..offset + 4].copy_from_slice(&[b, g, r, 255]);
        }
    }
    Ok(output)
}

fn expand_mask(pixel: u32, mask: u32) -> Result<u8> {
    if mask == 0 {
        return Err(anyhow!("X11 visual has an empty RGB mask"));
    }
    let shift = mask.trailing_zeros();
    let maximum = mask >> shift;
    let value = (pixel & mask) >> shift;
    Ok(((u64::from(value) * 255 + u64::from(maximum) / 2) / u64::from(maximum)) as u8)
}

#[cfg(test)]
mod tests {
    use super::*;

    const RGB888: ColorMasks = ColorMasks {
        red: 0x00ff0000,
        green: 0x0000ff00,
        blue: 0x000000ff,
    };

    #[test]
    fn geometry_polling_and_stride_are_bounded() {
        let now = Instant::now();
        assert!(!geometry_check_due(now, now));
        assert!(geometry_check_due(now - ROOT_GEOMETRY_CHECK_INTERVAL, now,));
        assert_eq!(packed_stride(3, 24, 32).unwrap(), 12);
        assert_eq!(packed_stride(3, 32, 32).unwrap(), 12);
        assert!(packed_stride(0, 32, 32).is_err());
        assert!(packed_stride(3, 32, 7).is_err());
    }

    #[test]
    fn converts_padded_lsb_bgrx_to_packed_bgra() {
        let source = [1, 2, 3, 0, 4, 5, 6, 0, 99, 99, 99, 99];
        assert_eq!(
            convert_native_to_bgra(&source, 2, 1, 12, 32, true, RGB888).unwrap(),
            [1, 2, 3, 255, 4, 5, 6, 255]
        );
    }

    #[test]
    fn converts_msb_xrgb_to_packed_bgra() {
        let source = [0, 30, 20, 10];
        assert_eq!(
            convert_native_to_bgra(&source, 1, 1, 4, 32, false, RGB888).unwrap(),
            [10, 20, 30, 255]
        );
    }

    #[test]
    fn rejects_truncated_rows() {
        assert!(convert_native_to_bgra(&[0; 7], 2, 1, 8, 32, true, RGB888).is_err());
    }
}
