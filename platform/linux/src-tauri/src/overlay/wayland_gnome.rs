//! GNOME degraded mode ("FloatingDegraded").
//!
//! GNOME/Mutter deliberately does NOT implement `wlr-layer-shell` (the wlroots
//! protocol every other Wayland compositor uses for overlays), and there is no
//! sanctioned Wayland API for an unprivileged app to draw a full-screen,
//! always-on-top, click-through surface over arbitrary windows. So on GNOME we
//! cannot deliver the true overlay experience.
//!
//! The documented fallback is a normal Tauri window with `alwaysOnTop = true`
//! and a small movable footprint, into which the inpaint patches are rendered.
//! It covers the marked region while it overlaps it, but it is a real focusable
//! window — the user can move it, it does not pass clicks through, and it cannot
//! float above a fullscreen app. We surface this clearly via a banner so the
//! user understands the limitation rather than thinking the app is broken.
//!
//! (If/when GNOME ships a portal for this — or the user runs the wlroots-based
//! `gnome-shell` fork or switches to X11 — `pick_strategy()` automatically
//! selects the real layer-shell / X11 path instead.)

use anyhow::Result;
use tauri::{AppHandle, Emitter};

/// Install-time hook. There's no compositor surface to configure (Tauri creates
/// the always-on-top window from `tauri.conf.json`); we only record that we're
/// in degraded mode. The banner is emitted via [`announce`] once the app handle
/// is available.
pub fn install() -> Result<()> {
    tracing::warn!(
        "GNOME/Mutter detected: wlr-layer-shell unsupported — running in \
         FloatingDegraded overlay mode (movable always-on-top window, not \
         click-through). See overlay::wayland_gnome for details."
    );
    Ok(())
}

/// Emit the explanatory banner to the frontend so it can show the degraded-mode
/// notice. Called from the Tauri setup hook after the app handle exists.
pub fn announce(app: &AppHandle) {
    let _ = app.emit(
        "overlay-mode",
        serde_json::json!({
            "mode": "gnome-floating-degraded",
            "clickThrough": false,
            "reason": "GNOME/Mutter does not support wlr-layer-shell; using a \
                       movable always-on-top window instead of a true overlay.",
        }),
    );
}

pub fn is_gnome_mode_active() -> bool {
    matches!(
        crate::overlay::pick_strategy(),
        crate::overlay::OverlayStrategy::WaylandGnomeMode
    )
}
