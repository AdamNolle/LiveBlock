//! Shared application state passed to every Tauri command.

use crate::capture::CaptureSession;
use crate::detection::Detector;
use crate::inpainting::Inpainter;
use crate::regions::SharedRegionStore;
use crate::training::TrainingJob;

use parking_lot::Mutex;
use std::sync::atomic::AtomicBool;
use std::sync::Arc;
use tauri::AppHandle;

pub struct AppState {
    pub regions: SharedRegionStore,
    pub capture: Arc<Mutex<Option<CaptureSession>>>,
    pub detection_enabled: Arc<AtomicBool>,
    pub detector: Arc<Mutex<Option<Detector>>>,
    pub inpainter: Arc<Mutex<Inpainter>>,
    pub training: Arc<Mutex<Option<TrainingJob>>>,
    pub app: AppHandle,
}

impl AppState {
    pub fn new(app: AppHandle, regions: SharedRegionStore) -> Self {
        Self {
            regions,
            capture: Arc::new(Mutex::new(None)),
            detection_enabled: Arc::new(AtomicBool::new(false)),
            detector: Arc::new(Mutex::new(None)),
            inpainter: Arc::new(Mutex::new(Inpainter::new())),
            training: Arc::new(Mutex::new(None)),
            app,
        }
    }
}
