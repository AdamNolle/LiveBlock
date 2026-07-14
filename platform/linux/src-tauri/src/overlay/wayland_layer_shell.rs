//! Configure Tauri's GTK render window as a non-interactive layer-shell
//! surface on KDE/wlroots compositors.

use anyhow::{Context, Result};
use gtk_layer_shell::{Edge, KeyboardMode, Layer, LayerShell};
use tauri::Manager;

pub fn install(app: &tauri::AppHandle) -> Result<()> {
    let render = app
        .get_webview_window("render")
        .context("render window is unavailable")?;
    let gtk_window = render.gtk_window().context("resolve GTK render window")?;
    gtk_window.init_layer_shell();
    gtk_window.set_layer(Layer::Overlay);
    gtk_window.set_keyboard_mode(KeyboardMode::None);
    gtk_window.set_exclusive_zone(0);
    gtk_window.set_namespace("liveblock-overlay");
    for edge in [Edge::Left, Edge::Right, Edge::Top, Edge::Bottom] {
        gtk_window.set_anchor(edge, true);
    }
    render
        .set_ignore_cursor_events(true)
        .context("make layer-shell overlay click-through")?;
    Ok(())
}
