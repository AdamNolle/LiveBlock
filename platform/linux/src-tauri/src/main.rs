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
mod model_updates;
mod overlay;
mod paths;
mod regions;
mod session;
mod state;
mod training;

use serde::Serialize;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tauri::{AppHandle, Emitter, Manager, State};
use uuid::Uuid;

use crate::capture::{open_capture, CaptureSource, FrameView};
use crate::detection::DetBox;
use crate::inpainting::PatchPayload;
use crate::labels::{LabelDocument, ScreenshotEntry};
use crate::regions::{NormalizedRegion, RegionStore, SharedRegionStore};
use crate::session::detect_session;
use crate::state::{AppState, CaptureRuntime, CaptureTelemetrySnapshot};

#[tauri::command]
fn get_capabilities(
    state: State<'_, Arc<AppState>>,
) -> Result<liveblock_config::DesktopCapabilityProfile, String> {
    use liveblock_config::{DesktopCapabilityProfile, DesktopPlatform};
    let platform = match detect_session() {
        session::SessionType::X11 => DesktopPlatform::LinuxX11,
        session::SessionType::Wayland => match session::detect_compositor() {
            session::WaylandCompositor::Kwin => DesktopPlatform::LinuxKdeWayland,
            session::WaylandCompositor::Wlroots => DesktopPlatform::LinuxWlrootsWayland,
            session::WaylandCompositor::Gnome => DesktopPlatform::LinuxGnomeWayland,
            session::WaylandCompositor::Unknown => {
                return Err("unsupported or unknown Wayland compositor".into())
            }
        },
        session::SessionType::Unknown => return Err("unknown desktop session".into()),
    };
    let mut profile = DesktopCapabilityProfile::linux(platform);
    if !state.hotkeys_initialized.load(Ordering::SeqCst)
        || !state.hotkeys_available.load(Ordering::SeqCst)
    {
        profile.global_hotkeys = false;
        profile
            .limitations
            .push("global shortcuts are unavailable in this runtime session".into());
    }
    profile.validate().map_err(str::to_string)?;
    Ok(profile)
}

// ---------- Capture / detection lifecycle ----------

static USER_ACTION_SEQUENCE: AtomicU64 = AtomicU64::new(0);

#[tauri::command]
fn begin_user_action() -> u64 {
    USER_ACTION_SEQUENCE.fetch_add(1, Ordering::SeqCst).wrapping_add(1)
}

#[tauri::command]
async fn start_capture(
    monitor_id: String,
    _action_sequence: u64,
    app: AppHandle,
    state: State<'_, Arc<AppState>>,
) -> Result<(), String> {
    start_capture_inner(monitor_id, app, state.inner().clone()).await
}

async fn start_capture_inner(
    monitor_id: String,
    app: AppHandle,
    state: Arc<AppState>,
) -> Result<(), String> {
    let expected_id = match detect_session() {
        session::SessionType::Wayland => "portal-selection",
        session::SessionType::X11 => "x11-root",
        session::SessionType::Unknown => return Err("unknown Linux display server".into()),
    };
    if monitor_id != expected_id {
        return Err("selected Linux capture target is no longer available".into());
    }

    let mut lifecycle = state.capture.lock().await;
    if let Some(previous) = lifecycle.take() {
        previous.stop_requested.store(true, Ordering::Release);
        let _ = previous.task.await;
    }
    state.capture_telemetry.reset();
    state.latest_frame.store(None);
    state.capture_running.store(false, Ordering::SeqCst);
    let stop_requested = Arc::new(AtomicBool::new(false));
    let task_stop = stop_requested.clone();
    let task_state = state.clone();
    let task_app = app.clone();
    let task = tokio::spawn(async move {
        run_capture_task(task_stop, task_state, task_app).await;
    });
    *lifecycle = Some(CaptureRuntime {
        stop_requested,
        task,
    });
    // The task emits `true` only after receiving and validating a first frame.
    // Portal selection can therefore be cancelled by stop/panic without this
    // command holding the lifecycle mutex.
    Ok(())
}

#[tauri::command]
async fn stop_capture(
    _action_sequence: u64,
    app: AppHandle,
    state: State<'_, Arc<AppState>>,
) -> Result<(), String> {
    stop_capture_inner(app, state.inner().clone()).await
}

async fn stop_capture_inner(app: AppHandle, state: Arc<AppState>) -> Result<(), String> {
    // Hold the lifecycle lock through join and global-state cleanup so a new
    // start cannot be overwritten by the previous generation's teardown.
    let mut lifecycle = state.capture.lock().await;
    let runtime = lifecycle.take();
    if let Some(runtime) = &runtime {
        runtime.stop_requested.store(true, Ordering::Release);
    }
    state.capture_running.store(false, Ordering::SeqCst);
    state.latest_frame.store(None);
    *state.current_patches.lock() = Vec::new();
    let _ = app.emit("capture-state-changed", false);
    let _ = app.emit("patches-updated", Vec::<PatchPayload>::new());
    if let Some(window) = app.get_webview_window("render") {
        let _ = window.hide();
    }
    if let Some(runtime) = runtime {
        let _ = runtime.task.await;
    }
    Ok(())
}

#[tauri::command]
fn get_capture_telemetry(state: State<'_, Arc<AppState>>) -> CaptureTelemetrySnapshot {
    state.capture_telemetry.snapshot()
}

#[tauri::command]
fn set_detection_enabled(enabled: bool, state: State<'_, Arc<AppState>>) -> Result<(), String> {
    state.detection_enabled.store(enabled, Ordering::SeqCst);
    Ok(())
}

#[tauri::command]
fn list_monitors() -> Vec<serde_json::Value> {
    match detect_session() {
        session::SessionType::Wayland => vec![serde_json::json!({
            "id": "portal-selection",
            "name": "Choose a display in the system portal",
            "isPrimary": true,
        })],
        session::SessionType::X11 => vec![serde_json::json!({
            "id": "x11-root",
            "name": "X11 virtual desktop",
            "isPrimary": true,
        })],
        session::SessionType::Unknown => Vec::new(),
    }
}

async fn run_capture_task(
    stop_requested: Arc<AtomicBool>,
    state: Arc<AppState>,
    app: AppHandle,
) {
    let source = tokio::select! {
        source = open_capture() => match source {
            Ok(source) => source,
            Err(error) => {
                let message = error.to_string();
                tracing::error!("Linux capture startup failed: {message}");
                let _ = app.emit("capture-runtime-error", message);
                let _ = app.emit("capture-state-changed", false);
                return;
            }
        },
        _ = wait_for_stop(stop_requested.clone()) => return,
    };
    run_capture_loop(source, stop_requested, state, app).await;
}

async fn run_capture_loop(
    mut source: Box<dyn CaptureSource>,
    stop_requested: Arc<AtomicBool>,
    state: Arc<AppState>,
    app: AppHandle,
) {
    let mut frame_number = 0u64;
    let mut recent_detections = (
        Instant::now() - Duration::from_secs(1),
        Vec::<DetBox>::new(),
    );
    let mut failure = None;
    let mut announced_running = false;
    while !stop_requested.load(Ordering::Acquire) {
        let frame_result = tokio::select! {
            frame = source.next_frame() => Some(frame),
            _ = wait_for_stop(stop_requested.clone()) => None,
        };
        let Some(frame_result) = frame_result else { break; };
        let frame = match frame_result {
            Ok(frame) => frame,
            Err(error) => {
                state.capture_telemetry.copy_error();
                failure = Some(error.to_string());
                break;
            }
        };
        if frame.width == 0
            || frame.height == 0
            || frame.stride != frame.width * 4
            || frame.pixels.len() != frame.width as usize * frame.height as usize * 4
        {
            state.capture_telemetry.copy_error();
            failure = Some("capture backend returned an invalid BGRA frame".into());
            break;
        }
        state.capture_telemetry.captured();
        state.capture_telemetry.set_dropped(source.dropped_frames());
        if !announced_running {
            announced_running = true;
            state.capture_running.store(true, Ordering::SeqCst);
            if let Some(window) = app.get_webview_window("render") {
                let _ = window.show();
            }
            let _ = app.emit("capture-state-changed", true);
        }
        state.latest_frame.store(Some(Arc::new(frame.clone())));
        frame_number = frame_number.wrapping_add(1);

        if state.detection_enabled.load(Ordering::Relaxed) && frame_number % 4 == 0 {
            let detector_state = state.clone();
            let detection_frame = frame.clone();
            match tokio::task::spawn_blocking(move || {
                let mut detector = detector_state.detector.lock();
                detector
                    .as_mut()
                    .map(|detector| {
                        detector.detect_bgra(
                            &detection_frame.pixels,
                            detection_frame.width,
                            detection_frame.height,
                        )
                    })
                    .transpose()
            })
            .await
            {
                Ok(Ok(Some(detections))) => {
                    recent_detections = (Instant::now(), detections);
                }
                Ok(Ok(None)) => {}
                Ok(Err(error)) => tracing::warn!("Linux detector frame failed: {error}"),
                Err(error) => tracing::error!("Linux detector worker failed: {error}"),
            }
        }

        let mut regions = state.region_store.current();
        if recent_detections.0.elapsed() < Duration::from_millis(800) {
            for detection in &recent_detections.1 {
                if detection.score < 0.5 {
                    continue;
                }
                regions.push(NormalizedRegion::new(
                    f64::from(detection.x) / f64::from(frame.width),
                    f64::from(detection.y) / f64::from(frame.height),
                    f64::from(detection.w) / f64::from(frame.width),
                    f64::from(detection.h) / f64::from(frame.height),
                ));
            }
        }
        let paint_state = state.clone();
        let paint_frame = frame.clone();
        let patches = match tokio::task::spawn_blocking(move || {
            paint_state.inpainter.lock().inpaint(
                &paint_frame.pixels,
                paint_frame.width,
                paint_frame.height,
                &regions,
            )
        })
        .await
        {
            Ok(Ok(patches)) => patches,
            Ok(Err(error)) => {
                tracing::warn!("Linux inpainting frame failed: {error}");
                Vec::new()
            }
            Err(error) => {
                failure = Some(format!("Linux inpainting worker stopped: {error}"));
                break;
            }
        };
        *state.current_patches.lock() = patches.clone();
        state.capture_telemetry.processed();
        let _ = app.emit("patches-updated", &patches);
        tokio::time::sleep(Duration::from_millis(33)).await;
    }
    source.stop().await;
    state.capture_running.store(false, Ordering::SeqCst);
    state.latest_frame.store(None);
    *state.current_patches.lock() = Vec::new();
    let _ = app.emit("patches-updated", Vec::<PatchPayload>::new());
    if let Some(window) = app.get_webview_window("render") {
        let _ = window.hide();
    }
    let _ = app.emit("capture-state-changed", false);
    if let Some(message) = failure {
        tracing::error!("Linux capture stopped: {message}");
        let _ = app.emit("capture-runtime-error", message);
    }
}

async fn wait_for_stop(stop_requested: Arc<AtomicBool>) {
    while !stop_requested.load(Ordering::Acquire) {
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
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
fn capture_screenshot_for_labeling(
    state: State<'_, Arc<AppState>>,
) -> Result<Option<PathBuf>, String> {
    capture_screenshot_inner(state.inner().as_ref())
}

fn capture_screenshot_inner(state: &AppState) -> Result<Option<PathBuf>, String> {
    let Some(frame) = state.latest_frame.load_full() else {
        return Ok(None);
    };
    paths::ensure_directories().map_err(|error| error.to_string())?;
    let stem = paths::new_screenshot_stem();
    let destination = paths::screenshots_dir().join(format!("{stem}.png"));
    let mut rgba = Vec::with_capacity(frame.pixels.len());
    for pixel in frame.pixels.chunks_exact(4) {
        rgba.extend_from_slice(&[pixel[2], pixel[1], pixel[0], pixel[3]]);
    }
    let image = image::ImageBuffer::<image::Rgba<u8>, _>::from_raw(frame.width, frame.height, rgba)
        .ok_or("invalid captured frame")?;
    let write_result = (|| -> Result<(), String> {
        use std::io::Write;
        let file = paths::create_private_file(&destination).map_err(|error| error.to_string())?;
        let mut writer = std::io::BufWriter::new(file);
        image::DynamicImage::ImageRgba8(image)
            .write_to(&mut writer, image::ImageFormat::Png)
            .map_err(|error| error.to_string())?;
        writer.flush().map_err(|error| error.to_string())?;
        writer.get_ref().sync_all().map_err(|error| error.to_string())
    })();
    if let Err(error) = write_result {
        let _ = std::fs::remove_file(&destination);
        return Err(error);
    }
    Ok(Some(destination))
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
            let p = paths::validate_screenshot_path(&e.path()).ok()?;
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

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ScreenshotData {
    width: u32,
    height: u32,
    png_data_url: String,
}

#[tauri::command]
fn load_screenshot(path: PathBuf) -> Result<ScreenshotData, String> {
    use base64::{engine::general_purpose::STANDARD, Engine as _};
    let path = paths::validate_screenshot_path(&path).map_err(|e| e.to_string())?;
    let bytes = std::fs::read(&path).map_err(|e| e.to_string())?;
    let image = image::load_from_memory(&bytes).map_err(|e| e.to_string())?;
    Ok(ScreenshotData {
        width: image.width(),
        height: image.height(),
        png_data_url: format!("data:image/png;base64,{}", STANDARD.encode(bytes)),
    })
}

#[tauri::command]
fn save_label(path: PathBuf, doc: LabelDocument) -> Result<(), String> {
    let path = paths::validate_label_path(&path, false).map_err(|e| e.to_string())?;
    validate_label_binding(&path, &doc)?;
    doc.save(&path).map_err(|e| e.to_string())
}

#[tauri::command]
fn load_label(path: PathBuf) -> Result<Option<LabelDocument>, String> {
    if !path.exists() {
        paths::validate_label_path(&path, false).map_err(|e| e.to_string())?;
        return Ok(None);
    }
    let path = paths::validate_label_path(&path, true).map_err(|e| e.to_string())?;
    LabelDocument::load(&path)
        .map(Some)
        .map_err(|e| e.to_string())
}

#[tauri::command]
fn discard_screenshot(path: PathBuf) -> Result<(), String> {
    let path = paths::validate_screenshot_path(&path).map_err(|e| e.to_string())?;
    let dst = paths::trash_dir().join(path.file_name().ok_or("no filename")?);
    liveblock_config::move_regular_file_no_replace(&path, &dst).map_err(|e| e.to_string())
}

fn validate_label_binding(path: &std::path::Path, doc: &LabelDocument) -> Result<(), String> {
    let label_stem = path.file_stem().and_then(|value| value.to_str()).ok_or("invalid label name")?;
    let image = std::path::Path::new(&doc.image);
    if image.parent().is_some_and(|parent| !parent.as_os_str().is_empty())
        || image.extension().and_then(|value| value.to_str()) != Some("png")
        || image.file_stem().and_then(|value| value.to_str()) != Some(label_stem)
    {
        return Err("label image must be the matching managed screenshot filename".into());
    }
    let screenshot = paths::screenshots_dir().join(&doc.image);
    paths::validate_screenshot_path(&screenshot).map_err(|error| error.to_string())?;
    Ok(())
}

async fn dispatch_hotkey(action: hotkeys::HotkeyAction, app: AppHandle, state: Arc<AppState>) {
    match action {
        hotkeys::HotkeyAction::ToggleCapture => {
            let has_runtime = state.capture.lock().await.is_some();
            if has_runtime {
                let _ = stop_capture_inner(app, state).await;
            } else {
                let target = match detect_session() {
                    session::SessionType::Wayland => Some("portal-selection"),
                    session::SessionType::X11 => Some("x11-root"),
                    session::SessionType::Unknown => None,
                };
                if let Some(target) = target {
                    if let Err(error) = start_capture_inner(target.into(), app.clone(), state).await {
                        let _ = app.emit("capture-runtime-error", error);
                    }
                }
            }
        }
        hotkeys::HotkeyAction::ToggleEditor => {
            if let Some(window) = app.get_webview_window("editor") {
                if window.is_visible().unwrap_or(false) {
                    let _ = window.hide();
                } else {
                    let _ = window.show();
                    let _ = window.set_focus();
                }
            }
        }
        hotkeys::HotkeyAction::CaptureForLabeling => {
            if capture_screenshot_inner(state.as_ref()).ok().flatten().is_some() {
                if let Some(window) = app.get_webview_window("labeling") {
                    let _ = window.show();
                    let _ = window.set_focus();
                }
            }
        }
        hotkeys::HotkeyAction::PanicDisable => {
            // Clear capture/overlay state before awaiting source teardown.
            let _ = stop_capture_inner(app.clone(), state).await;
            for label in ["editor", "render"] {
                if let Some(window) = app.get_webview_window(label) {
                    let _ = window.hide();
                }
            }
            if let Some(window) = app.get_webview_window("control") {
                let _ = window.show();
                let _ = window.set_focus();
            }
            let _ = app.emit("panic-disabled", ());
        }
    }
}

// ---------- Training pipeline ----------

#[tauri::command]
fn start_training(_epochs: u32, _batch: u32, _imgsz: u32) -> Result<(), String> {
    if !liveblock_config::developer_training_runtime_available() {
        return Err("release builds are inference-only; use the source training workflow".into());
    }
    Err("Linux source training dispatch is not implemented".into())
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

    tauri::Builder::default().manage(app_state)
        .setup(|app| {
            let detector = match model_updates::load_authenticated_active(app.handle()) {
                Ok(Some(detector)) => {
                    tracing::info!("loaded authenticated detector update");
                    Some(detector)
                }
                Ok(None) => match model_updates::load_authenticated_packaged(app.handle()) {
                    Ok(detector) => detector,
                    Err(error) => {
                        tracing::error!("authenticated packaged detector rejected: {error}");
                        None
                    }
                },
                Err(error) => {
                    tracing::error!("authenticated detector update rejected: {error}");
                    None
                }
            };
            if let (Some(detector), Some(state)) = (detector, app.try_state::<Arc<AppState>>()) {
                *state.detector.lock() = Some(detector);
            }

            let (hotkey_tx, hotkey_rx) = crossbeam_channel::unbounded();
            let hotkey_app = app.handle().clone();
            let hotkey_state = app.state::<Arc<AppState>>().inner().clone();
            hotkeys::spawn(hotkey_app.clone(), hotkey_tx, hotkey_state.clone());
            tauri::async_runtime::spawn(async move {
                loop {
                    match hotkey_rx.try_recv() {
                        Ok(action) => {
                            dispatch_hotkey(action, hotkey_app.clone(), hotkey_state.clone()).await;
                        }
                        Err(crossbeam_channel::TryRecvError::Empty) => {
                            tokio::time::sleep(Duration::from_millis(10)).await;
                        }
                        Err(crossbeam_channel::TryRecvError::Disconnected) => break,
                    }
                }
            });

            if let Err(e) = overlay::install_click_through(overlay::pick_strategy(), app.handle()) {
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
            get_capabilities,
            begin_user_action,
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
            show_window,
            hide_window,
            quit,
        ])
        .run(tauri::generate_context!())
        .expect("tauri::Builder run");
}
