//! Global hotkeys.
//!
//! Wayland: xdg-desktop-portal `org.freedesktop.portal.GlobalShortcuts`
//! (requires the user grant access; `persist_mode` so the prompt is one-time).
//!
//! X11: classic `XGrabKey` on the root window.

use anyhow::{anyhow, Result};
use crossbeam_channel::Sender;

#[derive(Debug, Clone, Copy)]
pub enum HotkeyAction {
    ToggleCapture,      // Ctrl+Shift+L
    ToggleEditor,       // Ctrl+Shift+B
    CaptureForLabeling, // Ctrl+Shift+S
    PanicDisable,       // Ctrl+Shift+Alt+Period
}

pub fn install(tx: Sender<HotkeyAction>) -> Result<()> {
    match crate::session::detect_session() {
        crate::session::SessionType::Wayland => install_wayland_portal(tx),
        crate::session::SessionType::X11 | crate::session::SessionType::Unknown => {
            install_x11_grab(tx)
        }
    }
}

fn install_wayland_portal(_tx: Sender<HotkeyAction>) -> Result<()> {
    Err(anyhow!(
        "Wayland GlobalShortcuts portal integration is not implemented"
    ))
}

fn install_x11_grab(_tx: Sender<HotkeyAction>) -> Result<()> {
    Err(anyhow!("X11 global hotkey integration is not implemented"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unfinished_installers_fail_instead_of_claiming_registration() {
        let (sender, _) = crossbeam_channel::bounded(1);
        assert!(install_wayland_portal(sender.clone()).is_err());
        assert!(install_x11_grab(sender).is_err());
    }
}
