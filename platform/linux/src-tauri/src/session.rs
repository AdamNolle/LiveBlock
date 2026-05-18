//! Display server detection. Runtime selection between Wayland and X11 paths.

use std::env;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionType {
    Wayland,
    X11,
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WaylandCompositor {
    /// Sway, Hyprland, river, COSMIC, niri, etc — supports wlr-layer-shell.
    Wlroots,
    /// KDE Plasma — supports wlr-layer-shell since 5.27.
    Kwin,
    /// GNOME / Mutter — does NOT support arbitrary layer-shell windows.
    /// Falls back to "GNOME mode" (visible movable window, not click-through).
    Gnome,
    /// Anything else — try layer-shell, fall back to GNOME mode if it fails.
    Unknown,
}

pub fn detect_session() -> SessionType {
    match env::var("XDG_SESSION_TYPE").ok().as_deref() {
        Some("wayland") => SessionType::Wayland,
        Some("x11") => SessionType::X11,
        // Sometimes WAYLAND_DISPLAY is set without XDG_SESSION_TYPE.
        _ => {
            if env::var_os("WAYLAND_DISPLAY").is_some() {
                SessionType::Wayland
            } else if env::var_os("DISPLAY").is_some() {
                SessionType::X11
            } else {
                SessionType::Unknown
            }
        }
    }
}

pub fn detect_compositor() -> WaylandCompositor {
    let desktop = env::var("XDG_CURRENT_DESKTOP")
        .unwrap_or_default()
        .to_lowercase();
    let session = env::var("XDG_SESSION_DESKTOP")
        .unwrap_or_default()
        .to_lowercase();

    let combined = format!("{desktop} {session}");

    if combined.contains("gnome") {
        WaylandCompositor::Gnome
    } else if combined.contains("kde") || combined.contains("plasma") {
        WaylandCompositor::Kwin
    } else if combined.contains("sway")
        || combined.contains("hyprland")
        || combined.contains("river")
        || combined.contains("cosmic")
        || combined.contains("niri")
        || combined.contains("wlroots")
    {
        WaylandCompositor::Wlroots
    } else {
        WaylandCompositor::Unknown
    }
}

pub fn supports_layer_shell(c: WaylandCompositor) -> bool {
    matches!(c, WaylandCompositor::Wlroots | WaylandCompositor::Kwin)
}
