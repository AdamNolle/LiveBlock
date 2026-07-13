//! LiveBlock — Windows entry. DPI-aware Tauri 2 app with a Rust hot path.

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod capture;
mod capture_policy;
mod detection;
mod hotkeys;
mod inpainting;
mod labels;
mod model_updates;
mod overlay;
mod paths;
mod regions;
mod state;
mod tray;
mod training;

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use parking_lot::Mutex;
use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager, State};
use uuid::Uuid;

use crate::capture::{enumerate_monitors, CaptureSession, FrameView};
use crate::capture_policy::{CaptureTelemetry, CaptureTelemetrySnapshot, ProtectedFrameDetector};
use crate::detection::{DetBox, Detector};
use crate::inpainting::PatchPayload;
use crate::labels::{LabelDocument, ScreenshotEntry};
use crate::regions::{NormalizedRegion, RegionStore};
use crate::state::AppState;

/// Set DPI awareness so the layered overlay tracks per-monitor DPI.
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

            // Apply per-window native styles after Tauri has created HWNDs.
            apply_window_styles(&handle);

            // Register tray + global hotkeys.
            tray::install(&handle).ok();
            hotkeys::spawn(handle.clone());

            // Try to load a default detector if the model is present.
            try_load_default_detector(&handle);

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            get_capabilities,
            start_capture,
            stop_capture,
            get_capture_telemetry,
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
            model_updates::install_model_update,
            quit,
            show_window,
            hide_window,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

#[cfg(windows)]
fn apply_window_styles(app: &AppHandle) {
    use windows::Win32::Foundation::HWND;
    if let Some(w) = app.get_webview_window("render") {
        if let Ok(h) = w.hwnd() {
            let hwnd = HWND(h.0 as *mut _);
            if let Err(e) = overlay::make_render_overlay(hwnd) {
                tracing::error!("make_render_overlay: {e}");
            }
        }
    }
    if let Some(w) = app.get_webview_window("editor") {
        if let Ok(h) = w.hwnd() {
            let hwnd = HWND(h.0 as *mut _);
            if let Err(e) = overlay::make_editor(hwnd) {
                tracing::error!("make_editor: {e}");
            }
        }
    }
}

#[cfg(not(windows))]
fn apply_window_styles(_: &AppHandle) {}

fn try_load_default_detector(app: &AppHandle) {
    let detector = match model_updates::load_authenticated_active(app) {
        Ok(Some(detector)) => {
            tracing::info!("loaded authenticated detector update");
            Some(detector)
        }
        Ok(None) => match model_updates::load_authenticated_packaged(app) {
            Ok(detector) => detector,
            Err(error) => {
                tracing::error!("authenticated packaged detector rejected: {error}");
                return;
            }
        },
        Err(error) => {
            // Accepted update state exists but failed recovery/authentication.
            // Never roll the running detector back to a packaged baseline.
            tracing::error!("authenticated detector update rejected: {error}");
            return;
        }
    };
    if let Some(detector) = detector {
        if let Some(state) = app.try_state::<AppState>() {
            *state.detector.lock() = Some(detector);
        }
    } else {
        tracing::warn!("no authenticated detector is installed or packaged");
    }
}

// ===== Tauri commands =====

#[tauri::command]
fn get_capabilities() -> Result<liveblock_config::DesktopCapabilityProfile, String> {
    let profile = liveblock_config::DesktopCapabilityProfile::windows();
    profile.validate().map_err(str::to_string)?;
    Ok(profile)
}

#[tauri::command]
fn list_monitors() -> Vec<MonitorInfo> {
    enumerate_monitors()
        .into_iter()
        .enumerate()
        .map(|(index, (handle, name))| MonitorInfo {
            id: (handle.0 as usize).to_string(),
            name,
            is_primary: index == 0,
        })
        .collect()
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct MonitorInfo {
    id: String,
    name: String,
    is_primary: bool,
}

#[tauri::command]
fn start_capture(monitor_id: String, state: State<'_, AppState>) -> Result<(), String> {
    let monitor_hmonitor = monitor_id
        .parse::<usize>()
        .map_err(|_| "invalid monitor id".to_string())?;
    let selected = enumerate_monitors()
        .into_iter()
        .map(|(handle, _)| handle)
        .find(|handle| handle.0 as usize == monitor_hmonitor)
        .ok_or_else(|| "selected monitor is no longer available".to_string())?;

    // Start/stop/publication share one lock so a concurrent stop cannot miss a
    // not-yet-published session and a concurrent start cannot overwrite one.
    let _lifecycle_guard = state.capture_lifecycle.lock();
    let generation = state
        .capture_generation
        .fetch_add(1, Ordering::SeqCst)
        .wrapping_add(1);
    let existing = { state.capture.lock().take() };
    if let Some(existing) = existing {
        existing.stop();
        let _ = state.app.emit("capture-state-changed", false);
        let _ = state.app.emit("patches-updated", Vec::<PatchPayload>::new());
    }
    let app = state.app.clone();
    let regions_arc = state.regions.clone();
    let inpainter_arc = state.inpainter.clone();
    let detector_arc = state.detector.clone();
    let detection_on = state.detection_enabled.clone();
    let last_detection: Arc<Mutex<(Instant, Vec<DetBox>)>> =
        Arc::new(Mutex::new((Instant::now() - Duration::from_secs(1), Vec::new())));
    let protected_detector = Arc::new(Mutex::new(ProtectedFrameDetector::default()));
    let telemetry = Arc::new(CaptureTelemetry::default());
    let telemetry_for_frame = telemetry.clone();

    let frame_counter = AtomicU64::new(0);
    let on_frame = Arc::new(move |frame: &FrameView| {
        let frame_number = frame_counter.fetch_add(1, Ordering::Relaxed).wrapping_add(1);

        let mut protected = protected_detector.lock();
        if let Some(value) = protected.observe_bgra(&frame.bytes, frame.width, frame.height) {
            telemetry_for_frame.set_protected(value);
            let _ = app.emit("protected-content-changed", value);
        }
        if protected.is_protected() {
            telemetry_for_frame.protected_frame();
            last_detection.lock().1.clear();
            drop(protected);
            let _ = app.emit("patches-updated", Vec::<PatchPayload>::new());
            return;
        }
        drop(protected);

        // Detection every fourth processed frame. CaptureSession's capacity-one
        // queue keeps this CPU/ORT work off the WGC callback and drops stale work.
        if detection_on.load(Ordering::Relaxed) && frame_number % 4 == 0 {
            if let Some(det) = detector_arc.lock().as_mut() {
                if let Ok(boxes) = det.detect(&frame.bytes, frame.width, frame.height) {
                    *last_detection.lock() = (Instant::now(), boxes);
                }
            }
        }

        // Build region list = user regions ∪ recent detections.
        let mut regions: Vec<NormalizedRegion> = regions_arc.current();
        let last = last_detection.lock();
        if last.0.elapsed() < Duration::from_millis(800) {
            for b in &last.1 {
                if b.confidence < 0.5 { continue; }
                regions.push(NormalizedRegion::new(
                    b.x as f64 / frame.width as f64,
                    b.y as f64 / frame.height as f64,
                    b.width as f64 / frame.width as f64,
                    b.height as f64 / frame.height as f64,
                ));
            }
        }
        drop(last);

        // Inpaint.
        let payloads: Vec<PatchPayload> = inpainter_arc
            .lock()
            .render(&frame.bytes, frame.width, frame.height, &regions)
            .unwrap_or_default();

        let _ = app.emit("patches-updated", &payloads);
    }) as Arc<dyn Fn(&FrameView) + Send + Sync>;

    let error_app = state.app.clone();
    let capture_slot = state.capture.clone();
    let capture_lifecycle = state.capture_lifecycle.clone();
    let active_generation = state.capture_generation.clone();
    let failure_reported = Arc::new(AtomicBool::new(false));
    let on_error = Arc::new(move |message: String| {
        if active_generation.load(Ordering::SeqCst) != generation
            || failure_reported.swap(true, Ordering::SeqCst)
        { return; }
        tracing::error!("capture runtime failure: {message}");
        let _ = error_app.emit("capture-runtime-error", &message);
        let _ = error_app.emit("capture-state-changed", false);
        let _ = error_app.emit("protected-content-changed", false);
        let _ = error_app.emit("patches-updated", Vec::<PatchPayload>::new());

        // Never tear down from a WGC callback or the frame worker itself. A
        // short-lived cleanup thread waits for start_capture to publish this
        // generation, then owns deterministic handler/session/worker closure.
        let slot = capture_slot.clone();
        let lifecycle = capture_lifecycle.clone();
        let generation_counter = active_generation.clone();
        std::thread::spawn(move || {
            for _ in 0..50 {
                let lifecycle_guard = lifecycle.lock();
                let failed = {
                    let mut guard = slot.lock();
                    if generation_counter.load(Ordering::SeqCst) != generation { return; }
                    guard.take()
                };
                if let Some(session) = failed {
                    session.stop();
                    drop(lifecycle_guard);
                    return;
                }
                drop(lifecycle_guard);
                std::thread::sleep(Duration::from_millis(10));
            }
        });
    });
    let session = CaptureSession::start(selected, on_frame, on_error, telemetry)
        .map_err(|e| e.to_string())?;
    if state.capture_generation.load(Ordering::SeqCst) != generation {
        session.stop();
        return Err("capture start was superseded".into());
    }
    *state.capture.lock() = Some(session);
    let _ = state.app.emit("protected-content-changed", false);
    let _ = state.app.emit("capture-state-changed", true);
    Ok(())
}

#[tauri::command]
fn stop_capture(state: State<'_, AppState>) -> Result<(), String> {
    let _lifecycle_guard = state.capture_lifecycle.lock();
    state.capture_generation.fetch_add(1, Ordering::SeqCst);
    let session = { state.capture.lock().take() };
    if let Some(session) = session { session.stop(); }
    let _ = state.app.emit("capture-state-changed", false);
    let _ = state.app.emit("protected-content-changed", false);
    let _ = state.app.emit("patches-updated", Vec::<PatchPayload>::new());
    Ok(())
}

#[tauri::command]
fn get_capture_telemetry(state: State<'_, AppState>) -> CaptureTelemetrySnapshot {
    state.capture.lock().as_ref().map(CaptureSession::telemetry).unwrap_or_default()
}

#[tauri::command]
fn set_detection_enabled(enabled: bool, state: State<'_, AppState>) -> Result<(), String> {
    state.detection_enabled.store(enabled, Ordering::Relaxed);
    Ok(())
}

#[tauri::command]
fn list_regions(state: State<'_, AppState>) -> Vec<NormalizedRegion> {
    state.regions.current()
}

#[tauri::command]
fn add_region(region: NormalizedRegion, state: State<'_, AppState>) -> Result<NormalizedRegion, String> {
    state.regions.add(region.clone()).map_err(|e| e.to_string())?;
    let _ = state.app.emit("regions-updated", state.regions.current());
    Ok(region)
}

#[tauri::command]
fn replace_region(id: Uuid, region: NormalizedRegion, state: State<'_, AppState>) -> Result<(), String> {
    state.regions.replace_id(id, region).map_err(|e| e.to_string())?;
    let _ = state.app.emit("regions-updated", state.regions.current());
    Ok(())
}

#[tauri::command]
fn delete_region(id: Uuid, state: State<'_, AppState>) -> Result<(), String> {
    state.regions.remove(id).map_err(|e| e.to_string())?;
    let _ = state.app.emit("regions-updated", state.regions.current());
    Ok(())
}

#[tauri::command]
fn clear_regions(state: State<'_, AppState>) -> Result<(), String> {
    state.regions.clear().map_err(|e| e.to_string())?;
    let _ = state.app.emit("regions-updated", state.regions.current());
    Ok(())
}

#[tauri::command]
fn capture_screenshot_for_labeling(state: State<'_, AppState>) -> Result<Option<PathBuf>, String> {
    let frame = match state.capture.lock().as_ref() {
        Some(s) => s.latest_frame(),
        None => return Ok(None),
    };
    let Some(frame) = frame else { return Ok(None); };
    let _ = paths::ensure_directories();
    let stem = paths::new_screenshot_stem();
    let dst = paths::screenshots_dir().join(format!("{stem}.png"));
    // Encode BGRA to RGBA → PNG.
    let mut rgba = Vec::with_capacity(frame.bytes.len());
    for c in frame.bytes.chunks_exact(4) {
        rgba.extend_from_slice(&[c[2], c[1], c[0], c[3]]);
    }
    let img = image::ImageBuffer::<image::Rgba<u8>, _>::from_raw(frame.width, frame.height, rgba)
        .ok_or("invalid frame")?;
    img.save(&dst).map_err(|e| e.to_string())?;
    Ok(Some(dst))
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
            if !ext.eq_ignore_ascii_case("png") { return None; }
            let stem = p.file_stem()?.to_string_lossy().into_owned();
            let label = paths::label_path_for(&p);
            Some(ScreenshotEntry { path: p, stem, labeled: label.exists() })
        })
        .collect();
    out.sort_by(|a, b| a.stem.cmp(&b.stem));
    out
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ScreenshotData {
    width: u32,
    height: u32,
    /// data:image/png;base64,...
    png_data_url: String,
}

#[tauri::command]
fn load_screenshot(path: PathBuf) -> Result<ScreenshotData, String> {
    use base64::{engine::general_purpose::STANDARD, Engine as _};
    let bytes = std::fs::read(&path).map_err(|e| e.to_string())?;
    let img = image::load_from_memory(&bytes).map_err(|e| e.to_string())?;
    Ok(ScreenshotData {
        width: img.width(),
        height: img.height(),
        png_data_url: format!("data:image/png;base64,{}", STANDARD.encode(&bytes)),
    })
}

#[tauri::command]
fn save_label(path: PathBuf, doc: LabelDocument) -> Result<(), String> {
    doc.save(&path).map_err(|e| e.to_string())
}

#[tauri::command]
fn load_label(path: PathBuf) -> Result<Option<LabelDocument>, String> {
    if !path.exists() { return Ok(None); }
    LabelDocument::load(&path).map(Some).map_err(|e| e.to_string())
}

#[tauri::command]
fn discard_screenshot(path: PathBuf) -> Result<(), String> {
    let _ = paths::ensure_directories();
    let dst = paths::trash_dir().join(path.file_name().ok_or("no name")?);
    std::fs::rename(&path, &dst).map_err(|e| e.to_string())
}

#[tauri::command]
fn start_training(epochs: u32, batch: u32, imgsz: u32, state: State<'_, AppState>) -> Result<(), String> {
    if !liveblock_config::developer_training_runtime_available() {
        return Err("release builds are inference-only; use the source training workflow".into());
    }
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

#[tauri::command]
fn show_window(label: String, app: AppHandle) -> Result<(), String> {
    app.get_webview_window(&label)
        .ok_or_else(|| format!("no window {label}"))?
        .show()
        .map_err(|e| e.to_string())
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
