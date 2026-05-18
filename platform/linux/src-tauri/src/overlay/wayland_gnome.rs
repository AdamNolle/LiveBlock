//! GNOME degraded mode. GNOME/Mutter does not implement wlr-layer-shell, so
//! we cannot create a true full-screen click-through overlay. Instead we
//! present a normal Tauri window with always-on-top + a small floating
//! footprint, and inpainting is rendered into that window only.
//!
//! The user is shown a banner explaining the degradation.

use anyhow::Result;

pub fn install() -> Result<()> {
    // Nothing to do at install time — Tauri creates the window with
    // alwaysOnTop=true via tauri.conf.json. We just emit a "gnome-mode"
    // event so the frontend can show the explanatory banner.
    Ok(())
}

pub fn is_gnome_mode_active() -> bool {
    matches!(
        crate::overlay::pick_strategy(),
        crate::overlay::OverlayStrategy::WaylandGnomeMode
    )
}
