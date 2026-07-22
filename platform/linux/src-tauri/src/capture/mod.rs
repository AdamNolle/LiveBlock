//! Capture dispatcher. Picks Wayland (PipeWire portal) or X11 (XShm + XComposite)
//! at runtime based on `XDG_SESSION_TYPE`.

pub mod wayland;
pub mod x11;

use crate::session::{detect_session, SessionType};
use anyhow::{anyhow, Result};
use std::sync::Arc;

#[derive(Debug, Clone)]
pub struct FrameView {
    /// BGRA8 pixel data, row-major, no padding.
    pub pixels: Arc<[u8]>,
    pub width: u32,
    pub height: u32,
    pub stride: u32,
}

impl FrameView {
    pub fn is_valid_packed_bgra(&self) -> bool {
        let Some(expected_stride) = self.width.checked_mul(4) else {
            return false;
        };
        let Some(pixels) = (self.width as usize).checked_mul(self.height as usize) else {
            return false;
        };
        let Some(expected_len) = pixels.checked_mul(4) else {
            return false;
        };
        self.width > 0
            && self.height > 0
            && self.stride == expected_stride
            && self.pixels.len() == expected_len
    }
}

#[derive(Debug, Clone)]
pub enum CaptureEvent {
    Frame(FrameView),
    Reset,
}

#[async_trait::async_trait]
pub trait CaptureSource: Send {
    async fn next_event(&mut self) -> Result<CaptureEvent>;
    fn dropped_frames(&self) -> u64 {
        0
    }
    async fn stop(&mut self);
}

/// Open a capture source for the current display server.
///
/// On Wayland this triggers the xdg-desktop-portal screen-share permission
/// dialog the first time. The user can persist the choice via `persist_mode`.
pub async fn open_capture() -> Result<Box<dyn CaptureSource>> {
    match detect_session() {
        SessionType::Wayland => {
            let cap = wayland::WaylandCapture::new().await?;
            Ok(Box::new(cap))
        }
        SessionType::X11 => {
            let cap = x11::X11Capture::new()?;
            Ok(Box::new(cap))
        }
        SessionType::Unknown => Err(anyhow!(
            "Could not detect display server. Set XDG_SESSION_TYPE=wayland or x11."
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::FrameView;
    use std::sync::Arc;

    #[test]
    fn packed_bgra_validation_rejects_truncation_stride_and_overflow() {
        let valid = FrameView {
            pixels: Arc::from(vec![0u8; 4 * 3 * 2]),
            width: 3,
            height: 2,
            stride: 12,
        };
        assert!(valid.is_valid_packed_bgra());

        let mut truncated = valid.clone();
        truncated.pixels = Arc::from(vec![0u8; 23]);
        assert!(!truncated.is_valid_packed_bgra());

        let mut wrong_stride = valid;
        wrong_stride.stride = 16;
        assert!(!wrong_stride.is_valid_packed_bgra());

        let overflow = FrameView {
            pixels: Arc::from([]),
            width: u32::MAX,
            height: u32::MAX,
            stride: 0,
        };
        assert!(!overflow.is_valid_packed_bgra());
    }
}
