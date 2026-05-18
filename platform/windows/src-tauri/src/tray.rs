//! System tray. Mirrors macOS NSStatusItem usage in `LiveBlockApp.swift`.

use anyhow::Result;
use tauri::{
    menu::{Menu, MenuItem, PredefinedMenuItem},
    tray::TrayIconBuilder,
    AppHandle, Emitter, Manager,
};

pub fn install(app: &AppHandle) -> Result<()> {
    let show = MenuItem::with_id(app, "show", "Show Control Panel", true, None::<&str>)?;
    let toggle = MenuItem::with_id(app, "toggle-capture", "Toggle Capture", true, None::<&str>)?;
    let editor = MenuItem::with_id(app, "open-editor", "Open Region Editor", true, None::<&str>)?;
    let labeling = MenuItem::with_id(app, "open-labeling", "Open Labeling", true, None::<&str>)?;
    let training = MenuItem::with_id(app, "open-training", "Open Training", true, None::<&str>)?;
    let separator = PredefinedMenuItem::separator(app)?;
    let quit = MenuItem::with_id(app, "quit", "Quit LiveBlock", true, None::<&str>)?;

    let menu = Menu::with_items(
        app,
        &[&show, &toggle, &editor, &labeling, &training, &separator, &quit],
    )?;

    let _tray = TrayIconBuilder::new()
        .menu(&menu)
        .tooltip("LiveBlock")
        .on_menu_event(|app, event| match event.id().0.as_str() {
            "show" => {
                if let Some(w) = app.get_webview_window("control") {
                    let _ = w.show();
                    let _ = w.set_focus();
                }
            }
            "toggle-capture" => {
                let _ = app.emit("tray-toggle-capture", ());
            }
            "open-editor" => {
                if let Some(w) = app.get_webview_window("editor") {
                    let _ = w.show();
                }
            }
            "open-labeling" => {
                if let Some(w) = app.get_webview_window("labeling") {
                    let _ = w.show();
                    let _ = w.set_focus();
                }
            }
            "open-training" => {
                if let Some(w) = app.get_webview_window("training") {
                    let _ = w.show();
                    let _ = w.set_focus();
                }
            }
            "quit" => {
                app.exit(0);
            }
            _ => {}
        })
        .build(app)?;

    Ok(())
}
