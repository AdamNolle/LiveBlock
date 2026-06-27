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
mod runtime;
mod session;
mod state;
mod training;
mod tray;

use std::path::PathBuf;
use std::sync::atomic::Ordering;
use std::sync::Arc;
use tauri::{AppHandle, Emitter, Manager, State};
use uuid::Uuid;

use crate::detection::Detector;
use crate::hotkeys::HotkeyAction;
use crate::labels::{LabelDocument, ScreenshotEntry};
use crate::regions::{NormalizedRegion, RegionStore, SharedRegionStore};
use crate::session::detect_session;
use crate::state::{AppState, CaptureHandle};

#[tauri::command]
fn get_session_info() -> serde_json::Value {
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
        "overlay_fidelity": format!("{:?}", strategy.fidelity()),
    })
}

// ---------- Capture / detection lifecycle ----------

#[tauri::command]
fn start_capture(app: AppHandle, state: State<'_, Arc<AppState>>) -> Result<(), String> {
    // Idempotent: if already running, do nothing.
    if state.capture_running.swap(true, Ordering::SeqCst) {
        return Ok(());
    }
    let st = state.inner().clone();
    let join = runtime::spawn(app, st.clone());
    *st.capture_task.lock() = Some(CaptureHandle { join });
    Ok(())
}

#[tauri::command]
fn stop_capture(app: AppHandle, state: State<'_, Arc<AppState>>) -> Result<(), String> {
    state.capture_running.store(false, Ordering::SeqCst);
    // The loop polls `capture_running` and exits on the next frame; abort the
    // task as a backstop so a blocked capture can't linger.
    if let Some(handle) = state.capture_task.lock().take() {
        handle.join.abort();
    }
    state.current_patches.lock().clear();
    let _ = app.emit("capture-state-changed", serde_json::json!({ "running": false }));
    let _ = app.emit("patches-updated", Vec::<inpainting::PatchPayload>::new());
    Ok(())
}

#[tauri::command]
fn set_detection_enabled(enabled: bool, state: State<'_, Arc<AppState>>) -> Result<(), String> {
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
    Ok(())
}

#[tauri::command]
fn clear_regions(app: AppHandle, state: State<'_, Arc<AppState>>) -> Result<(), String> {
    state.region_store.clear().map_err(|e| e.to_string())?;
    let _ = app.emit("regions-updated", state.region_store.current());
    Ok(())
}

// ---------- Labeling pipeline ----------

#[tauri::command]
fn capture_screenshot_for_labeling(
    state: State<'_, Arc<AppState>>,
) -> Result<Option<PathBuf>, String> {
    // Persist the most recent inpaint-source frame? We don't retain raw frames
    // (the hot loop holds at most the current `FrameView`), so the labeling
    // capture takes a fresh single-shot grab off the active capture source.
    //
    // To avoid blocking the UI thread on a portal round-trip, we do a one-shot
    // synchronous X11 grab when on X11 (cheap), and on Wayland we surface the
    // current cached patches' source is unavailable -> return None so the
    // frontend prompts the user to enable capture first. The full Wayland
    // single-shot uses the same portal session as `start_capture`; wiring it to
    // reuse the live `CaptureSource` is the follow-up symmetric with Windows.
    let _ = state; // reserved for future live-frame reuse
    match single_shot_screenshot() {
        Ok(Some(path)) => Ok(Some(path)),
        Ok(None) => Ok(None),
        Err(e) => Err(e),
    }
}

/// Best-effort single screenshot for the labeling pipeline. Returns the saved
/// PNG path, or None if no synchronous capture path is available right now.
fn single_shot_screenshot() -> Result<Option<PathBuf>, String> {
    use crate::capture::{CaptureSource, FrameView};

    // Only attempt the synchronous X11 path here; the Wayland portal path is
    // async and shares the live session (see note in the command above).
    if detect_session() != session::SessionType::X11 {
        return Ok(None);
    }
    paths::ensure_directories().map_err(|e| e.to_string())?;

    // Grab one frame on a short-lived blocking runtime.
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|e| e.to_string())?;
    let frame: Option<FrameView> = rt.block_on(async {
        let mut cap = crate::capture::x11::X11Capture::new().ok()?;
        let f = cap.next_frame().await.ok()?;
        cap.stop();
        if f.width == 0 || f.height == 0 || f.pixels.is_empty() {
            None
        } else {
            Some(f)
        }
    });
    let Some(frame) = frame else { return Ok(None) };

    // Encode BGRA → PNG and write under the screenshots dir.
    let stem = paths::new_screenshot_stem();
    let path = paths::screenshots_dir().join(format!("{stem}.png"));
    let mut rgba = Vec::with_capacity(frame.pixels.len());
    for c in frame.pixels.chunks_exact(4) {
        rgba.extend_from_slice(&[c[2], c[1], c[0], 255]);
    }
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

/// Labeling screenshot payload. Shape MUST match the Windows port (shared
/// frontend `ipc.ts`): `{ width, height, png_data_url }`.
#[derive(serde::Serialize)]
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
fn start_training(
    app: AppHandle,
    _epochs: u32,
    _batch: u32,
    _imgsz: u32,
) -> Result<(), String> {
    // Dispatch the shared `tools/auto.sh` training driver and stream its log to
    // the frontend via `training-log` / `training-finished` events (same vocab
    // as macOS / Windows). The repo root is two levels above the data dir's
    // sibling; we resolve it relative to the running binary's source tree.
    let repo_root = training_repo_root();
    let data_yaml = paths::exports_dir().join("dataset.yaml");
    let rx = training::start_training(repo_root, data_yaml).map_err(|e| e.to_string())?;

    // Pump training events to the frontend on a background thread.
    let app = app.clone();
    std::thread::spawn(move || {
        for evt in rx.iter() {
            match evt {
                training::TrainEvent::Stdout(line) | training::TrainEvent::Stderr(line) => {
                    let _ = app.emit("training-log", line);
                }
                training::TrainEvent::Finished { exit_code } => {
                    let _ = app.emit(
                        "training-finished",
                        serde_json::json!({ "exitCode": exit_code }),
                    );
                    break;
                }
            }
        }
    });
    Ok(())
}

/// Resolve the repo root that owns `tools/auto.sh`. The binary lives at
/// `platform/linux/src-tauri/target/.../liveblock-linux`, so the repo root is a
/// few parents up; we also honor an explicit override env var.
fn training_repo_root() -> PathBuf {
    if let Some(p) = std::env::var_os("LIVEBLOCK_REPO_ROOT") {
        return PathBuf::from(p);
    }
    // Walk up from the current exe looking for a `tools/auto.sh`.
    if let Ok(exe) = std::env::current_exe() {
        let mut dir = exe.parent().map(|p| p.to_path_buf());
        while let Some(d) = dir {
            if d.join("tools").join("auto.sh").exists() {
                return d;
            }
            dir = d.parent().map(|p| p.to_path_buf());
        }
    }
    PathBuf::from(".")
}

#[tauri::command]
fn cancel_training() -> Result<(), String> {
    // The training child is owned by the `training` module's spawned threads;
    // cancellation is cooperative via the auto.sh process group. A hard kill
    // switch is the follow-up; for now stopping is driven from the script side.
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
                        // Hard stop: kill capture + clear all overlay output
                        // immediately. This must always work.
                        state.capture_running.store(false, Ordering::SeqCst);
                        state.detection_enabled.store(false, Ordering::SeqCst);
                        if let Some(handle) = state.capture_task.lock().take() {
                            handle.join.abort();
                        }
                        state.current_patches.lock().clear();
                        let _ = app.emit(
                            "capture-state-changed",
                            serde_json::json!({ "running": false }),
                        );
                        let _ = app
                            .emit("patches-updated", Vec::<inpainting::PatchPayload>::new());
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
                            } else {
                                let _ = w.show();
                                let _ = w.set_focus();
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
            if let Err(e) = overlay::install_click_through(strategy) {
                tracing::warn!("overlay install failed: {e:#}");
            }
            if strategy == overlay::OverlayStrategy::WaylandGnomeMode {
                overlay::wayland_gnome::announce(&handle);
            }

            // Tray + global hotkeys (now actually CALLED — previously dead code).
            if let Err(e) = tray::install(&handle) {
                tracing::warn!("tray install failed: {e:#}");
            }
            spawn_hotkey_receiver(handle.clone(), app_state.clone());

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
