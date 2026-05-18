//! Central app state. Held inside Tauri's State<>.

use crate::regions::SharedRegionStore;
use parking_lot::Mutex;
use std::sync::atomic::AtomicBool;
use std::sync::Arc;

pub struct AppState {
    pub region_store: SharedRegionStore,
    pub capture_running: AtomicBool,
    pub detection_enabled: AtomicBool,
    pub current_patches: Mutex<Vec<crate::inpainting::PatchPayload>>,
    pub last_detections: Mutex<Vec<String>>,
}

impl AppState {
    pub fn new(region_store: SharedRegionStore) -> Arc<Self> {
        Arc::new(Self {
            region_store,
            capture_running: AtomicBool::new(false),
            detection_enabled: AtomicBool::new(false),
            current_patches: Mutex::new(Vec::new()),
            last_detections: Mutex::new(Vec::new()),
        })
    }
}
