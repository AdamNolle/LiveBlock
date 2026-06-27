//! Shared application state passed to every Tauri command.

use crate::capture::{CaptureSession, FrameView};
use crate::detection::Detector;
use crate::overlay::OverlayWindow;
use crate::pipeline::PipelineWorker;
use crate::regions::{NormalizedRegion, SharedRegionStore};
use crate::training::TrainingJob;

use arc_swap::ArcSwapOption;
use parking_lot::Mutex;
use std::sync::atomic::AtomicBool;
use std::sync::Arc;
use tauri::AppHandle;

pub struct AppState {
    pub regions: SharedRegionStore,
    /// Live region snapshot the pipeline worker reads each tick (kept in sync
    /// with `regions` by the region commands).
    pub region_snapshot: Arc<ArcSwapOption<Vec<NormalizedRegion>>>,
    pub capture: Arc<Mutex<Option<CaptureSession>>>,
    /// Active pipeline worker (one per running capture).
    pub pipeline: Arc<Mutex<Option<PipelineWorker>>>,
    /// Freshest captured frame, shared between capture, worker, and labeling.
    pub latest_frame: Arc<ArcSwapOption<FrameView>>,
    /// Native overlay surface (created lazily on first capture for the chosen
    /// monitor).
    pub overlay: Arc<Mutex<Option<Arc<OverlayWindow>>>>,
    pub detection_enabled: Arc<AtomicBool>,
    pub detector: Arc<Mutex<Option<Detector>>>,
    pub training: Arc<Mutex<Option<TrainingJob>>>,
    pub app: AppHandle,
}

impl AppState {
    pub fn new(app: AppHandle, regions: SharedRegionStore) -> Self {
        // Seed the worker's region snapshot from the persisted store.
        let snapshot = Arc::new(ArcSwapOption::from(Some(Arc::new(regions.current()))));
        Self {
            regions,
            region_snapshot: snapshot,
            capture: Arc::new(Mutex::new(None)),
            pipeline: Arc::new(Mutex::new(None)),
            latest_frame: Arc::new(ArcSwapOption::from(None)),
            overlay: Arc::new(Mutex::new(None)),
            detection_enabled: Arc::new(AtomicBool::new(false)),
            detector: Arc::new(Mutex::new(None)),
            training: Arc::new(Mutex::new(None)),
            app,
        }
    }

    /// Refresh the worker-visible region snapshot from the persisted store.
    /// Called after every region mutation command.
    pub fn refresh_region_snapshot(&self) {
        self.region_snapshot
            .store(Some(Arc::new(self.regions.current())));
    }
}
