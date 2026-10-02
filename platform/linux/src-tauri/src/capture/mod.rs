//! Capture dispatcher. Picks Wayland (PipeWire portal) or X11 (XShm + XComposite)
//! at runtime based on `XDG_SESSION_TYPE`.

mod pixels;
pub mod wayland;
pub mod x11;

use crate::session::{detect_session, SessionType};
use anyhow::{anyhow, Result};
mod types;
mod x11_pixels;
pub use types::{CaptureSource, FrameView};

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
