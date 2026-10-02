//! LiveBlock — Windows entry. DPI-aware Tauri 2 app with a Rust hot path.

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod capture;
mod capture_policy;
mod detection;
mod hotkeys;
mod inpainting;
mod labels;
mod lifecycle;
mod model_updates;
mod overlay;
mod paths;
mod regions;
mod state;
mod tray;

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use parking_lot::{Mutex, MutexGuard};
use serde::Serialize;
use tauri::{AppHandle, Emitter, Listener, Manager, State};
use uuid::Uuid;

use crate::capture::{enumerate_monitors, CaptureSession, FrameView, MonitorDescriptor};
use crate::capture_policy::{
    action_is_newer_than_barriers, update_suspension_reasons, CaptureTelemetry,
    CaptureTelemetrySnapshot, ProtectedFrameDetector, SuspensionTransition,
    FIRST_FRAME_TIMEOUT_MS, RECOVERY_DELAYS_MS, SUSPENSION_POWER, SUSPENSION_SESSION_LOCK,
};
use crate::detection::DetBox;
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
        .setup(move |app| {
            let handle = app.handle().clone();
            let state = AppState::new(handle.clone(), regions.clone());
            app.manage(state);

            // Apply per-window native styles after Tauri has created HWNDs.
            let overlay_available = apply_window_styles(&handle);
            let runtime_state = handle.state::<AppState>();
            runtime_state
                .click_through_available
                .store(overlay_available, Ordering::SeqCst);
            runtime_state
                .capture_exclusion_available
                .store(overlay_available, Ordering::SeqCst);
            install_action_listeners(&handle);

            // Register tray + global hotkeys.
            tray::install(&handle).ok();
            hotkeys::spawn(handle.clone());
            lifecycle::spawn(handle.clone());
            spawn_monitor_geometry_watcher(handle.clone());

            // Try to load a default detector if the model is present.
            try_load_default_detector(&handle);

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            get_capabilities,
            get_behavior_contract,
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
            quit,
            show_window,
            hide_window,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

#[cfg(windows)]
fn apply_window_styles(app: &AppHandle) -> bool {
    use windows::Win32::Foundation::HWND;
    let mut render_available = false;
    if let Some(w) = app.get_webview_window("render") {
        if let Ok(h) = w.hwnd() {
            let hwnd = HWND(h.0 as *mut _);
            match overlay::make_render_overlay(hwnd) {
                Ok(()) => render_available = true,
                Err(e) => tracing::error!("make_render_overlay: {e}"),
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
    render_available
}

#[cfg(not(windows))]
fn apply_window_styles(_: &AppHandle) -> bool {
    false
}

fn align_windows_to_monitor(app: &AppHandle, monitor: &MonitorDescriptor) -> anyhow::Result<()> {
    use tauri::{PhysicalPosition, PhysicalSize, Position, Size};
    for label in ["render", "editor"] {
        if let Some(window) = app.get_webview_window(label) {
            window.set_position(Position::Physical(PhysicalPosition::new(monitor.x, monitor.y)))?;
            window.set_size(Size::Physical(PhysicalSize::new(monitor.width, monitor.height)))?;
        }
    }
    Ok(())
}

fn install_action_listeners(app: &AppHandle) {
    for event in ["hotkey-toggle-capture", "tray-toggle-capture"] {
        let handle = app.clone();
        app.listen(event, move |event| {
            let sequence = serde_json::from_str(event.payload()).unwrap_or(0);
            toggle_capture_action(&handle, sequence);
        });
    }

    let handle = app.clone();
    app.listen("hotkey-availability-changed", move |event| {
        let available = serde_json::from_str(event.payload()).unwrap_or(false);
        handle
            .state::<AppState>()
            .global_hotkeys_available
            .store(available, Ordering::SeqCst);
        let _ = handle.emit("capabilities-changed", ());
    });

    let handle = app.clone();
    app.listen("hotkey-toggle-editor", move |event| {
        let sequence = serde_json::from_str(event.payload()).unwrap_or(0);
        if !action_sequence_is_allowed(&handle, sequence) {
            return;
        }
        if let Some(window) = handle.get_webview_window("editor") {
            if window.is_visible().unwrap_or(false) {
                let _ = window.hide();
            } else {
                let _ = window.show();
                let _ = window.set_focus();
            }
            if !action_sequence_is_allowed(&handle, sequence) {
                let _ = window.hide();
            }
        }
    });

    let handle = app.clone();
    app.listen("hotkey-capture-screenshot", move |event| {
        let sequence = serde_json::from_str(event.payload()).unwrap_or(0);
        let state = handle.state::<AppState>();
        if !action_sequence_is_allowed(&handle, sequence) {
            return;
        }
        if let Ok(Some(path)) = capture_screenshot_for_labeling_inner(state.inner()) {
            if action_sequence_is_allowed(&handle, sequence) {
                if let Some(window) = handle.get_webview_window("labeling") {
                    let _ = window.show();
                    let _ = window.set_focus();
                    if !action_sequence_is_allowed(&handle, sequence) {
                        let _ = window.hide();
                    }
                }
            } else {
                let _ = std::fs::remove_file(path);
            }
        }
    });

    let handle = app.clone();
    app.listen("hotkey-panic-disable", move |event| {
        let sequence = serde_json::from_str(event.payload()).unwrap_or(u64::MAX);
        panic_disable_action(&handle, sequence);
    });

    let handle = app.clone();
    app.listen("tray-quit-requested", move |_| {
        quit_action(&handle);
    });

    let handle = app.clone();
    app.listen("windows-lifecycle-availability-changed", move |event| {
        let available = serde_json::from_str(event.payload()).unwrap_or(false);
        let state = handle.state::<AppState>();
        state
            .lifecycle_observer_available
            .store(available, Ordering::SeqCst);
        if !available {
            cancel_capture_intent(state.inner());
            let _ = handle.emit(
                "capture-runtime-error",
                "Windows power/session lifecycle observation is unavailable",
            );
        }
        let _ = handle.emit("capabilities-changed", ());
    });

    let handle = app.clone();
    app.listen("windows-power-suspension-changed", move |event| {
        let active = serde_json::from_str(event.payload()).unwrap_or(true);
        update_windows_suspension(&handle, SUSPENSION_POWER, active);
    });

    let handle = app.clone();
    app.listen("windows-session-lock-changed", move |event| {
        let active = serde_json::from_str(event.payload()).unwrap_or(true);
        update_windows_suspension(&handle, SUSPENSION_SESSION_LOCK, active);
    });

    let handle = app.clone();
    app.listen("windows-display-topology-changed", move |_| {
        let state = handle.state::<AppState>();
        if state.capture_desired.load(Ordering::SeqCst)
            && state.suspension_reasons.load(Ordering::SeqCst) == 0
            && state.capture.lock().is_none()
        {
            schedule_capture_recovery(handle.clone());
        }
    });
}

fn update_windows_suspension(app: &AppHandle, reason: u8, active: bool) {
    let state = app.state::<AppState>();
    let transition = loop {
        let current = state.suspension_reasons.load(Ordering::SeqCst);
        let (next, transition) = update_suspension_reasons(current, reason, active);
        match state.suspension_reasons.compare_exchange(
            current,
            next,
            Ordering::SeqCst,
            Ordering::SeqCst,
        ) {
            Ok(_) => break transition,
            Err(_) => continue,
        }
    };
    match transition {
        SuspensionTransition::BecameSuspended => {
            state.recovery_generation.fetch_add(1, Ordering::SeqCst);
            let _ = stop_capture_inner(state.inner());
            let _ = app.emit("capture-suspended-changed", true);
        }
        SuspensionTransition::BecameResumable => {
            let _ = app.emit("capture-suspended-changed", false);
            if state.capture_desired.load(Ordering::SeqCst) {
                schedule_capture_recovery(app.clone());
            }
        }
        SuspensionTransition::Unchanged => {}
    }
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct RecoveryAttemptEvent {
    attempt: usize,
    delay_ms: u64,
}

fn schedule_capture_recovery(app: AppHandle) {
    let state = app.state::<AppState>();
    if !state.capture_desired.load(Ordering::SeqCst)
        || state.suspension_reasons.load(Ordering::SeqCst) != 0
    {
        return;
    }
    let token = state
        .recovery_generation
        .fetch_add(1, Ordering::SeqCst)
        .wrapping_add(1);
    std::thread::spawn(move || {
        for (index, delay_ms) in RECOVERY_DELAYS_MS.into_iter().enumerate() {
            std::thread::sleep(Duration::from_millis(delay_ms));
            let state = app.state::<AppState>();
            if state.recovery_generation.load(Ordering::SeqCst) != token
                || !state.capture_desired.load(Ordering::SeqCst)
                || state.suspension_reasons.load(Ordering::SeqCst) != 0
            {
                return;
            }
            if state.capture.lock().is_some() {
                return;
            }
            let _ = app.emit(
                "capture-recovery-attempt",
                RecoveryAttemptEvent {
                    attempt: index + 1,
                    delay_ms,
                },
            );
            let selected_handle = state.selected_monitor.load(Ordering::SeqCst);
            let selected_name = state.selected_monitor_name.lock().clone();
            let monitors = enumerate_monitors();
            let selected = if let Some(name) = selected_name {
                monitors.into_iter().find(|monitor| monitor.name == name)
            } else {
                monitors
                    .into_iter()
                    .find(|monitor| monitor.handle.0 as usize == selected_handle)
            };
            if let Some(selected) = selected {
                if start_capture_inner(
                    (selected.handle.0 as usize).to_string(),
                    state.inner(),
                    None,
                    Some(token),
                )
                .is_ok()
                    && state.capture.lock().is_some()
                {
                    // Active state is emitted only by the first-frame gate. A
                    // recovery attempt does not succeed merely because WGC
                    // created a session; wait for a packed valid frame.
                    for _ in 0..(FIRST_FRAME_TIMEOUT_MS / 10 + 20) {
                        if state.recovery_generation.load(Ordering::SeqCst) != token
                            || !state.capture_desired.load(Ordering::SeqCst)
                        {
                            return;
                        }
                        let readiness = state
                            .capture
                            .lock()
                            .as_ref()
                            .map(CaptureSession::has_first_frame);
                        match readiness {
                            Some(true) => return,
                            Some(false) => std::thread::sleep(Duration::from_millis(10)),
                            None => break,
                        }
                    }
                }
            }
        }
        let state = app.state::<AppState>();
        let lifecycle_guard = state.capture_lifecycle.lock();
        let exhausted = state.recovery_generation.load(Ordering::SeqCst) == token
            && state.capture_desired.load(Ordering::SeqCst)
            && state.capture.lock().is_none();
        if exhausted {
            state.capture_desired.store(false, Ordering::SeqCst);
        }
        drop(lifecycle_guard);
        if exhausted {
            let _ = app.emit(
                "capture-runtime-error",
                "automatic capture recovery exhausted after four attempts",
            );
        }
    });
}

fn action_sequence_is_allowed(app: &AppHandle, action_sequence: u64) -> bool {
    let state = app.state::<AppState>();
    action_is_newer_than_barriers(
        action_sequence,
        state.last_panic_action.load(Ordering::SeqCst),
        state.last_stop_action.load(Ordering::SeqCst),
        state.shutting_down.load(Ordering::SeqCst),
    )
}

fn toggle_capture_action(app: &AppHandle, action_sequence: u64) {
    let state = app.state::<AppState>();
    if !action_sequence_is_allowed(app, action_sequence) {
        return;
    }
    let running_or_pending = state.capture.lock().is_some()
        || state.capture_desired.load(Ordering::SeqCst);
    if running_or_pending {
        state
            .last_stop_action
            .fetch_max(action_sequence, Ordering::SeqCst);
        cancel_capture_intent(state.inner());
        return;
    }
    let selected = state.selected_monitor.load(Ordering::SeqCst);
    let monitor = enumerate_monitors()
        .into_iter()
        .find(|monitor| monitor.handle.0 as usize == selected)
        .or_else(|| enumerate_monitors().into_iter().find(|monitor| monitor.is_primary));
    if let Some(monitor) = monitor {
        if let Err(error) = start_capture_inner(
            (monitor.handle.0 as usize).to_string(),
            state.inner(),
            Some(action_sequence),
            None,
        ) {
            let _ = app.emit("capture-runtime-error", &error);
            if state.capture_desired.load(Ordering::SeqCst) {
                schedule_capture_recovery(app.clone());
            }
        }
    } else {
        let _ = app.emit("capture-runtime-error", "no Windows monitor is available");
    }
}

fn persistent_mutation_guard(state: &AppState) -> Result<MutexGuard<'_, ()>, String> {
    let guard = state.persistent_mutation.lock();
    if state.shutting_down.load(Ordering::SeqCst) {
        Err("persistent changes are unavailable during shutdown".into())
    } else {
        Ok(guard)
    }
}

fn cancel_detector_run(state: &AppState) {
    // Never wait for the detector mutex during panic/stop. RunOptions is held
    // separately so ORT can be asked to terminate an in-flight DirectML/CPU run.
    if let Some(run_slot) = state.detector_run.try_lock() {
        if let Some(run_options) = run_slot.as_ref() {
            let _ = run_options.terminate();
        }
    }
}

fn quit_action(app: &AppHandle) {
    let state = app.state::<AppState>();
    if state.shutting_down.swap(true, Ordering::SeqCst) {
        return;
    }
    state.capture_desired.store(false, Ordering::SeqCst);
    state.recovery_generation.fetch_add(1, Ordering::SeqCst);
    state.recovery_cycles.store(0, Ordering::SeqCst);
    // Privacy-visible windows hide before native capture or an authenticated
    // model transaction can delay terminal teardown.
    for label in ["editor", "render", "labeling", "training"] {
        if let Some(window) = app.get_webview_window(label) {
            let _ = window.hide();
        }
    }
    let _ = stop_capture_inner(state.inner());
    // Finish an already-admitted region/label/screenshot write before exit;
    // commands queued after the terminal flag fail when they acquire this gate.
    let _mutation_guard = state.persistent_mutation.lock();
    // A verified update already holding this lock is crash-recoverable, but a
    // normal quit waits for its disk transaction instead of exiting mid-swap.
    let _update_guard = state.model_update.lock();
    app.exit(0);
}

fn panic_disable_action(app: &AppHandle, action_sequence: u64) {
    let state = app.state::<AppState>();
    cancel_detector_run(state.inner());
    state
        .last_panic_action
        .fetch_max(action_sequence, Ordering::SeqCst);
    state.capture_desired.store(false, Ordering::SeqCst);
    state.recovery_generation.fetch_add(1, Ordering::SeqCst);
    // Visible privacy state clears before potentially contended native teardown.
    let _ = app.emit("capture-state-changed", false);
    let _ = app.emit("protected-content-changed", false);
    let _ = app.emit("patches-updated", Vec::<PatchPayload>::new());
    for label in ["editor", "render", "labeling", "training"] {
        if let Some(window) = app.get_webview_window(label) {
            let _ = window.hide();
        }
    }
    let _ = stop_capture_inner(state.inner());
    if let Some(window) = app.get_webview_window("control") {
        let _ = window.show();
        let _ = window.set_focus();
    }
    let _ = app.emit("panic-disabled", ());
}

fn spawn_first_frame_gate(
    app: AppHandle,
    generation: u64,
    first_frame_seen: Arc<AtomicBool>,
    failure_reported: Arc<AtomicBool>,
    startup_gate: Arc<Mutex<()>>,
    is_recovery: bool,
) {
    std::thread::spawn(move || {
        let polls = FIRST_FRAME_TIMEOUT_MS / 10;
        for _ in 0..polls {
            if first_frame_seen.load(Ordering::Acquire) {
                let _failure_guard = startup_gate.lock();
                let state = app.state::<AppState>();
                let _lifecycle_guard = state.capture_lifecycle.lock();
                if !failure_reported.load(Ordering::SeqCst)
                    && state.capture_generation.load(Ordering::SeqCst) == generation
                    && state.capture_desired.load(Ordering::SeqCst)
                    && state.suspension_reasons.load(Ordering::SeqCst) == 0
                    && state.capture.lock().is_some()
                {
                    if let Some(window) = app.get_webview_window("render") {
                        let _ = window.show();
                    }
                    let _ = app.emit("protected-content-changed", false);
                    let _ = app.emit("capture-state-changed", true);
                    if is_recovery {
                        let _ = app.emit("capture-recovery-succeeded", ());
                        spawn_recovery_stability_reset(app.clone(), generation);
                    }
                }
                return;
            }
            std::thread::sleep(Duration::from_millis(10));
        }

        let _failure_guard = startup_gate.lock();
        let state = app.state::<AppState>();
        let lifecycle_guard = state.capture_lifecycle.lock();
        if failure_reported.load(Ordering::SeqCst)
            || state.capture_generation.load(Ordering::SeqCst) != generation
        {
            return;
        }
        state.capture_generation.fetch_add(1, Ordering::SeqCst);
        cancel_detector_run(state.inner());
        if let Some(session) = state.capture.lock().take() {
            session.stop();
        }
        let should_recover = state.capture_desired.load(Ordering::SeqCst)
            && state.suspension_reasons.load(Ordering::SeqCst) == 0;
        drop(lifecycle_guard);
        let _ = app.emit("capture-state-changed", false);
        let _ = app.emit(
            "capture-runtime-error",
            "capture started but produced no valid frame within 3 seconds",
        );
        if should_recover && !is_recovery {
            schedule_capture_recovery(app);
        }
    });
}

fn spawn_recovery_stability_reset(app: AppHandle, generation: u64) {
    std::thread::spawn(move || {
        std::thread::sleep(Duration::from_secs(10));
        let state = app.state::<AppState>();
        let _lifecycle_guard = state.capture_lifecycle.lock();
        if state.capture_generation.load(Ordering::SeqCst) == generation
            && state.capture_desired.load(Ordering::SeqCst)
            && state.capture.lock().is_some()
        {
            state.recovery_cycles.store(0, Ordering::SeqCst);
        }
    });
}

fn spawn_monitor_geometry_watcher(app: AppHandle) {
    std::thread::spawn(move || loop {
        std::thread::sleep(Duration::from_secs(1));
        let state = app.state::<AppState>();
        let lifecycle_guard = state.capture_lifecycle.lock();
        if state.capture.lock().is_none() {
            continue;
        }
        let selected = state.selected_monitor.load(Ordering::SeqCst);
        let stop_reason = if let Some(monitor) = enumerate_monitors()
            .into_iter()
            .find(|monitor| monitor.handle.0 as usize == selected)
        {
            match align_windows_to_monitor(&app, &monitor) {
                Ok(()) => continue,
                Err(error) => {
                    tracing::error!("monitor geometry refresh failed: {error}");
                    "selected monitor geometry could not be refreshed; capture stopped"
                }
            }
        } else {
            "selected monitor was removed; capture stopped"
        };

        state.capture_generation.fetch_add(1, Ordering::SeqCst);
        cancel_detector_run(state.inner());
        if let Some(session) = state.capture.lock().take() {
            session.stop();
        }
        let unstable_exhausted = state
            .recovery_cycles
            .fetch_add(1, Ordering::SeqCst)
            .saturating_add(1)
            >= 4;
        if unstable_exhausted {
            state.capture_desired.store(false, Ordering::SeqCst);
            state.recovery_generation.fetch_add(1, Ordering::SeqCst);
        }
        drop(lifecycle_guard);
        if let Some(window) = app.get_webview_window("render") {
            let _ = window.hide();
        }
        let _ = app.emit("capture-state-changed", false);
        let _ = app.emit("protected-content-changed", false);
        let _ = app.emit("patches-updated", Vec::<PatchPayload>::new());
        let _ = app.emit(
            "capture-runtime-error",
            if unstable_exhausted {
                "capture remained unstable after four display recovery cycles"
            } else {
                stop_reason
            },
        );
        if !unstable_exhausted
            && state.capture_desired.load(Ordering::SeqCst)
            && state.suspension_reasons.load(Ordering::SeqCst) == 0
        {
            schedule_capture_recovery(app.clone());
        }
    });
}

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
fn get_behavior_contract() -> Result<liveblock_config::DesktopBehaviorContract, String> {
    let contract = liveblock_config::DesktopBehaviorContract::default();
    contract.validate().map_err(|error| error.to_owned())?;
    Ok(contract)
}

#[tauri::command]
fn get_capabilities(
    state: State<'_, AppState>,
) -> Result<liveblock_config::DesktopCapabilityProfile, String> {
    let mut profile = liveblock_config::DesktopCapabilityProfile::windows();
    profile.click_through_overlay = state.click_through_available.load(Ordering::SeqCst);
    profile.capture_exclusion = state.capture_exclusion_available.load(Ordering::SeqCst);
    profile.global_hotkeys = state.global_hotkeys_available.load(Ordering::SeqCst);
    if !state.lifecycle_observer_available.load(Ordering::SeqCst) {
        profile.limitations.push(
            "power/session lifecycle observation is unavailable; capture is disabled".into(),
        );
    }
    profile.limitations.push(format!(
        "inpainting backend: {}; CPU-uploaded D3D11 patches are read back for webview composition",
        state.inpainter.lock().backend_status()
    ));
    let detector_backend = state
        .detector
        .lock()
        .as_ref()
        .map(|detector| detector.backend_status());
    profile.limitations.push(match detector_backend {
        Some(backend) => format!(
            "detector backend: {backend}; registration/load status is not proof of physical GPU graph execution"
        ),
        None => "detector backend: no authenticated model loaded; DirectML execution is unobserved".into(),
    });
    profile.limitations.push(
        "DirectML texture transport: blocked (0/8 readiness gates); capture uses a plain unshared D3D11 texture and ORT receives CPU-uploaded NCHW"
            .into(),
    );
    profile.validate().map_err(str::to_string)?;
    Ok(profile)
}

#[tauri::command]
fn list_monitors() -> Vec<MonitorInfo> {
    enumerate_monitors()
        .into_iter()
        .map(|monitor| MonitorInfo {
            id: (monitor.handle.0 as usize).to_string(),
            name: monitor.name,
            is_primary: monitor.is_primary,
            x: monitor.x,
            y: monitor.y,
            width: monitor.width,
            height: monitor.height,
            scale_factor: monitor.scale_factor,
        })
        .collect()
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct MonitorInfo {
    id: String,
    name: String,
    is_primary: bool,
    x: i32,
    y: i32,
    width: u32,
    height: u32,
    scale_factor: f64,
}

#[tauri::command]
fn begin_user_action() -> u64 {
    hotkeys::next_action_sequence()
}

#[tauri::command]
fn start_capture(
    monitor_id: String,
    action_sequence: u64,
    state: State<'_, AppState>,
) -> Result<(), String> {
    let result = start_capture_inner(
        monitor_id,
        state.inner(),
        Some(action_sequence),
        None,
    );
    if result.is_err() && state.capture_desired.load(Ordering::SeqCst) {
        schedule_capture_recovery(state.app.clone());
    }
    result
}

fn start_capture_inner(
    monitor_id: String,
    state: &AppState,
    action_sequence: Option<u64>,
    recovery_token: Option<u64>,
) -> Result<(), String> {
    let monitor_hmonitor = monitor_id
        .parse::<usize>()
        .map_err(|_| "invalid monitor id".to_string())?;

    // Selection revalidation, panic ordering, window movement, teardown, start,
    // and publication share one lock, so concurrent transitions cannot split
    // geometry or resurrect a pre-panic action.
    let _lifecycle_guard = state.capture_lifecycle.lock();
    if state.shutting_down.load(Ordering::SeqCst) {
        return Ok(());
    }
    if let Some(sequence) = action_sequence {
        if !action_is_newer_than_barriers(
            sequence,
            state.last_panic_action.load(Ordering::SeqCst),
            state.last_stop_action.load(Ordering::SeqCst),
            state.shutting_down.load(Ordering::SeqCst),
        ) {
            return Ok(());
        }
    }
    if let Some(token) = recovery_token {
        if state.recovery_generation.load(Ordering::SeqCst) != token
            || !state.capture_desired.load(Ordering::SeqCst)
        {
            return Ok(());
        }
    }
    if !state.lifecycle_observer_available.load(Ordering::SeqCst) {
        return Err("Windows power/session lifecycle observation is unavailable".into());
    }
    let selected = enumerate_monitors()
        .into_iter()
        .find(|monitor| monitor.handle.0 as usize == monitor_hmonitor)
        .ok_or_else(|| "selected monitor is no longer available".to_string())?;
    let intent_token = if let Some(token) = recovery_token {
        token
    } else {
        let token = state
            .recovery_generation
            .fetch_add(1, Ordering::SeqCst)
            .wrapping_add(1);
        state.recovery_cycles.store(0, Ordering::SeqCst);
        state.capture_desired.store(true, Ordering::SeqCst);
        token
    };
    state
        .selected_monitor
        .store(monitor_hmonitor, Ordering::SeqCst);
    *state.selected_monitor_name.lock() = Some(selected.name.clone());
    if state.suspension_reasons.load(Ordering::SeqCst) != 0 {
        return Ok(());
    }
    let generation = state
        .capture_generation
        .fetch_add(1, Ordering::SeqCst)
        .wrapping_add(1);
    cancel_detector_run(state);
    let existing = { state.capture.lock().take() };
    if let Some(existing) = existing {
        existing.stop();
        let _ = state.app.emit("capture-state-changed", false);
        let _ = state.app.emit("protected-content-changed", false);
        let _ = state.app.emit("patches-updated", Vec::<PatchPayload>::new());
    }
    if let Err(error) = align_windows_to_monitor(&state.app, &selected) {
        if let Some(window) = state.app.get_webview_window("render") {
            let _ = window.hide();
        }
        return Err(error.to_string());
    }
    let app = state.app.clone();
    let regions_arc = state.regions.clone();
    let inpainter_arc = state.inpainter.clone();
    let detector_arc = state.detector.clone();
    let detector_run = state.detector_run.clone();
    let detection_on = state.detection_enabled.clone();
    let last_detection: Arc<Mutex<(Instant, Vec<DetBox>)>> =
        Arc::new(Mutex::new((Instant::now() - Duration::from_secs(1), Vec::new())));
    let protected_detector = Arc::new(Mutex::new(ProtectedFrameDetector::default()));
    let telemetry = Arc::new(CaptureTelemetry::default());
    let telemetry_for_frame = telemetry.clone();
    let frame_generation = state.capture_generation.clone();

    let frame_counter = AtomicU64::new(0);
    let on_frame = Arc::new(move |frame: &FrameView| {
        if frame_generation.load(Ordering::SeqCst) != generation {
            return;
        }
        let frame_number = frame_counter.fetch_add(1, Ordering::Relaxed).wrapping_add(1);

        let mut protected = protected_detector.lock();
        if let Some(value) = protected.observe_bgra(&frame.bytes, frame.width, frame.height) {
            telemetry_for_frame.set_protected(value);
            if value {
                // Remove stale patches before publishing the uncertain
                // protected/unavailable state, even if the render window is gone.
                last_detection.lock().1.clear();
                let _ = app.emit("patches-updated", Vec::<PatchPayload>::new());
            }
            if let Some(window) = app.get_webview_window("render") {
                if value {
                    // Hide the entire overlay surface; transparency alone must not
                    // be relied upon to suppress stale or recursively captured UI.
                    let _ = window.hide();
                } else {
                    // This callback is generation-guarded, so only the active
                    // capture can restore the renderer after visible frames return.
                    let _ = window.show();
                }
            }
            let _ = app.emit("protected-content-changed", value);
        }
        if protected.is_protected() {
            telemetry_for_frame.protected_frame();
            drop(protected);
            return;
        }
        drop(protected);

        // Detection every fourth processed frame. CaptureSession's capacity-one
        // queue keeps this CPU/ORT work off the WGC callback and drops stale work.
        if detection_on.load(Ordering::Relaxed) && frame_number % 4 == 0 {
            let mut detector = detector_arc.lock();
            if let (Some(det), Ok(options)) = (detector.as_mut(), ort::RunOptions::new()) {
                let options = Arc::new(options);
                *detector_run.lock() = Some(options.clone());
                if frame_generation.load(Ordering::SeqCst) != generation {
                    let _ = options.terminate();
                } else if let Ok(boxes) =
                    det.detect(&frame.bytes, frame.width, frame.height, &options)
                {
                    *last_detection.lock() = (Instant::now(), boxes);
                }
                let mut active_run = detector_run.lock();
                if active_run
                    .as_ref()
                    .is_some_and(|current| Arc::ptr_eq(current, &options))
                {
                    *active_run = None;
                }
            }
        }
        if frame_generation.load(Ordering::SeqCst) != generation {
            return;
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
        if frame_generation.load(Ordering::SeqCst) != generation {
            return;
        }

        // Inpaint.
        let payloads: Vec<PatchPayload> = inpainter_arc
            .lock()
            .render_frame(frame, &regions)
            .unwrap_or_default();

        if frame_generation.load(Ordering::SeqCst) == generation {
            let _ = app.emit("patches-updated", &payloads);
        }
    }) as Arc<dyn Fn(&FrameView) + Send + Sync>;

    let error_app = state.app.clone();
    let capture_slot = state.capture.clone();
    let capture_lifecycle = state.capture_lifecycle.clone();
    let active_generation = state.capture_generation.clone();
    let failure_reported = Arc::new(AtomicBool::new(false));
    let is_recovery_start = recovery_token.is_some();
    let failure_for_callback = failure_reported.clone();
    let startup_gate = Arc::new(Mutex::new(()));
    let startup_gate_for_callback = startup_gate.clone();
    let on_error = Arc::new(move |message: String| {
        // Serialize first-failure publication with startup's final check. If
        // failure wins, startup never publishes active; otherwise active is
        // published first and this callback's stopped event is ordered after it.
        let _startup_guard = startup_gate_for_callback.lock();
        if active_generation.load(Ordering::SeqCst) != generation
            || failure_for_callback.swap(true, Ordering::SeqCst)
        { return; }
        tracing::error!("capture runtime failure: {message}");
        let _ = error_app.emit("capture-runtime-error", &message);
        let _ = error_app.emit("capture-state-changed", false);
        let _ = error_app.emit("protected-content-changed", false);
        let _ = error_app.emit("patches-updated", Vec::<PatchPayload>::new());
        if let Some(window) = error_app.get_webview_window("render") {
            let _ = window.hide();
        }

        // Never tear down from a WGC callback or the frame worker itself. A
        // short-lived cleanup thread waits for start_capture to publish this
        // generation, then owns deterministic handler/session closure. The
        // CPU worker is cancellation-signaled and may finish current work.
        let slot = capture_slot.clone();
        let lifecycle = capture_lifecycle.clone();
        let generation_counter = active_generation.clone();
        let cleanup_app = error_app.clone();
        std::thread::spawn(move || {
            for _ in 0..50 {
                let lifecycle_guard = lifecycle.lock();
                let failed = {
                    let mut guard = slot.lock();
                    if generation_counter.load(Ordering::SeqCst) != generation { return; }
                    guard.take()
                };
                if let Some(session) = failed {
                    generation_counter.fetch_add(1, Ordering::SeqCst);
                    let recovery_state = cleanup_app.state::<AppState>();
                    cancel_detector_run(recovery_state.inner());
                    let had_valid_frame = session.has_first_frame();
                    session.stop();
                    let _ = cleanup_app.emit("capture-state-changed", false);
                    let _ = cleanup_app.emit("protected-content-changed", false);
                    let _ = cleanup_app.emit("patches-updated", Vec::<PatchPayload>::new());
                    if let Some(window) = cleanup_app.get_webview_window("render") {
                        let _ = window.hide();
                    }
                    let unstable_exhausted = if is_recovery_start && had_valid_frame {
                        recovery_state
                            .recovery_cycles
                            .fetch_add(1, Ordering::SeqCst)
                            .saturating_add(1)
                            >= 4
                    } else {
                        if !is_recovery_start {
                            recovery_state.recovery_cycles.store(0, Ordering::SeqCst);
                        }
                        false
                    };
                    if unstable_exhausted {
                        recovery_state.capture_desired.store(false, Ordering::SeqCst);
                        recovery_state.recovery_generation.fetch_add(1, Ordering::SeqCst);
                    }
                    let should_schedule = !unstable_exhausted
                        && (!is_recovery_start || had_valid_frame)
                        && recovery_state.capture_desired.load(Ordering::SeqCst)
                        && recovery_state.suspension_reasons.load(Ordering::SeqCst) == 0;
                    drop(lifecycle_guard);
                    if unstable_exhausted {
                        let _ = cleanup_app.emit(
                            "capture-runtime-error",
                            "capture remained unstable after four recovery cycles",
                        );
                    } else if should_schedule {
                        schedule_capture_recovery(cleanup_app.clone());
                    }
                    return;
                }
                drop(lifecycle_guard);
                std::thread::sleep(Duration::from_millis(10));
            }
        });
    });
    let session = match CaptureSession::start(selected.handle, on_frame, on_error, telemetry) {
        Ok(session) => session,
        Err(error) => {
            if let Some(window) = state.app.get_webview_window("render") {
                let _ = window.hide();
            }
            let _ = state.app.emit("capture-state-changed", false);
            return Err(error.to_string());
        }
    };
    let _startup_guard = startup_gate.lock();
    let action_still_allowed = action_sequence.is_none_or(|sequence| {
        action_is_newer_than_barriers(
            sequence,
            state.last_panic_action.load(Ordering::SeqCst),
            state.last_stop_action.load(Ordering::SeqCst),
            state.shutting_down.load(Ordering::SeqCst),
        )
    });
    if state.capture_generation.load(Ordering::SeqCst) != generation
        || state.recovery_generation.load(Ordering::SeqCst) != intent_token
        || !state.capture_desired.load(Ordering::SeqCst)
        || state.suspension_reasons.load(Ordering::SeqCst) != 0
        || !action_still_allowed
        || failure_reported.load(Ordering::SeqCst)
    {
        session.stop();
        let _ = state.app.emit("capture-state-changed", false);
        return Err("capture failed during startup".into());
    }
    let first_frame_seen = session.first_frame_signal();
    *state.capture.lock() = Some(session);
    let _ = state.app.emit("capture-state-changed", false);
    spawn_first_frame_gate(
        state.app.clone(),
        generation,
        first_frame_seen,
        failure_reported.clone(),
        startup_gate.clone(),
        recovery_token.is_some(),
    );
    Ok(())
}

#[tauri::command]
fn stop_capture(action_sequence: u64, state: State<'_, AppState>) -> Result<(), String> {
    state
        .last_stop_action
        .fetch_max(action_sequence, Ordering::SeqCst);
    cancel_capture_intent(state.inner());
    Ok(())
}

fn cancel_capture_intent(state: &AppState) {
    // Commit cancellation before waiting on potentially slow native startup,
    // then repeat it under the lifecycle lock so an older start cannot restore
    // intent while stop is queued.
    state.capture_desired.store(false, Ordering::SeqCst);
    state.recovery_generation.fetch_add(1, Ordering::SeqCst);
    state.recovery_cycles.store(0, Ordering::SeqCst);
    let _lifecycle_guard = state.capture_lifecycle.lock();
    state.capture_desired.store(false, Ordering::SeqCst);
    state.recovery_generation.fetch_add(1, Ordering::SeqCst);
    stop_capture_locked(state);
}

fn stop_capture_inner(state: &AppState) -> Result<(), String> {
    let _lifecycle_guard = state.capture_lifecycle.lock();
    stop_capture_locked(state);
    Ok(())
}

fn stop_capture_locked(state: &AppState) {
    // Invalidate first. If a frame publishes a new RunOptions after the
    // nonblocking cancellation attempt, its generation recheck self-terminates.
    state.capture_generation.fetch_add(1, Ordering::SeqCst);
    cancel_detector_run(state);
    let session = { state.capture.lock().take() };
    if let Some(session) = session {
        session.stop();
    }
    let _ = state.app.emit("capture-state-changed", false);
    let _ = state.app.emit("protected-content-changed", false);
    let _ = state.app.emit("patches-updated", Vec::<PatchPayload>::new());
    if let Some(window) = state.app.get_webview_window("render") {
        let _ = window.hide();
    }
}

#[tauri::command]
fn get_capture_telemetry(state: State<'_, AppState>) -> CaptureTelemetrySnapshot {
    state.capture.lock().as_ref().map(CaptureSession::telemetry).unwrap_or_default()
}

#[tauri::command]
fn set_detection_enabled(enabled: bool, state: State<'_, AppState>) -> Result<(), String> {
    if state.shutting_down.load(Ordering::SeqCst) {
        return Err("runtime changes are unavailable during shutdown".into());
    }
    state.detection_enabled.store(enabled, Ordering::Relaxed);
    Ok(())
}

#[tauri::command]
fn list_regions(state: State<'_, AppState>) -> Vec<NormalizedRegion> {
    state.regions.current()
}

#[tauri::command]
fn add_region(region: NormalizedRegion, state: State<'_, AppState>) -> Result<NormalizedRegion, String> {
    let _mutation_guard = persistent_mutation_guard(state.inner())?;
    state.regions.add(region.clone()).map_err(|e| e.to_string())?;
    let _ = state.app.emit("regions-updated", state.regions.current());
    Ok(region)
}

#[tauri::command]
fn replace_region(id: Uuid, region: NormalizedRegion, state: State<'_, AppState>) -> Result<(), String> {
    let _mutation_guard = persistent_mutation_guard(state.inner())?;
    state.regions.replace_id(id, region).map_err(|e| e.to_string())?;
    let _ = state.app.emit("regions-updated", state.regions.current());
    Ok(())
}

#[tauri::command]
fn delete_region(id: Uuid, state: State<'_, AppState>) -> Result<(), String> {
    let _mutation_guard = persistent_mutation_guard(state.inner())?;
    state.regions.remove(id).map_err(|e| e.to_string())?;
    let _ = state.app.emit("regions-updated", state.regions.current());
    Ok(())
}

#[tauri::command]
fn clear_regions(state: State<'_, AppState>) -> Result<(), String> {
    let _mutation_guard = persistent_mutation_guard(state.inner())?;
    state.regions.clear().map_err(|e| e.to_string())?;
    let _ = state.app.emit("regions-updated", state.regions.current());
    Ok(())
}

#[tauri::command]
fn capture_screenshot_for_labeling(state: State<'_, AppState>) -> Result<Option<PathBuf>, String> {
    capture_screenshot_for_labeling_inner(state.inner())
}

fn capture_screenshot_for_labeling_inner(state: &AppState) -> Result<Option<PathBuf>, String> {
    let _mutation_guard = persistent_mutation_guard(state)?;
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
    let image = image::ImageBuffer::<image::Rgba<u8>, _>::from_raw(frame.width, frame.height, rgba)
        .ok_or("invalid frame")?;
    let write_result = (|| -> Result<(), String> {
        use std::io::Write;
        let file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&dst)
            .map_err(|error| error.to_string())?;
        let mut writer = std::io::BufWriter::new(file);
        image::DynamicImage::ImageRgba8(image)
            .write_to(&mut writer, image::ImageFormat::Png)
            .map_err(|error| error.to_string())?;
        writer.flush().map_err(|error| error.to_string())?;
        writer.get_ref().sync_all().map_err(|error| error.to_string())
    })();
    if let Err(error) = write_result {
        let _ = std::fs::remove_file(&dst);
        return Err(error);
    }
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
            let p = paths::validate_screenshot_path(&e.path()).ok()?;
            let stem = p.file_stem()?.to_string_lossy().into_owned();
            let label = paths::label_path_for(&p);
            Some(ScreenshotEntry {
                path: p,
                label_path: label.clone(),
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
    /// data:image/png;base64,...
    png_data_url: String,
}

#[tauri::command]
fn load_screenshot(path: PathBuf) -> Result<ScreenshotData, String> {
    use base64::{engine::general_purpose::STANDARD, Engine as _};
    let path = paths::validate_screenshot_path(&path).map_err(|e| e.to_string())?;
    let bytes = std::fs::read(&path).map_err(|e| e.to_string())?;
    let img = image::load_from_memory(&bytes).map_err(|e| e.to_string())?;
    Ok(ScreenshotData {
        width: img.width(),
        height: img.height(),
        png_data_url: format!("data:image/png;base64,{}", STANDARD.encode(&bytes)),
    })
}

#[tauri::command]
fn save_label(
    path: PathBuf,
    doc: LabelDocument,
    state: State<'_, AppState>,
) -> Result<(), String> {
    let _mutation_guard = persistent_mutation_guard(state.inner())?;
    let path = paths::validate_label_path(&path, false).map_err(|e| e.to_string())?;
    if path.exists() {
        // Validate the existing discriminator before replacement. Unknown future
        // sidecars remain byte-for-byte untouched even if IPC is invoked directly.
        LabelDocument::load(&path).map_err(|e| e.to_string())?;
    }
    validate_label_binding(&path, &doc)?;
    doc.save(&path).map_err(|e| e.to_string())
}

#[tauri::command]
fn load_label(path: PathBuf) -> Result<Option<LabelDocument>, String> {
    if !path.exists() {
        // Even a missing path must point to the fixed labels directory.
        paths::validate_label_path(&path, false).map_err(|e| e.to_string())?;
        return Ok(None);
    }
    let path = paths::validate_label_path(&path, true).map_err(|e| e.to_string())?;
    LabelDocument::load(&path).map(Some).map_err(|e| e.to_string())
}

#[tauri::command]
fn discard_screenshot(path: PathBuf, state: State<'_, AppState>) -> Result<(), String> {
    let _mutation_guard = persistent_mutation_guard(state.inner())?;
    let path = paths::validate_screenshot_path(&path).map_err(|e| e.to_string())?;
    let dst = paths::trash_dir().join(path.file_name().ok_or("no name")?);
    let label = paths::label_path_for(&path);
    let label_move = if label.exists() {
        let label = paths::validate_label_path(&label, true).map_err(|e| e.to_string())?;
        let destination = paths::trash_dir().join(label.file_name().ok_or("no label name")?);
        Some((label, destination))
    } else {
        None
    };
    liveblock_config::move_regular_file_pair_no_replace(
        &path,
        &dst,
        label_move
            .as_ref()
            .map(|(source, destination)| (source.as_path(), destination.as_path())),
    )
    .map_err(|error| error.to_string())
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

#[tauri::command]
fn start_training(_epochs: u32, _batch: u32, _imgsz: u32) -> Result<(), String> {
    if !liveblock_config::developer_training_runtime_available() {
        return Err("release builds are inference-only; use the source training workflow".into());
    }
    Err("Windows source training dispatch is not implemented; use the documented source companion workflow".into())
}

#[tauri::command]
fn cancel_training() -> Result<(), String> {
    Ok(())
}

#[tauri::command]
fn show_window(label: String, action_sequence: u64, app: AppHandle) -> Result<(), String> {
    if !matches!(label.as_str(), "editor" | "labeling" | "training") {
        return Err(format!("window is not renderer-openable: {label}"));
    }
    if !action_sequence_is_allowed(&app, action_sequence) {
        return Ok(());
    }
    let window = app
        .get_webview_window(&label)
        .ok_or_else(|| format!("window is unavailable: {label}"))?;
    window.show().map_err(|error| error.to_string())?;
    window.set_focus().map_err(|error| error.to_string())
}

#[tauri::command]
fn hide_window(label: String, app: AppHandle) -> Result<(), String> {
    if !matches!(label.as_str(), "editor" | "labeling" | "training") {
        return Err(format!("window is not renderer-hideable: {label}"));
    }
    app.get_webview_window(&label)
        .ok_or_else(|| format!("window is unavailable: {label}"))?
        .hide()
        .map_err(|error| error.to_string())
}

#[tauri::command]
fn quit(state: State<'_, AppState>) {
    quit_action(&state.app);
}
