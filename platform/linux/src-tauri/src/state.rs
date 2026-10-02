//! Central app state. Held inside Tauri's `State<Arc<AppState>>`.
//!
//! Beyond the persisted region store and the capture/detection toggles, this
//! owns the long-lived runtime singletons the per-frame pipeline reuses:
//!   - the ONNX detector (loaded once, behind a `Mutex` because `ort::Session`
//!     is not `Sync`),
//!   - the mirror-blend inpainter (owns its patch cache + optional wgpu device),
//!   - the constant-velocity `Tracker` (carries sticky keep/remove verdicts
//!     across frames — see `liveblock_core` / `liveblock_tracker`),
//!   - a handle to the running capture task so `stop_capture` can join it.

use crate::detection::Detector;
use crate::inpainting::Inpainter;
use crate::regions::SharedRegionStore;
use liveblock_core::PolicyMode;
use liveblock_tracker::{Tracker, TrackerConfig};
use parking_lot::Mutex;
use std::sync::atomic::{AtomicBool, AtomicU64};
use std::sync::Arc;

/// Handle to the spawned capture→detect→track→paint task. Dropping/aborting it
/// stops the loop; the `running` flag is the cooperative shutdown signal the
/// loop itself polls. We use Tauri's async-runtime handle so spawning works
/// from a sync command handler (it spawns onto the runtime we `set()` in main).
pub struct CaptureHandle {
    pub join: tauri::async_runtime::JoinHandle<()>,
}

pub struct AppState {
    pub training_job: Mutex<Option<crate::training::TrainingJob>>,
    pub region_store: SharedRegionStore,
    pub overlay_ready: AtomicBool,
    pub layer_shell_active: AtomicBool,
    pub capture_running: AtomicBool,
    pub cover_only_active: AtomicBool,
    pub cover_only_preferred: AtomicBool,
    pub capture_generation: AtomicU64,
    pub latest_frame: arc_swap::ArcSwapOption<crate::capture::FrameView>,
    pub detection_enabled: AtomicBool,
    pub current_patches: Mutex<Vec<crate::inpainting::PatchPayload>>,
    pub last_detections: Mutex<Vec<String>>,

    // ---- Runtime singletons (lazily initialized by the capture task) ----
    /// Loaded ONNX detector. `None` until a model is found + loaded; the
    /// pipeline degrades to "user regions only" when absent.
    pub detector: Arc<Mutex<Option<Detector>>>,
    /// Mirror-blend inpainter + paint-over patch builder.
    pub inpainter: Arc<Mutex<Inpainter>>,
    /// Constant-velocity tracker carrying sticky verdicts.
    pub tracker: Arc<Mutex<Tracker>>,
    /// Which policy applies when classifier signals are ambiguous. Sports mode
    /// is the safe default (never erase identity on a guess).
    pub policy_mode: PolicyMode,
    /// Live capture task handle (so `stop_capture` can abort + join it).
    pub capture_task: Mutex<Option<CaptureHandle>>,
}

impl AppState {
    pub fn new(region_store: SharedRegionStore) -> Arc<Self> {
        Arc::new(Self {
            region_store,
            training_job: Mutex::new(None),
            overlay_ready: AtomicBool::new(false),
            layer_shell_active: AtomicBool::new(false),
            capture_running: AtomicBool::new(false),
            cover_only_active: AtomicBool::new(false),
            cover_only_preferred: AtomicBool::new(false),
            capture_generation: AtomicU64::new(0),
            latest_frame: arc_swap::ArcSwapOption::from(None),
            detection_enabled: AtomicBool::new(false),
            current_patches: Mutex::new(Vec::new()),
            last_detections: Mutex::new(Vec::new()),
            detector: Arc::new(Mutex::new(None)),
            inpainter: Arc::new(Mutex::new(Inpainter::new())),
            tracker: Arc::new(Mutex::new(Tracker::with_config(TrackerConfig::default()))),
            // Keep identity on uncertainty; only explicit removable model labels
            // can authorize automatic covers.
            policy_mode: PolicyMode::SportsBroadcast,
            capture_task: Mutex::new(None),
        })
    }
}
