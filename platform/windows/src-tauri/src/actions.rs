//! Backend actions shared by the Tauri command layer (`main.rs`), the system
//! tray (`tray.rs`), and the global hotkeys (`hotkeys.rs`).
//!
//! These live in their own module (compiled by BOTH the `[lib]` and the binary)
//! so the tray/hotkey handlers can call real logic without depending on
//! `main.rs`-only items. Each function takes the `AppHandle` and operates on the
//! managed [`AppState`].

use std::sync::atomic::Ordering;
use std::sync::Arc;

use tauri::{AppHandle, Emitter, Manager};

use crate::capture::CaptureSession;
use crate::overlay::OverlayWindow;
use crate::pipeline::{PipelineContext, PipelineWorker};
use crate::state::AppState;

/// Emit the capture-state change the frontend listens for
/// (`events.onCaptureState` -> `capture-state-changed`, bare boolean payload).
pub fn emit_capture_state(app: &AppHandle, running: bool) {
    let _ = app.emit("capture-state-changed", running);
}

/// HMONITOR (as i64) of the primary display, if resolvable.
#[cfg(windows)]
pub fn primary_monitor_id() -> Option<i64> {
    use windows::Win32::Foundation::POINT;
    use windows::Win32::Graphics::Gdi::{MonitorFromPoint, MONITOR_DEFAULTTOPRIMARY};
    let h = unsafe { MonitorFromPoint(POINT { x: 0, y: 0 }, MONITOR_DEFAULTTOPRIMARY) };
    if h.0.is_null() {
        None
    } else {
        Some(h.0 as isize as i64)
    }
}

#[cfg(not(windows))]
pub fn primary_monitor_id() -> Option<i64> {
    None
}

/// Start capture on `monitor_id` (stringified HMONITOR), or the primary display
/// when `None`. Creates/repositions the native overlay, starts WGC capture into
/// the shared `latest` arc-swap, and spawns the off-thread pipeline worker.
#[cfg(windows)]
pub fn start_capture(state: &AppState, monitor_id: Option<String>) -> anyhow::Result<()> {
    use windows::Win32::Graphics::Gdi::HMONITOR;

    // Restart cleanly if already running.
    stop_capture(state);

    let target: HMONITOR = match monitor_id.and_then(|s| s.parse::<i64>().ok()) {
        Some(v) => HMONITOR(v as isize as *mut _),
        None => match primary_monitor_id() {
            Some(v) => HMONITOR(v as isize as *mut _),
            None => crate::capture::enumerate_monitors()
                .first()
                .map(|(h, _)| *h)
                .ok_or_else(|| anyhow::anyhow!("no monitors found"))?,
        },
    };

    let rect = crate::overlay::monitor_rect(target)
        .ok_or_else(|| anyhow::anyhow!("could not query monitor rect"))?;
    let overlay: Arc<OverlayWindow> = {
        let mut guard = state.overlay.lock();
        match guard.as_ref() {
            Some(o) => {
                o.position_on(rect)?;
                o.clone()
            }
            None => {
                let o = OverlayWindow::create(rect)?;
                *guard = Some(o.clone());
                o
            }
        }
    };
    overlay.show()?;

    let session = CaptureSession::start(target, state.latest_frame.clone())?;
    let ticks = session.ticks();

    let ctx = PipelineContext {
        latest: state.latest_frame.clone(),
        overlay: overlay.clone(),
        detector: state.detector.clone(),
        detection_enabled: state.detection_enabled.clone(),
        regions: state.region_snapshot.clone(),
    };
    let worker = PipelineWorker::spawn(ctx, ticks);

    *state.capture.lock() = Some(session);
    *state.pipeline.lock() = Some(worker);

    emit_capture_state(&state.app, true);
    Ok(())
}

#[cfg(not(windows))]
pub fn start_capture(_state: &AppState, _monitor_id: Option<String>) -> anyhow::Result<()> {
    Err(anyhow::anyhow!("capture is only implemented on Windows"))
}

/// Stop capture: join the worker, close the session, clear + hide the overlay.
pub fn stop_capture(state: &AppState) {
    if let Some(worker) = state.pipeline.lock().take() {
        worker.stop();
    }
    if let Some(session) = state.capture.lock().take() {
        session.stop();
    }
    if let Some(overlay) = state.overlay.lock().as_ref() {
        let _ = overlay.present(&[]);
        let _ = overlay.hide();
    }
}

/// Instantly stop covering: disable detection, stop capture, hide overlay.
pub fn panic_disable(state: &AppState) {
    state.detection_enabled.store(false, Ordering::SeqCst);
    stop_capture(state);
    emit_capture_state(&state.app, false);
    tracing::info!("panic-disable engaged");
}

/// DRM-safe capture-free PaintOver: cover the user's regions with opaque-black
/// patches on the native overlay WITHOUT starting any WGC capture at all. This
/// is the path for fully content-protected video — nothing is captured, so HDCP
/// has nothing to black out, and we never read a protected pixel. The overlay
/// just draws flat covers over the marked regions.
///
/// Stops any running capture first (PaintOver and the capture+inpaint path are
/// mutually exclusive), then shows the overlay on the primary monitor and
/// presents the region covers. Idempotent; call again after editing regions.
#[cfg(windows)]
pub fn paint_over_regions_only(state: &AppState) -> anyhow::Result<()> {
    use liveblock_core::{paint_over_regions, Fill};

    // No capturing while in PaintOver mode.
    stop_capture(state);
    state.detection_enabled.store(false, Ordering::SeqCst);

    // Bind the overlay to the primary monitor (PaintOver isn't tied to a capture
    // target since nothing is captured).
    let target = match primary_monitor_id() {
        Some(v) => v,
        None => crate::capture::enumerate_monitors()
            .first()
            .map(|(h, _)| h.0 as isize as i64)
            .ok_or_else(|| anyhow::anyhow!("no monitors found"))?,
    };
    use windows::Win32::Graphics::Gdi::HMONITOR;
    let rect = crate::overlay::monitor_rect(HMONITOR(target as isize as *mut _))
        .ok_or_else(|| anyhow::anyhow!("could not query monitor rect"))?;

    let overlay: Arc<OverlayWindow> = {
        let mut guard = state.overlay.lock();
        match guard.as_ref() {
            Some(o) => {
                o.position_on(rect)?;
                o.clone()
            }
            None => {
                let o = OverlayWindow::create(rect)?;
                *guard = Some(o.clone());
                o
            }
        }
    };
    overlay.show()?;

    // Build opaque-black PaintOver patches straight from the user regions — no
    // frame, no capture.
    let regions = state.regions.current();
    let patches: Vec<crate::overlay::CoverPatch> =
        paint_over_regions(regions.iter(), Fill::opaque_black())
            .into_iter()
            .map(crate::overlay::CoverPatch::from)
            .collect();
    overlay.present(&patches)?;
    tracing::info!("paint-over (capture-free DRM-safe) presenting {} regions", regions.len());
    Ok(())
}

#[cfg(not(windows))]
pub fn paint_over_regions_only(_state: &AppState) -> anyhow::Result<()> {
    Err(anyhow::anyhow!("paint-over is only implemented on Windows"))
}

/// Toggle capture based on the current running state (tray / hotkey).
pub fn toggle_capture(app: &AppHandle) {
    let Some(state) = app.try_state::<AppState>() else {
        return;
    };
    let running = state.capture.lock().is_some();
    if running {
        stop_capture(&state);
        emit_capture_state(app, false);
    } else if let Err(e) = start_capture(&state, None) {
        tracing::error!("toggle_capture: start failed: {e}");
    }
}

/// Panic-disable from the tray / hotkey.
pub fn panic_disable_action(app: &AppHandle) {
    if let Some(state) = app.try_state::<AppState>() {
        panic_disable(&state);
    }
}

/// Capture a labeling screenshot of the freshest frame (tray / hotkey).
pub fn capture_screenshot_action(app: &AppHandle) {
    let Some(state) = app.try_state::<AppState>() else {
        return;
    };
    match crate::screenshot::save_latest_frame(&state) {
        Ok(Some(path)) => tracing::info!("captured screenshot {}", path.display()),
        Ok(None) => tracing::warn!("no frame available for screenshot"),
        Err(e) => tracing::error!("screenshot failed: {e}"),
    }
}

/// Show a webview window by label (tray / hotkey).
pub fn show_window_action(app: &AppHandle, label: &str) {
    if let Some(w) = app.get_webview_window(label) {
        let _ = w.show();
        let _ = w.set_focus();
    }
}
