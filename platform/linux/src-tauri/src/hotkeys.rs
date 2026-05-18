//! Global hotkeys.
//!
//! Wayland: xdg-desktop-portal `org.freedesktop.portal.GlobalShortcuts`
//! (requires the user grant access; `persist_mode` so the prompt is one-time).
//!
//! X11: classic `XGrabKey` on the root window.

use anyhow::{Context, Result};
use crossbeam_channel::Sender;

#[derive(Debug, Clone, Copy)]
pub enum HotkeyAction {
    ToggleCapture,        // Ctrl+Shift+L
    ToggleEditor,         // Ctrl+Shift+B
    CaptureForLabeling,   // Ctrl+Shift+S
    PanicDisable,         // Ctrl+Shift+Alt+Period
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
    // TODO(linux-port): use ashpd::desktop::global_shortcuts::GlobalShortcuts.
    // Steps:
    //   1. create_session (persist_mode = ExplicitlyRevoked)
    //   2. bind_shortcuts(["toggle_capture" → Ctrl+Shift+L, ...])
    //   3. listen on the .activated signal; map shortcut id → HotkeyAction
    //      and forward via tx.
    Ok(())
}

fn install_x11_grab(_tx: Sender<HotkeyAction>) -> Result<()> {
    // TODO(linux-port): open an x11rb connection, call grab_key on the root
    // window for each combo. Spawn a thread to pump XEvents and dispatch.
    Ok(())
}
