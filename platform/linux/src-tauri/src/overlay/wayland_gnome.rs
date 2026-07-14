//! GNOME/Mutter limited mode. Mutter intentionally lacks layer-shell, so the
//! renderer is a bounded decorated preview window rather than a global overlay.

use anyhow::{Context, Result};
use tauri::{LogicalSize, Manager, Size};

pub fn install(app: &tauri::AppHandle) -> Result<()> {
    let window = app
        .get_webview_window("render")
        .context("render window is unavailable")?;
    window
        .set_decorations(true)
        .context("enable GNOME preview decorations")?;
    window
        .set_size(Size::Logical(LogicalSize::new(720.0, 405.0)))
        .context("size GNOME preview")?;
    window.center().context("center GNOME preview")?;
    window
        .set_ignore_cursor_events(false)
        .context("keep GNOME preview movable")?;
    Ok(())
}

pub fn is_gnome_mode_active() -> bool {
    matches!(
        crate::overlay::pick_strategy(),
        crate::overlay::OverlayStrategy::WaylandGnomeMode
    )
}
