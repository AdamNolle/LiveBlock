//! Configure the actual Tauri patch renderer as a click-through overlay.
//! X11 spans the virtual desktop. Supported Wayland compositors initialize the
//! actual hidden renderer and editor as GTK layer surfaces before realization.

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

/// Fidelity class of an overlay strategy. `LayerExact` means a true full-screen,
/// pixel-exact, click-through overlay; `FloatingDegraded` is the GNOME fallback
/// (a movable always-on-top window). The frontend uses this to decide whether to
/// show the degraded-mode banner.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OverlayFidelity {
    /// X11 renderer aligned to the virtual desktop.
    LayerExact,
    /// Wayland compositor-managed floating fallback.
    FloatingDegraded,
}

impl OverlayStrategy {
    pub fn fidelity(self) -> OverlayFidelity {
        match self {
            OverlayStrategy::X11OverrideRedirect => {
                OverlayFidelity::LayerExact
            }
            OverlayStrategy::WaylandGnomeMode | OverlayStrategy::WaylandLayerShell => OverlayFidelity::FloatingDegraded,
        }
    }
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

/// Configure the window that actually receives patch events.
/// Protocol probes alone do not make the Tauri renderer a native layer surface.
pub fn install_click_through(app: &tauri::AppHandle, strategy: OverlayStrategy) -> Result<()> {
    use tauri::Manager;
    let render = app.get_webview_window("render")
        .ok_or_else(|| anyhow::anyhow!("Render window missing"))?;
    use gtk::prelude::WidgetExt;
    let native = render.gtk_window()?;
    if detect_session() == SessionType::X11
        && !native.screen().is_some_and(|screen| screen.is_composited())
    {
        anyhow::bail!("Enable an X11 compositor before blocking; transparent overlays require alpha compositing");
    }
    if detect_session() == SessionType::Wayland && gtk_layer_shell::is_supported() {
        install_layer_surfaces(app, &native)?;
    }
    native.realize();
    let window = native.window()
        .ok_or_else(|| anyhow::anyhow!("GTK render window could not be realized"))?;
    // A managed, borderless X11 window still gets a window-manager frame,
    // which can swallow pointer events even when its child has an empty shape.
    // Configure the actual renderer before its first map, not a spare window.
    if detect_session() == SessionType::X11 {
        window.set_override_redirect(true);
    }
    // Store the shape on the GTK widget so hiding, mapping and resizing do
    // not restore its input area. Reapply after each map as well.
    native.input_shape_combine_region(Some(&gtk::cairo::Region::create()));
    native.connect_map(|widget| {
        widget.input_shape_combine_region(Some(&gtk::cairo::Region::create()));
    });
    render.set_focusable(false)?;
    if detect_session() == SessionType::X11 {
        use std::sync::{atomic::Ordering, Arc};
        use tauri::Emitter;
        // Losing alpha compositing makes the transparent desktop-sized window
        // opaque. Hide both overlays immediately; readiness stays off until the
        // app restarts so a later Start cannot show an opaque window again.
        let handle = app.clone();
        if let Some(screen) = native.screen() {
            screen.connect_composited_changed(move |screen| {
                if screen.is_composited() { return; }
                let state = handle.state::<Arc<crate::state::AppState>>();
                if !state.overlay_ready.swap(false, Ordering::SeqCst) { return; }
                state.detection_enabled.store(false, Ordering::SeqCst);
                crate::runtime::stop_capture(&handle, &state);
                if let Some(editor) = handle.get_webview_window("editor") {
                    let _ = editor.hide();
                }
                tracing::warn!("X11 compositor lost; blocking and editor stopped");
                let _ = handle.emit("operation-failed", "X11 compositor stopped. Blocking has been stopped. Restart the compositor and LiveBlock before blocking again.");
            });
        }
    }
    if strategy != OverlayStrategy::X11OverrideRedirect && !layer_shell_active(app) {
        tracing::warn!("Wayland renderer uses a compositor-managed floating window; exact fullscreen placement is not guaranteed");
    }
    Ok(())
}

/// Initialize both actual Tauri windows while hidden and unrealized.
/// GTK layer-shell cannot convert an already realized window safely.
fn install_layer_surfaces(app: &tauri::AppHandle, render: &gtk::ApplicationWindow) -> Result<()> {
    use gtk::prelude::*;
    use gtk_layer_shell::{Edge, KeyboardMode, Layer, LayerShell};
    use std::sync::{atomic::Ordering, Arc};
    use tauri::Manager;
    let editor = app.get_webview_window("editor")
        .ok_or_else(|| anyhow::anyhow!("Region editor window missing"))?.gtk_window()?;
    tracing::info!(render_realized = render.is_realized(), editor_realized = editor.is_realized(), "Wayland overlay setup");
    if render.is_realized() || editor.is_realized() {
        anyhow::bail!("Wayland layer overlays must initialize before their GTK windows are realized");
    }
    let display = render.display();
    let monitor = display.primary_monitor().or_else(|| display.monitor(0))
        .ok_or_else(|| anyhow::anyhow!("Wayland output unavailable"))?;
    for (window, namespace, keyboard) in [
        (render, "liveblock-render", KeyboardMode::None),
        (&editor, "liveblock-editor", KeyboardMode::Exclusive),
    ] {
        window.init_layer_shell();
        window.set_namespace(namespace);
        window.set_layer(Layer::Overlay);
        window.set_keyboard_mode(keyboard);
        window.set_exclusive_zone(-1);
        window.set_monitor(&monitor);
        for edge in [Edge::Top, Edge::Bottom, Edge::Left, Edge::Right] {
            window.set_anchor(edge, true);
            window.set_layer_shell_margin(edge, 0);
        }
        if !window.is_layer_window() {
            anyhow::bail!("Wayland layer initialization failed for {namespace}");
        }
    }
    app.state::<Arc<crate::state::AppState>>().layer_shell_active.store(true, Ordering::SeqCst);
    tracing::info!("Actual Wayland renderer and editor initialized as layer surfaces");
    Ok(())
}

pub fn layer_shell_active(app: &tauri::AppHandle) -> bool {
    use std::sync::{atomic::Ordering, Arc};
    use tauri::Manager;
    app.state::<Arc<crate::state::AppState>>().layer_shell_active.load(Ordering::SeqCst)
}

pub fn fidelity(app: &tauri::AppHandle) -> OverlayFidelity {
    if layer_shell_active(app) { OverlayFidelity::LayerExact } else { pick_strategy().fidelity() }
}

/// Match the capture's virtual desktop on X11, or the primary monitor for
/// the Wayland floating fallback. Multi-monitor portal placement is not exact.
pub fn align_window(app: &tauri::AppHandle, label: &str) -> Result<()> {
    use tauri::Manager;
    let window = app.get_webview_window(label).ok_or_else(|| anyhow::anyhow!("Window {label} missing"))?;
    if layer_shell_active(app) { return Ok(()); }
    if detect_session() == SessionType::X11 {
        use x11rb::connection::Connection;
        let (connection, screen) = x11rb::connect(None)?;
        let root = &connection.setup().roots[screen];
        window.set_position(tauri::PhysicalPosition::new(0, 0))?;
        window.set_size(tauri::PhysicalSize::new(root.width_in_pixels as u32, root.height_in_pixels as u32))?;
    } else if let Some(monitor) = window.primary_monitor()? {
        window.set_position(*monitor.position())?;
        window.set_size(*monitor.size())?;
    }
    Ok(())
}

/// Use the same checked placement path for IPC, tray actions and hotkeys.
pub fn show_editor(app: &tauri::AppHandle) -> Result<()> {
    use std::sync::{atomic::Ordering, Arc};
    use tauri::Manager;
    let state = app.state::<Arc<crate::state::AppState>>();
    if !state.overlay_ready.load(Ordering::SeqCst) {
        anyhow::bail!("Overlay unavailable. On X11, enable an alpha compositor and restart LiveBlock.");
    }
    align_window(app, "editor")?;
    let window = app.get_webview_window("editor")
        .ok_or_else(|| anyhow::anyhow!("Region editor window missing"))?;
    window.show()?;
    if !layer_shell_active(app) { window.set_focus()?; }
    Ok(())
}
