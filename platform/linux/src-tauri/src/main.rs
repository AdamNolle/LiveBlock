//! LiveBlock — Linux entry point. Tauri 2 with a Rust hot path. Wayland or
//! X11 picked at runtime.
//!
//! IPC vocabulary matches the Windows port verbatim so the shared frontend
//! at `platform/_shared-frontend/` works against either platform.

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod capture;
mod detection;
mod hotkeys;
mod inpainting;
mod labels;
mod overlay;
mod paths;
mod regions;
mod session;
mod state;
mod training;

use std::path::PathBuf;
use std::sync::atomic::Ordering;
use std::sync::Arc;
use tauri::{AppHandle, Manager, State};
use uuid::Uuid;

use crate::labels::{LabelDocument, ScreenshotEntry};
use crate::regions::{NormalizedRegion, RegionStore, SharedRegionStore};
use crate::session::detect_session;
use crate::state::AppState;

#[tauri::command]
fn get_session_info() -> serde_json::Value {
    serde_json::json!({
        "session": match detect_session() {
            session::SessionType::Wayland => "wayland",
            session::SessionType::X11 => "x11",
            session::SessionType::Unknown => "unknown",
        },
        "compositor": format!("{:?}", session::detect_compositor()),
        "supports_layer_shell": session::supports_layer_shell(session::detect_compositor()),
        "overlay_strategy": format!("{:?}", overlay::pick_strategy()),
    })
}

// ---------- Capture / detection lifecycle ----------

#[tauri::command]
fn start_capture(state: State<'_, Arc<AppState>>) -> Result<(), String> {
    state.capture_running.store(true, Ordering::SeqCst);
    Ok(())
}

#[tauri::command]
fn stop_capture(state: State<'_, Arc<AppState>>) -> Result<(), String> {
    state.capture_running.store(false, Ordering::SeqCst);
    Ok(())
}

#[tauri::command]
fn set_detection_enabled(enabled: bool, state: State<'_, Arc<AppState>>) -> Result<(), String> {
    state.detection_enabled.store(enabled, Ordering::SeqCst);
    Ok(())
}

#[tauri::command]
fn list_monitors() -> Vec<serde_json::Value> {
    // Linux capture surface is per-output but the runtime enumeration depends
    // on the live wayland/x11 connection. Until that's wired, return a single
    // synthetic primary so the frontend's monitor picker doesn't render empty.
    vec![serde_json::json!({
        "id": "primary",
        "name": "Primary display",
        "is_primary": true,
    })]
}

// ---------- Region CRUD ----------

#[tauri::command]
fn list_regions(state: State<'_, Arc<AppState>>) -> Vec<NormalizedRegion> {
    state.region_store.current()
}

#[tauri::command]
fn add_region(
    region: NormalizedRegion,
    state: State<'_, Arc<AppState>>,
) -> Result<NormalizedRegion, String> {
    state
        .region_store
        .add(region.clone())
        .map_err(|e| e.to_string())?;
    Ok(region)
}

#[tauri::command]
fn replace_region(
    id: Uuid,
    region: NormalizedRegion,
    state: State<'_, Arc<AppState>>,
) -> Result<(), String> {
    state
        .region_store
        .replace_id(id, region)
        .map_err(|e| e.to_string())
}

#[tauri::command]
fn delete_region(id: Uuid, state: State<'_, Arc<AppState>>) -> Result<(), String> {
    state.region_store.remove(id).map_err(|e| e.to_string())
}

#[tauri::command]
fn clear_regions(state: State<'_, Arc<AppState>>) -> Result<(), String> {
    state.region_store.clear().map_err(|e| e.to_string())
}

// ---------- Labeling pipeline ----------

#[tauri::command]
fn capture_screenshot_for_labeling() -> Result<Option<PathBuf>, String> {
    // The Wayland/X11 capture surface isn't yet plumbed into the labeling
    // pipeline. Return None so the frontend treats this as "no frame yet"
    // rather than an error. Wiring is symmetric with Windows once
    // `capture::latest_frame()` is exposed.
    Ok(None)
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
    paths::ensure_directories().map_err(|e| e.to_string())?;
    let dst = paths::trash_dir().join(path.file_name().ok_or("no filename")?);
    std::fs::rename(&path, &dst).map_err(|e| e.to_string())
}

// ---------- Training pipeline ----------

#[tauri::command]
fn start_training(_epochs: u32, _batch: u32, _imgsz: u32) -> Result<(), String> {
    // Training driver lives in the Python pipeline at tools/train_logos.py.
    // The Rust side dispatches and pipes progress through stdout; the
    // training module owns the lifecycle. Stub for now — Linux wires this
    // identically to Windows once the training module ships.
    Ok(())
}

#[tauri::command]
fn cancel_training() -> Result<(), String> {
    Ok(())
}

// ---------- Window lifecycle ----------

#[tauri::command]
fn show_window(app: AppHandle, label: &str) -> Result<(), String> {
    if let Some(w) = app.get_webview_window(label) {
        let _ = w.show();
        let _ = w.set_focus();
    }
    Ok(())
}

#[tauri::command]
fn hide_window(app: AppHandle, label: &str) -> Result<(), String> {
    if let Some(w) = app.get_webview_window(label) {
        let _ = w.hide();
    }
    Ok(())
}

#[tauri::command]
fn quit(app: AppHandle) {
    app.exit(0);
}

fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "info".parse().unwrap()),
        )
        .init();

    let _ = paths::ensure_directories();
    let region_store: SharedRegionStore = Arc::new(
        RegionStore::open(paths::regions_path()).unwrap_or_else(|_| RegionStore::in_memory()),
    );
    let app_state = AppState::new(region_store);

    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_fs::init())
        .plugin(tauri_plugin_shell::init())
        .plugin(tauri_plugin_os::init())
        .manage(app_state)
        .setup(|app| {
            if let Err(e) = overlay::install_click_through(overlay::pick_strategy()) {
                tracing::warn!("overlay install failed: {e}");
            }
            for label in ["editor", "render", "labeling", "training"] {
                if let Some(w) = app.get_webview_window(label) {
                    let _ = w.hide();
                }
            }
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            get_session_info,
            start_capture,
            stop_capture,
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
            show_window,
            hide_window,
            quit,
        ])
        .run(tauri::generate_context!())
        .expect("tauri::Builder run");
}
