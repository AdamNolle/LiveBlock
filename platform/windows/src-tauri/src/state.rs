//! Shared application state passed to every Tauri command.

use crate::capture::CaptureSession;
use crate::detection::Detector;
use crate::inpainting::Inpainter;
use crate::regions::SharedRegionStore;
use crate::training::TrainingJob;

use parking_lot::Mutex;
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicU8, AtomicUsize};
use std::sync::Arc;
use tauri::AppHandle;

pub struct AppState {
    pub regions: SharedRegionStore,
    pub capture: Arc<Mutex<Option<CaptureSession>>>,
    pub capture_lifecycle: Arc<Mutex<()>>,
    pub capture_generation: Arc<AtomicU64>,
    pub recovery_generation: AtomicU64,
    pub recovery_cycles: AtomicU8,
    pub capture_desired: AtomicBool,
    pub suspension_reasons: AtomicU8,
    pub selected_monitor: AtomicUsize,
    pub selected_monitor_name: Mutex<Option<String>>,
    pub last_panic_action: AtomicU64,
    pub last_stop_action: AtomicU64,
    pub shutting_down: AtomicBool,
    pub click_through_available: AtomicBool,
    pub capture_exclusion_available: AtomicBool,
    pub global_hotkeys_available: AtomicBool,
    pub lifecycle_observer_available: AtomicBool,
    pub detection_enabled: Arc<AtomicBool>,
    pub detector: Arc<Mutex<Option<Detector>>>,
    pub model_update: Mutex<()>,
    pub inpainter: Arc<Mutex<Inpainter>>,
    pub training: Arc<Mutex<Option<TrainingJob>>>,
    pub app: AppHandle,
}

impl AppState {
    pub fn new(app: AppHandle, regions: SharedRegionStore) -> Self {
        Self {
            regions,
            capture: Arc::new(Mutex::new(None)),
            capture_lifecycle: Arc::new(Mutex::new(())),
            capture_generation: Arc::new(AtomicU64::new(0)),
            recovery_generation: AtomicU64::new(0),
            recovery_cycles: AtomicU8::new(0),
            capture_desired: AtomicBool::new(false),
            suspension_reasons: AtomicU8::new(0),
            selected_monitor: AtomicUsize::new(0),
            selected_monitor_name: Mutex::new(None),
            last_panic_action: AtomicU64::new(0),
            last_stop_action: AtomicU64::new(0),
            shutting_down: AtomicBool::new(false),
            click_through_available: AtomicBool::new(false),
            capture_exclusion_available: AtomicBool::new(false),
            global_hotkeys_available: AtomicBool::new(false),
            lifecycle_observer_available: AtomicBool::new(false),
            detection_enabled: Arc::new(AtomicBool::new(false)),
            detector: Arc::new(Mutex::new(None)),
            model_update: Mutex::new(()),
            inpainter: Arc::new(Mutex::new(Inpainter::new())),
            training: Arc::new(Mutex::new(None)),
            app,
        }
    }
}
