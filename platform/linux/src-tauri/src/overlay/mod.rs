//! Overlay-window dispatcher. Picks the right strategy at runtime:
//!
//!   - Wayland + wlr-layer-shell capable compositor (Sway, Hyprland, KDE)
//!     → full-screen click-through layer-shell window.
//!   - Wayland + GNOME (no layer-shell)
//!     → "GNOME mode": visible Tauri window, always-on-top, movable.
//!   - X11 → override-redirect window with an empty input shape (xfixes).

pub mod wayland_layer_shell;
pub mod wayland_gnome;
pub mod x11;

use crate::session::{detect_compositor, detect_session, supports_layer_shell, SessionType};
use anyhow::Result;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OverlayStrategy {
    WaylandLayerShell,
    WaylandGnomeMode,
    X11OverrideRedirect,
}

pub fn pick_strategy() -> OverlayStrategy {
    match detect_session() {
        SessionType::Wayland => {
            if supports_layer_shell(detect_compositor()) {
                OverlayStrategy::WaylandLayerShell
            } else {
                OverlayStrategy::WaylandGnomeMode
            }
        }
        SessionType::X11 | SessionType::Unknown => OverlayStrategy::X11OverrideRedirect,
    }
}

pub fn install_click_through(strategy: OverlayStrategy) -> Result<()> {
    match strategy {
        OverlayStrategy::WaylandLayerShell => wayland_layer_shell::install(),
        OverlayStrategy::WaylandGnomeMode => wayland_gnome::install(),
        OverlayStrategy::X11OverrideRedirect => x11::install(),
    }
}
