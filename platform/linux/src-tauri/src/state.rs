//! Central Linux application and capture lifecycle state.

use crate::capture::FrameView;
use crate::detection::Detector;
use crate::inpainting::Inpainter;
use crate::regions::SharedRegionStore;
use arc_swap::ArcSwapOption;
use parking_lot::Mutex;
use serde::Serialize;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

pub struct CaptureRuntime {
    pub stop_requested: Arc<AtomicBool>,
    pub task: tokio::task::JoinHandle<()>,
}

#[derive(Default)]
pub struct CaptureTelemetry {
    captured_frames: AtomicU64,
    processed_frames: AtomicU64,
    dropped_frames: AtomicU64,
    copy_errors: AtomicU64,
    last_frame_unix_ms: AtomicU64,
}

#[derive(Debug, Clone, Copy, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CaptureTelemetrySnapshot {
    pub captured_frames: u64,
    pub processed_frames: u64,
    pub dropped_frames: u64,
    pub copy_errors: u64,
    pub protected_frames: u64,
    pub protected_content: bool,
    pub last_frame_unix_ms: u64,
}

impl CaptureTelemetry {
    pub fn reset(&self) {
        self.captured_frames.store(0, Ordering::Relaxed);
        self.processed_frames.store(0, Ordering::Relaxed);
        self.dropped_frames.store(0, Ordering::Relaxed);
        self.copy_errors.store(0, Ordering::Relaxed);
        self.last_frame_unix_ms.store(0, Ordering::Relaxed);
    }
    pub fn captured(&self) {
        self.captured_frames.fetch_add(1, Ordering::Relaxed);
        self.last_frame_unix_ms
            .store(unix_millis(), Ordering::Relaxed);
    }
    pub fn processed(&self) {
        self.processed_frames.fetch_add(1, Ordering::Relaxed);
    }
    pub fn copy_error(&self) {
        self.copy_errors.fetch_add(1, Ordering::Relaxed);
    }
    pub fn set_dropped(&self, value: u64) {
        self.dropped_frames.store(value, Ordering::Relaxed);
    }
    pub fn snapshot(&self) -> CaptureTelemetrySnapshot {
        CaptureTelemetrySnapshot {
            captured_frames: self.captured_frames.load(Ordering::Relaxed),
            processed_frames: self.processed_frames.load(Ordering::Relaxed),
            dropped_frames: self.dropped_frames.load(Ordering::Relaxed),
            copy_errors: self.copy_errors.load(Ordering::Relaxed),
            protected_frames: 0,
            protected_content: false,
            last_frame_unix_ms: self.last_frame_unix_ms.load(Ordering::Relaxed),
        }
    }
}

pub struct AppState {
    pub region_store: SharedRegionStore,
    pub capture_running: AtomicBool,
    pub capture: tokio::sync::Mutex<Option<CaptureRuntime>>,
    pub capture_telemetry: CaptureTelemetry,
    pub latest_frame: ArcSwapOption<FrameView>,
    pub hotkeys_initialized: AtomicBool,
    pub hotkeys_available: AtomicBool,
    pub detection_enabled: AtomicBool,
    pub detector: Mutex<Option<Detector>>,
    pub model_update: Mutex<()>,
    pub inpainter: Mutex<Inpainter>,
    pub current_patches: Mutex<Vec<crate::inpainting::PatchPayload>>,
    pub last_detections: Mutex<Vec<String>>,
}

impl AppState {
    pub fn new(region_store: SharedRegionStore) -> Arc<Self> {
        Arc::new(Self {
            region_store,
            capture_running: AtomicBool::new(false),
            capture: tokio::sync::Mutex::new(None),
            capture_telemetry: CaptureTelemetry::default(),
            latest_frame: ArcSwapOption::from(None),
            hotkeys_initialized: AtomicBool::new(false),
            hotkeys_available: AtomicBool::new(false),
            detection_enabled: AtomicBool::new(false),
            detector: Mutex::new(None),
            model_update: Mutex::new(()),
            inpainter: Mutex::new(Inpainter::new()),
            current_patches: Mutex::new(Vec::new()),
            last_detections: Mutex::new(Vec::new()),
        })
    }
}

fn unix_millis() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}
