//! LiveBlock — Windows entry. DPI-aware Tauri 2 app with a Rust hot path.
//!
//! The realtime path is fully native: WGC capture -> off-thread pipeline worker
//! (detect -> Tracker -> decide_verdict[safe stub] -> build_remove_mask_from_tracks
//! -> DRM-aware cover) -> a native layered overlay window. There is no
//! webview/base64 patch channel anymore.

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod actions;
mod capture;
mod detection;
mod hotkeys;
mod inpainting;
mod labels;
mod overlay;
mod paths;
mod pipeline;
mod regions;
mod screenshot;
mod state;
mod tray;
mod training;

use std::path::PathBuf;
use std::sync::atomic::Ordering;
use std::sync::Arc;

use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager, State};
use uuid::Uuid;

use crate::capture::enumerate_monitors;
use crate::detection::Detector;
use crate::labels::{LabelDocument, ScreenshotEntry};
use crate::regions::{NormalizedRegion, RegionStore};
use crate::state::AppState;

/// Set DPI awareness so the layered overlay tracks per-monitor DPI (V2).
#[cfg(windows)]
fn set_dpi_awareness() {
    use windows::Win32::UI::HiDpi::{
        SetProcessDpiAwarenessContext, DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2,
    };
    unsafe {
        let _ = SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
    }
}

#[cfg(not(windows))]
fn set_dpi_awareness() {}

fn main() {
    set_dpi_awareness();
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();
    let _ = paths::ensure_directories();

    let regions = Arc::new(
        RegionStore::open(paths::regions_path()).unwrap_or_else(|_| RegionStore::in_memory()),
    );

    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_fs::init())
        .plugin(tauri_plugin_shell::init())
        .plugin(tauri_plugin_os::init())
        .setup(move |app| {
            let handle = app.handle().clone();
            let state = AppState::new(handle.clone(), regions.clone());
            app.manage(state);

            // Apply native click-through / capture-exclusion styles to the
            // existing Tauri webview windows (control/editor/render).
            apply_window_styles(&handle);

            // Register tray + global hotkeys (both drive real backend actions).
            if let Err(e) = tray::install(&handle) {
                tracing::error!("tray install failed: {e}");
            }
            hotkeys::spawn(handle.clone());

            // Try to load a default detector if the model is present.
            try_load_default_detector(&handle);

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            start_capture,
            stop_capture,
            start_paint_over,
            set_detection_enabled,
            list_monitors,
            list_regions,
            add_region,
            replace_region,
            delete_region,
            clear_regions,
            capture_screenshot_for_labeling,
            list_screenshots,
            load_screenshot,
            save_label,
            load_label,
            discard_screenshot,
            start_training,
            cancel_training,
            quit,
            show_window,
            hide_window,
            panic_disable,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

#[cfg(windows)]
fn apply_window_styles(app: &AppHandle) {
    use windows::Win32::Foundation::HWND;
    // Tauri/tao use a DIFFERENT windows-rs version than this crate, so
    // `w.hwnd()` yields their HWND. Rebuild OUR `windows-0.58` HWND from the raw
    // pointer (`*mut c_void`) — both are pointer-newtypes, compatible by value.
    let rebuild = |raw: *mut core::ffi::c_void| HWND(raw);
    if let Some(w) = app.get_webview_window("render") {
        if let Ok(h) = w.hwnd() {
            let hwnd = rebuild(h.0 as *mut _);
            if let Err(e) = overlay::make_render_overlay(hwnd) {
                tracing::error!("make_render_overlay: {e}");
            }
        }
    }
    if let Some(w) = app.get_webview_window("editor") {
        if let Ok(h) = w.hwnd() {
            let hwnd = rebuild(h.0 as *mut _);
            if let Err(e) = overlay::make_editor(hwnd) {
                tracing::error!("make_editor: {e}");
            }
        }
    }
}

#[cfg(not(windows))]
fn apply_window_styles(_: &AppHandle) {}

fn try_load_default_detector(app: &AppHandle) {
    let candidate: PathBuf = match app
        .path()
        .resolve("yolov8n.onnx", tauri::path::BaseDirectory::Resource)
    {
        Ok(p) => p,
        Err(_) => return,
    };
    if !candidate.exists() {
        tracing::warn!(
            "detector model not found at {} — drop yolov8n.onnx into resources/",
            candidate.display()
        );
        return;
    }
    match Detector::load(&candidate) {
        Ok(d) => {
            if let Some(state) = app.try_state::<AppState>() {
                *state.detector.lock() = Some(d);
                tracing::info!("loaded detector from {}", candidate.display());
            }
        }
        Err(e) => tracing::error!("detector load failed: {e}"),
    }
}

// ===== Monitor enumeration =====

/// Frontend `MonitorInfo`: `{ id: string, name: string, is_primary: bool }`.
#[derive(Serialize)]
struct MonitorInfo {
    /// Stringified HMONITOR pointer value; pass back to `start_capture`.
    id: String,
    name: String,
    is_primary: bool,
}

#[tauri::command]
fn list_monitors() -> Vec<MonitorInfo> {
    let primary = actions::primary_monitor_id();
    enumerate_monitors()
        .into_iter()
        .map(|(h, name)| {
            let id = (h.0 as isize as i64).to_string();
            let is_primary = Some(h.0 as isize as i64) == primary;
            MonitorInfo { id, name, is_primary }
        })
        .collect()
}

// ===== Capture lifecycle (thin wrappers over `actions`) =====

/// Start capturing. The frontend calls this with no arguments (capture the
/// primary display); an optional `monitor_id` (stringified HMONITOR from
/// `list_monitors`) selects a specific display.
#[tauri::command]
fn start_capture(monitor_id: Option<String>, state: State<'_, AppState>) -> Result<(), String> {
    actions::start_capture(&state, monitor_id).map_err(|e| e.to_string())
}

#[tauri::command]
fn stop_capture(state: State<'_, AppState>) -> Result<(), String> {
    actions::stop_capture(&state);
    actions::emit_capture_state(&state.app, false);
    Ok(())
}

/// Panic-disable: instantly stop covering and hide the overlay, and disable
/// detection. Mirrors macOS Cmd+Shift+Opt+. Also wired to the hotkey + tray.
#[tauri::command]
fn panic_disable(state: State<'_, AppState>) -> Result<(), String> {
    actions::panic_disable(&state);
    Ok(())
}

/// DRM-safe capture-free PaintOver: cover the user's regions with opaque black
/// on the native overlay WITHOUT capturing any pixels (for fully content-
/// protected video). See `actions::paint_over_regions_only`.
#[tauri::command]
fn start_paint_over(state: State<'_, AppState>) -> Result<(), String> {
    actions::paint_over_regions_only(&state).map_err(|e| e.to_string())
}

#[tauri::command]
fn set_detection_enabled(enabled: bool, state: State<'_, AppState>) -> Result<(), String> {
    state.detection_enabled.store(enabled, Ordering::Relaxed);
    Ok(())
}

// ===== Region CRUD (keeps the worker snapshot in sync) =====

#[tauri::command]
fn list_regions(state: State<'_, AppState>) -> Vec<NormalizedRegion> {
    state.regions.current()
}

/// Returns the stored region (the frontend `ipc.ts` types `addRegion` as
/// `invoke<NormalizedRegion>`).
#[tauri::command]
fn add_region(
    region: NormalizedRegion,
    state: State<'_, AppState>,
) -> Result<NormalizedRegion, String> {
    let stored = region.clone();
    state.regions.add(region).map_err(|e| e.to_string())?;
    state.refresh_region_snapshot();
    let _ = state.app.emit("regions-updated", state.regions.current());
    Ok(stored)
}

#[tauri::command]
fn replace_region(
    id: Uuid,
    region: NormalizedRegion,
    state: State<'_, AppState>,
) -> Result<(), String> {
    state
        .regions
        .replace_id(id, region)
        .map_err(|e| e.to_string())?;
    state.refresh_region_snapshot();
    let _ = state.app.emit("regions-updated", state.regions.current());
    Ok(())
}

#[tauri::command]
fn delete_region(id: Uuid, state: State<'_, AppState>) -> Result<(), String> {
    state.regions.remove(id).map_err(|e| e.to_string())?;
    state.refresh_region_snapshot();
    let _ = state.app.emit("regions-updated", state.regions.current());
    Ok(())
}

#[tauri::command]
fn clear_regions(state: State<'_, AppState>) -> Result<(), String> {
    state.regions.clear().map_err(|e| e.to_string())?;
    state.refresh_region_snapshot();
    let _ = state.app.emit("regions-updated", state.regions.current());
    Ok(())
}

// ===== Labeling pipeline =====

#[tauri::command]
fn capture_screenshot_for_labeling(state: State<'_, AppState>) -> Result<Option<PathBuf>, String> {
    // Read + encode the freshest captured frame. Shared with the tray/hotkey
    // screenshot action so both produce identical PNGs.
    screenshot::save_latest_frame(&state).map_err(|e| e.to_string())
}

#[tauri::command]
fn list_screenshots() -> Vec<ScreenshotEntry> {
    let dir = paths::screenshots_dir();
    let entries = match std::fs::read_dir(&dir) {
        Ok(d) => d,
        Err(_) => return Vec::new(),
    };
    let mut out: Vec<ScreenshotEntry> = entries
        .flatten()
        .filter_map(|e| {
            let p = e.path();
            let ext = p.extension().and_then(|x| x.to_str())?;
            if !ext.eq_ignore_ascii_case("png") {
                return None;
            }
            let stem = p.file_stem()?.to_string_lossy().into_owned();
            let label = paths::label_path_for(&p);
            Some(ScreenshotEntry {
                path: p,
                stem,
                labeled: label.exists(),
            })
        })
        .collect();
    out.sort_by(|a, b| a.stem.cmp(&b.stem));
    out
}

/// Return the raw PNG bytes for a screenshot. The frontend (`ipc.ts`) types this
/// as `number[]`; serde serializes `Vec<u8>` to a JSON number array, which the
/// labeling UI turns into a Blob/ObjectURL — no base64 data-url anymore.
#[tauri::command]
fn load_screenshot(path: PathBuf) -> Result<Vec<u8>, String> {
    std::fs::read(&path).map_err(|e| e.to_string())
}

#[tauri::command]
fn save_label(path: PathBuf, doc: LabelDocument) -> Result<(), String> {
    doc.save(&path).map_err(|e| e.to_string())
}

#[tauri::command]
fn load_label(path: PathBuf) -> Result<Option<LabelDocument>, String> {
    if !path.exists() {
        return Ok(None);
    }
    LabelDocument::load(&path).map(Some).map_err(|e| e.to_string())
}

#[tauri::command]
fn discard_screenshot(path: PathBuf) -> Result<(), String> {
    let _ = paths::ensure_directories();
    let dst = paths::trash_dir().join(path.file_name().ok_or("no name")?);
    std::fs::rename(&path, &dst).map_err(|e| e.to_string())
}

// ===== Training =====

#[tauri::command]
fn start_training(
    epochs: u32,
    batch: u32,
    imgsz: u32,
    state: State<'_, AppState>,
) -> Result<(), String> {
    let job = training::TrainingJob::start(state.app.clone(), epochs, batch, imgsz)
        .map_err(|e| e.to_string())?;
    *state.training.lock() = Some(job);
    Ok(())
}

#[tauri::command]
fn cancel_training(state: State<'_, AppState>) -> Result<(), String> {
    if let Some(job) = state.training.lock().take() {
        job.cancel();
    }
    Ok(())
}

// ===== Window lifecycle (names match the frontend `ipc.ts`) =====

#[tauri::command]
fn show_window(label: String, app: AppHandle) -> Result<(), String> {
    let w = app
        .get_webview_window(&label)
        .ok_or_else(|| format!("no window {label}"))?;
    w.show().map_err(|e| e.to_string())?;
    let _ = w.set_focus();
    Ok(())
}

#[tauri::command]
fn hide_window(label: String, app: AppHandle) -> Result<(), String> {
    app.get_webview_window(&label)
        .ok_or_else(|| format!("no window {label}"))?
        .hide()
        .map_err(|e| e.to_string())
}

#[tauri::command]
fn quit(app: AppHandle) {
    app.exit(0);
}
