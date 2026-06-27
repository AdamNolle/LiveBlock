//! System tray (app indicator). Mirrors the Windows port's tray and macOS's
//! NSStatusItem. On Linux, Tauri's `TrayIconBuilder` talks to the desktop's
//! StatusNotifierItem / libappindicator host (KDE, GNOME-with-extension, most
//! tray daemons). Where no SNI host exists the icon simply doesn't appear; all
//! actions remain reachable from the control window and global hotkeys, so the
//! app stays fully usable.

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

    let mut builder = TrayIconBuilder::new()
        .menu(&menu)
        .tooltip("LiveBlock")
        .on_menu_event(|app, event| match event.id().0.as_str() {
            "show" => show_focus(app, "control"),
            "toggle-capture" => {
                // The control window owns capture state; ask it to toggle so the
                // tray and the UI never disagree. Matches the Windows port.
                let _ = app.emit("tray-toggle-capture", ());
            }
            "open-editor" => show_focus(app, "editor"),
            "open-labeling" => show_focus(app, "labeling"),
            "open-training" => show_focus(app, "training"),
            "quit" => app.exit(0),
            _ => {}
        });

    // Use the bundled window icon if Tauri exposes one (set via tauri.conf.json
    // bundle.icon). Falling back to no explicit icon lets the host pick a
    // default rather than failing the build.
    if let Some(icon) = app.default_window_icon().cloned() {
        builder = builder.icon(icon);
    }

    let _tray = builder.build(app)?;
    Ok(())
}

fn show_focus(app: &AppHandle, label: &str) {
    if let Some(w) = app.get_webview_window(label) {
        let _ = w.show();
        let _ = w.set_focus();
    }
}
