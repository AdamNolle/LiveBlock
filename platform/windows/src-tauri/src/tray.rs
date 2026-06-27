//! System tray. Mirrors macOS NSStatusItem usage in `LiveBlockApp.swift`.
//!
//! Menu actions drive REAL backend handlers (not dead events): toggle capture,
//! panic-disable, open windows. The handlers live in `main.rs` and operate on
//! the managed `AppState`.

use anyhow::Result;
use tauri::{
    menu::{Menu, MenuItem, PredefinedMenuItem},
    tray::TrayIconBuilder,
    AppHandle, Manager,
};

pub fn install(app: &AppHandle) -> Result<()> {
    let show = MenuItem::with_id(app, "show", "Show Control Panel", true, None::<&str>)?;
    let toggle = MenuItem::with_id(app, "toggle-capture", "Toggle Capture", true, None::<&str>)?;
    let paintover = MenuItem::with_id(app, "paint-over", "Cover Regions (DRM-safe, no capture)", true, None::<&str>)?;
    let panic = MenuItem::with_id(app, "panic-disable", "Panic Disable (stop covering)", true, None::<&str>)?;
    let editor = MenuItem::with_id(app, "open-editor", "Open Region Editor", true, None::<&str>)?;
    let labeling = MenuItem::with_id(app, "open-labeling", "Open Labeling", true, None::<&str>)?;
    let training = MenuItem::with_id(app, "open-training", "Open Training", true, None::<&str>)?;
    let separator = PredefinedMenuItem::separator(app)?;
    let quit = MenuItem::with_id(app, "quit", "Quit LiveBlock", true, None::<&str>)?;

    let menu = Menu::with_items(
        app,
        &[&show, &toggle, &paintover, &panic, &editor, &labeling, &training, &separator, &quit],
    )?;

    let _tray = TrayIconBuilder::new()
        .menu(&menu)
        .tooltip("LiveBlock")
        .on_menu_event(|app, event| match event.id().0.as_str() {
            "show" => crate::actions::show_window_action(app, "control"),
            "toggle-capture" => crate::actions::toggle_capture(app),
            "paint-over" => {
                if let Some(state) = app.try_state::<crate::state::AppState>() {
                    if let Err(e) = crate::actions::paint_over_regions_only(&state) {
                        tracing::error!("paint-over failed: {e}");
                    }
                }
            }
            "panic-disable" => crate::actions::panic_disable_action(app),
            "open-editor" => crate::actions::show_window_action(app, "editor"),
            "open-labeling" => crate::actions::show_window_action(app, "labeling"),
            "open-training" => crate::actions::show_window_action(app, "training"),
            "quit" => app.exit(0),
            _ => {}
        })
        .build(app)?;

    Ok(())
}
