//! LiveBlock — Linux entry point. Tauri 2 with a Rust hot path. Wayland or
//! X11 picked at runtime.
//!
//! IPC vocabulary matches the Windows port verbatim so the shared frontend
//! at `platform/_shared-frontend/` works against either platform.

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod capture;
mod detection;
#[path = "../../../_shared-backend/onnx_runtime.rs"]
mod onnx_runtime;
mod hotkeys;
mod inpainting;
mod labels;
mod overlay;
mod paths;
mod regions;
mod runtime;
mod session;
mod state;
mod training;
mod tray;

use std::path::PathBuf;
use std::sync::atomic::Ordering;
use std::sync::Arc;
use tauri::{AppHandle, Emitter, Listener, Manager, State};
use uuid::Uuid;

use crate::detection::Detector;
use crate::hotkeys::HotkeyAction;
use crate::labels::{LabelDocument, ScreenshotEntry};
use crate::regions::{NormalizedRegion, RegionStore, SharedRegionStore};
use crate::session::detect_session;
use crate::state::{AppState, CaptureHandle};

#[tauri::command]
fn get_session_info(app: AppHandle) -> serde_json::Value {
    let strategy = overlay::pick_strategy();
    serde_json::json!({
        "session": match detect_session() {
            session::SessionType::Wayland => "wayland",
            session::SessionType::X11 => "x11",
            session::SessionType::Unknown => "unknown",
        },
        "compositor": format!("{:?}", session::detect_compositor()),
        "supports_layer_shell": session::supports_layer_shell(session::detect_compositor()),
        "overlay_strategy": format!("{:?}", strategy),
        "overlay_fidelity": format!("{:?}", overlay::fidelity(&app)),
    })
}

// ---------- Capture / detection lifecycle ----------

#[tauri::command]
fn start_capture(app: AppHandle, state: State<'_, Arc<AppState>>) -> Result<(), String> {
    if !state.overlay_ready.load(Ordering::SeqCst) { return Err("Overlay unavailable. On X11, enable an alpha compositor and restart LiveBlock.".into()); }
    // Idempotent: if already running, do nothing.
    if state.capture_running.swap(true, Ordering::SeqCst) {
        return Ok(());
    }
    state.cover_only_active.store(false, Ordering::SeqCst);
    state.cover_only_preferred.store(false, Ordering::SeqCst);
    if let Err(error) = overlay::align_window(&app, "render") {
        state.capture_running.store(false, Ordering::SeqCst);
        return Err(error.to_string());
    }
    try_load_default_detector(&app);
    let st = state.inner().clone();
    let generation = st.capture_generation.fetch_add(1, Ordering::SeqCst) + 1;
    let join = runtime::spawn(app, st.clone(), generation);
    *st.capture_task.lock() = Some(CaptureHandle { join });
    Ok(())
}

#[tauri::command]
fn stop_capture(app: AppHandle, state: State<'_, Arc<AppState>>) -> Result<(), String> {
    runtime::stop_capture(&app, &state);
    Ok(())
}

/// Synchronize the selected method before a tray or global-shortcut start.
#[tauri::command]
fn set_cover_only_preferred(cover_only: bool, state: State<'_, Arc<AppState>>) -> Result<(), String> {
    if state.capture_running.load(Ordering::SeqCst) {
        return Err("Stop blocking before changing its method.".into());
    }
    state.cover_only_preferred.store(cover_only, Ordering::SeqCst);
    Ok(())
}

#[tauri::command]
fn capture_status(app: AppHandle, state: State<'_, Arc<AppState>>) -> serde_json::Value {
    serde_json::json!({"running": state.capture_running.load(Ordering::SeqCst), "coverOnly": state.cover_only_active.load(Ordering::SeqCst), "detectionEnabled": state.detection_enabled.load(Ordering::SeqCst), "trainingRunning": state.training_job.lock().as_ref().is_some_and(|job| job.is_running()), "detectorReady": state.detector.lock().is_some(), "overlayMode": format!("{:?}", overlay::fidelity(&app))})
}

fn refresh_cover(app: &AppHandle, state: &AppState) -> Result<(), String> {
    if !state.cover_only_active.load(Ordering::SeqCst) { return Ok(()); }
    let regions = state.region_store.current();
    let patches: Vec<inpainting::PatchPayload> = liveblock_core::paint_over_regions(regions.iter(), liveblock_core::Fill::opaque_black())
        .into_iter().enumerate().map(|(i, patch)| inpainting::PatchPayload {
            id: format!("cover-{i}"), x: patch.rect.x as f64, y: patch.rect.y as f64,
            width: patch.rect.width as f64, height: patch.rect.height as f64,
            png_data_url: String::new(), fill: Some(patch.fill.into()),
        }).collect();
    *state.current_patches.lock() = patches.clone();
    app.emit("patches-updated", patches).map_err(|e| e.to_string())
}

#[tauri::command]
fn start_paint_over(app: AppHandle, state: State<'_, Arc<AppState>>) -> Result<(), String> {
    if !state.overlay_ready.load(Ordering::SeqCst) { return Err("Overlay unavailable. On X11, enable an alpha compositor and restart LiveBlock.".into()); }
    stop_capture(app.clone(), state.clone())?;
    overlay::align_window(&app, "render").map_err(|e| e.to_string())?;
    let window = app.get_webview_window("render").ok_or("Overlay window missing")?;
    state.detection_enabled.store(false, Ordering::SeqCst);
    state.cover_only_active.store(true, Ordering::SeqCst);
    state.cover_only_preferred.store(true, Ordering::SeqCst);
    if let Err(error) = refresh_cover(&app, &state).and_then(|_| window.show().map_err(|e| e.to_string())) {
        let _ = stop_capture(app, state);
        return Err(error);
    }
    state.capture_running.store(true, Ordering::SeqCst);
    let _ = app.emit("capture-state-changed", true);
    Ok(())
}

#[tauri::command]
fn current_patches(state: State<'_, Arc<AppState>>) -> Vec<inpainting::PatchPayload> {
    state.current_patches.lock().clone()
}

#[tauri::command]
fn set_detection_enabled(enabled: bool, state: State<'_, Arc<AppState>>) -> Result<(), String> {
    if enabled && state.cover_only_active.load(Ordering::SeqCst) { return Err("Automatic detection needs screen capture. Stop Cover only first.".into()); }
    if enabled && state.detector.lock().is_none() { return Err("No detector loaded. Install a trained model and restart capture.".into()); }
    state.detection_enabled.store(enabled, Ordering::SeqCst);
    Ok(())
}

#[derive(serde::Serialize)]
struct MonitorInfo {
    id: String,
    name: String,
    is_primary: bool,
    width: u32,
    height: u32,
}

#[tauri::command]
fn list_monitors(app: AppHandle) -> Vec<serde_json::Value> {
    // Enumerate via Tauri's window/monitor API (GTK/GDK backend), which works on
    // both Wayland and X11. The per-output capture surface is still chosen by the
    // portal/root grab, but this gives the frontend a real monitor list. Monitor
    // methods live on a webview window in Tauri 2, so we query the control
    // window (always present at runtime).
    let Some(win) = app.get_webview_window("control") else {
        return vec![synthetic_primary()];
    };
    let primary_name = win
        .primary_monitor()
        .ok()
        .flatten()
        .and_then(|m| m.name().cloned());

    let monitors = match win.available_monitors() {
        Ok(m) if !m.is_empty() => m,
        _ => return vec![synthetic_primary()],
    };

    let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();
    let mut out = Vec::new();
    for m in monitors {
        let name = m.name().cloned().unwrap_or_else(|| "display".into());
        let size = m.size();
        let id = format!("{name}-{}x{}", size.width, size.height);
        if !seen.insert(id.clone()) {
            continue;
        }
        let is_primary = primary_name.as_deref() == Some(name.as_str());
        out.push(
            serde_json::to_value(MonitorInfo {
                id,
                name,
                is_primary,
                width: size.width,
                height: size.height,
            })
            .unwrap(),
        );
    }
    if out.is_empty() {
        out.push(synthetic_primary());
    }
    out
}

/// Last-resort synthetic primary so the frontend's picker never renders empty.
fn synthetic_primary() -> serde_json::Value {
    serde_json::json!({
        "id": "primary",
        "name": "Primary display",
        "is_primary": true,
        "width": 0,
        "height": 0,
    })
}

// ---------- Region CRUD ----------

#[tauri::command]
fn list_regions(state: State<'_, Arc<AppState>>) -> Vec<NormalizedRegion> {
    state.region_store.current()
}

#[tauri::command]
fn add_region(
    app: AppHandle,
    region: NormalizedRegion,
    state: State<'_, Arc<AppState>>,
) -> Result<NormalizedRegion, String> {
    state
        .region_store
        .add(region.clone())
        .map_err(|e| e.to_string())?;
    let _ = app.emit("regions-updated", state.region_store.current());
    refresh_cover(&app, &state)?;
    Ok(region)
}

#[tauri::command]
fn replace_region(
    app: AppHandle,
    id: Uuid,
    region: NormalizedRegion,
    state: State<'_, Arc<AppState>>,
) -> Result<(), String> {
    state
        .region_store
        .replace_id(id, region)
        .map_err(|e| e.to_string())?;
    let _ = app.emit("regions-updated", state.region_store.current());
    refresh_cover(&app, &state)?;
    Ok(())
}

#[tauri::command]
fn delete_region(
    app: AppHandle,
    id: Uuid,
    state: State<'_, Arc<AppState>>,
) -> Result<(), String> {
    state.region_store.remove(id).map_err(|e| e.to_string())?;
    let _ = app.emit("regions-updated", state.region_store.current());
    refresh_cover(&app, &state)?;
    Ok(())
}

#[tauri::command]
fn clear_regions(app: AppHandle, state: State<'_, Arc<AppState>>) -> Result<(), String> {
    state.region_store.clear().map_err(|e| e.to_string())?;
    let _ = app.emit("regions-updated", state.region_store.current());
    refresh_cover(&app, &state)?;
    Ok(())
}

// ---------- Labeling pipeline ----------

#[tauri::command]
fn capture_screenshot_for_labeling(
    state: State<'_, Arc<AppState>>,
) -> Result<Option<PathBuf>, String> {
    let Some(frame) = state.latest_frame.load_full() else {
        return Ok(None);
    };
    paths::ensure_directories().map_err(|e| e.to_string())?;
    let stem = paths::new_screenshot_stem();
    let path = paths::screenshots_dir().join(format!("{stem}.png"));
    let mut rgba = Vec::with_capacity(frame.pixels.len());
    for c in frame.pixels.chunks_exact(4) { rgba.extend_from_slice(&[c[2], c[1], c[0], 255]); }
    let img = image::ImageBuffer::<image::Rgba<u8>, _>::from_raw(frame.width, frame.height, rgba)
        .ok_or_else(|| "invalid frame buffer".to_string())?;
    img.save(&path).map_err(|e| e.to_string())?;
    Ok(Some(path))
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
    doc.save(&paths::label_path_for(&path)).map_err(|e| e.to_string())
}

#[tauri::command]
fn load_label(path: PathBuf) -> Result<Option<LabelDocument>, String> {
    let path = paths::label_path_for(&path);
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
fn start_training(app: AppHandle, epochs: u32, batch: u32, imgsz: u32, dataset: Option<String>,
    state: State<'_, Arc<AppState>>) -> Result<(), String> {
    let mut active = state.training_job.lock();
    if active.as_ref().is_some_and(|job| job.is_running()) { return Err("Training is already running".into()); }
    *active = Some(training::TrainingJob::start(app, epochs, batch, imgsz, dataset).map_err(|e| e.to_string())?);
    Ok(())
}

#[tauri::command]
fn cancel_training(state: State<'_, Arc<AppState>>) -> Result<(), String> {
    if let Some(job) = state.training_job.lock().as_ref() { job.cancel(); }
    Ok(())
}

// ---------- Window lifecycle ----------

#[tauri::command]
fn show_window(app: AppHandle, label: &str) -> Result<(), String> {
    if label == "editor" {
        return overlay::show_editor(&app).map_err(|error| error.to_string());
    }
    let w = app.get_webview_window(label).ok_or_else(|| format!("Unknown window: {label}"))?;
    w.show().map_err(|e| e.to_string())?;
    w.set_focus().map_err(|e| e.to_string())?;
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

/// Wire the global-hotkey receiver loop. `hotkeys::install` spawns the platform
/// listener (ashpd portal on Wayland / XGrabKey on X11); here we drain the
/// channel and turn each action into app state changes + events. PanicDisable is
/// handled unconditionally and first so it can never be starved.
fn spawn_hotkey_receiver(app: AppHandle, state: Arc<AppState>) {
    let (tx, rx) = crossbeam_channel::unbounded::<HotkeyAction>();
    if let Err(e) = hotkeys::install(tx) {
        tracing::warn!("global hotkeys unavailable: {e:#}");
        return;
    }
    std::thread::Builder::new()
        .name("hotkey-receiver".into())
        .spawn(move || {
            for action in rx.iter() {
                match action {
                    HotkeyAction::PanicDisable => {
                        let active = app.state::<Arc<AppState>>();
                        active.detection_enabled.store(false, Ordering::SeqCst);
                        let _ = stop_capture(app.clone(), active);
                        if let Some(editor) = app.get_webview_window("editor") {
                            let _ = editor.hide();
                        }
                        let _ = app.emit("panic-disabled", ());
                    }
                    HotkeyAction::ToggleCapture => {
                        let _ = app.emit("tray-toggle-capture", ());
                    }
                    HotkeyAction::ToggleEditor => {
                        if let Some(w) = app.get_webview_window("editor") {
                            // Toggle visibility.
                            let visible = w.is_visible().unwrap_or(false);
                            if visible {
                                let _ = w.hide();
                            } else if let Err(error) = overlay::show_editor(&app) {
                                tracing::warn!("region editor: {error}");
                                let _ = app.emit("operation-failed", error.to_string());
                            }
                        }
                    }
                    HotkeyAction::CaptureForLabeling => {
                        let _ = app.emit("hotkey-capture-for-labeling", ());
                    }
                }
            }
        })
        .ok();
}

/// Try to load the bundled COCO detector. Absence is non-fatal: the pipeline
/// degrades to user-drawn regions only. NOTE: with the SAFE-STUB empty
/// allowlist in `runtime.rs`, even a loaded COCO model auto-erases NOTHING.
fn try_load_default_detector(app: &AppHandle) {
    let bundled: PathBuf = match app
        .path()
        .resolve("yolov8n.onnx", tauri::path::BaseDirectory::Resource)
    {
        Ok(p) => p,
        Err(_) => return,
    };
    let runtime = paths::data_root().join("models/yolov8n.onnx");
    let candidate = if runtime.exists() { runtime } else if bundled.exists() { bundled }
        else { app.path().resolve("resources/yolov8n.onnx", tauri::path::BaseDirectory::Resource)
            .unwrap_or(bundled) };
    if !candidate.exists() {
        tracing::warn!(
            "detector model not found at {} — drop yolov8n.onnx into resources/",
            candidate.display()
        );
        return;
    }
    let resource_dir = app.path().resource_dir().unwrap_or_default();
    if let Err(error) = onnx_runtime::initialize(&resource_dir) {
        tracing::error!("Detector unavailable: {error:#}");
        return;
    }
    match Detector::load(&candidate) {
        Ok(d) => {
            if let Some(state) = app.try_state::<Arc<AppState>>() {
                *state.detector.lock() = Some(d);
                tracing::info!("loaded detector from {}", candidate.display());
            }
        }
        Err(e) => tracing::error!("detector load failed: {e:#}"),
    }
}

fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "info".parse().unwrap()),
        )
        .init();

    // Drive Tauri (and our `tokio::spawn`ed capture loop) on a multi-thread
    // tokio runtime. The capture loop uses `block_in_place` for the blocking
    // x11rb / channel recvs, which REQUIRES the multi-thread scheduler.
    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .expect("build tokio runtime");
    tauri::async_runtime::set(rt.handle().clone());

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
        .manage(app_state.clone())
        .setup(move |app| {
            let handle = app.handle().clone();

            // Overlay: configure the click-through strategy for this session.
            let strategy = overlay::pick_strategy();
            match overlay::install_click_through(&handle, strategy) {
                Ok(()) => app_state.overlay_ready.store(true, Ordering::SeqCst),
                Err(e) => tracing::warn!("overlay install failed: {e:#}"),
            }
            if strategy == overlay::OverlayStrategy::WaylandGnomeMode {
                overlay::wayland_gnome::announce(&handle);
            }

            // Tray + global hotkeys (now actually CALLED — previously dead code).
            if let Err(e) = tray::install(&handle) {
                tracing::warn!("tray install failed: {e:#}");
            }
            spawn_hotkey_receiver(handle.clone(), app_state.clone());

            // Dispatch tray and hotkey events in Rust, even with a hidden control window.
            let toggle_app = handle.clone();
            handle.listen("tray-toggle-capture", move |_| {
                let active = toggle_app.state::<Arc<AppState>>();
                if active.capture_running.load(Ordering::SeqCst) {
                    let _ = stop_capture(toggle_app.clone(), active);
                } else if let Err(error) = if active.cover_only_preferred.load(Ordering::SeqCst) {
                    start_paint_over(toggle_app.clone(), active)
                } else { start_capture(toggle_app.clone(), active) } {
                    tracing::error!("capture toggle: {error}");
                }
            });
            let screenshot_app = handle.clone();
            handle.listen("hotkey-capture-for-labeling", move |_| {
                if matches!(capture_screenshot_for_labeling(screenshot_app.state::<Arc<AppState>>()), Ok(Some(_))) {
                    if let Some(w) = screenshot_app.get_webview_window("labeling") {
                        let _ = w.show(); let _ = w.set_focus();
                    }
                }
            });

            // Load the bundled detector if present (non-fatal if absent).
            try_load_default_detector(&handle);

            // Hide the auxiliary windows on launch; the control window stays.
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
            start_paint_over,
            set_cover_only_preferred,
            current_patches,
            stop_capture,
            set_detection_enabled,
            capture_status,
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
