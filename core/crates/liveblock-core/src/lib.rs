//! LiveBlock core: cross-platform traits and frame-loop coordinator.
//!
//! Re-exports the data types from `liveblock-regions`, `liveblock-labels`,
//! and `liveblock-detection`. Defines the platform-agnostic traits the host
//! app implements: `Capture`, `Detector`, `Inpainter`. The `Coordinator`
//! drives the per-frame pipeline.
//!
//! No GUI or platform deps live in this crate. macOS bindings (swift-bridge)
//! and Windows/Linux bindings (cbindgen) are added in separate adapter crates.

use async_trait::async_trait;
use parking_lot::Mutex;
use std::sync::Arc;
use thiserror::Error;

pub use liveblock_detection::{
    self as detection, flip_y, iou as detection_iou, non_max_suppression, to_pixel_rect_bottom_left,
    to_pixel_rect_top_left, Detection, PixelRect,
};
pub use liveblock_labels::{self as labels, Iso8601, LabelBox, LabelDocument};
pub use liveblock_regions::{self as regions, NormalizedRegion, RegionStore};

#[derive(Debug, Error)]
pub enum CoreError {
    #[error("capture failed: {0}")]
    Capture(String),
    #[error("detection failed: {0}")]
    Detect(String),
    #[error("inpaint failed: {0}")]
    Inpaint(String),
}

/// One captured frame in a platform-neutral form. Pixel data is opaque to the
/// core; platform adapters interpret `Frame::pixels` according to `format`.
#[derive(Debug, Clone)]
pub struct Frame {
    pub width: u32,
    pub height: u32,
    pub format: PixelFormat,
    /// Frame index since coordinator start.
    pub index: u64,
    /// Raw bytes; layout determined by `format`.
    pub pixels: Arc<Vec<u8>>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PixelFormat {
    Bgra8,
    Rgba8,
    /// 4:2:0 planar with platform-defined plane layout.
    Nv12,
}

/// A binary mask covering regions to inpaint, in the frame's coordinate space.
#[derive(Debug, Clone)]
pub struct Mask {
    pub width: u32,
    pub height: u32,
    /// Row-major; 0 = keep, non-zero = inpaint.
    pub bits: Arc<Vec<u8>>,
}

/// Platform capture source. Each call returns the next frame, or `None` if
/// capture has stopped.
#[async_trait]
pub trait Capture: Send + Sync {
    async fn next_frame(&self) -> Result<Option<Frame>, CoreError>;
}

/// ML detector. Maps a frame to normalized [0..1] detections.
#[async_trait]
pub trait Detector: Send + Sync {
    async fn detect(&self, frame: &Frame) -> Result<Vec<Detection>, CoreError>;
}

/// Inpainting backend. Receives a frame plus mask and writes a corrected
/// frame back through some platform sink (the trait does not dictate where
/// the output goes; implementations may push to a window, encoder, etc.).
#[async_trait]
pub trait Inpainter: Send + Sync {
    async fn inpaint(&self, frame: &Frame, mask: &Mask) -> Result<(), CoreError>;
}

/// Cached masks keyed by frame size. Avoids re-rasterizing user regions on
/// every frame when the capture size hasn't changed.
#[derive(Debug, Default)]
pub struct MaskCache {
    inner: Mutex<Option<CachedMask>>,
}

#[derive(Debug, Clone)]
struct CachedMask {
    width: u32,
    height: u32,
    region_signature: u64,
    mask: Arc<Mask>,
}

impl MaskCache {
    pub fn new() -> Self {
        Self::default()
    }

    /// Look up a cached mask matching `(width, height, signature)` or compute
    /// it via `build`.
    pub fn get_or_build<F>(
        &self,
        width: u32,
        height: u32,
        signature: u64,
        build: F,
    ) -> Arc<Mask>
    where
        F: FnOnce() -> Mask,
    {
        let mut g = self.inner.lock();
        if let Some(c) = g.as_ref() {
            if c.width == width && c.height == height && c.region_signature == signature {
                return c.mask.clone();
            }
        }
        let m = Arc::new(build());
        *g = Some(CachedMask {
            width,
            height,
            region_signature: signature,
            mask: m.clone(),
        });
        m
    }

    pub fn invalidate(&self) {
        *self.inner.lock() = None;
    }
}

/// Coordinator config. `detect_every` runs the detector once every N frames;
/// other frames reuse the most recent detections (cheaper steady-state).
#[derive(Debug, Clone)]
pub struct CoordinatorConfig {
    pub detect_every: u64,
    pub score_threshold: f32,
    pub nms_iou_threshold: f32,
}

impl Default for CoordinatorConfig {
    fn default() -> Self {
        Self {
            detect_every: 4,
            score_threshold: 0.25,
            nms_iou_threshold: 0.45,
        }
    }
}

/// Drives one tick of the capture -> detect -> inpaint pipeline.
pub struct Coordinator<C: Capture, D: Detector, I: Inpainter> {
    capture: Arc<C>,
    detector: Arc<D>,
    inpainter: Arc<I>,
    config: CoordinatorConfig,
    last_detections: Mutex<Vec<Detection>>,
    frame_count: Mutex<u64>,
}

impl<C: Capture, D: Detector, I: Inpainter> Coordinator<C, D, I> {
    pub fn new(capture: Arc<C>, detector: Arc<D>, inpainter: Arc<I>) -> Self {
        Self::with_config(capture, detector, inpainter, CoordinatorConfig::default())
    }

    pub fn with_config(
        capture: Arc<C>,
        detector: Arc<D>,
        inpainter: Arc<I>,
        mut config: CoordinatorConfig,
    ) -> Self {
        // Guard against `detect_every == 0` which would `count % 0` panic.
        if config.detect_every == 0 {
            config.detect_every = 1;
        }
        Self {
            capture,
            detector,
            inpainter,
            config,
            last_detections: Mutex::new(Vec::new()),
            frame_count: Mutex::new(0),
        }
    }

    /// Run one iteration of the pipeline. Returns `Ok(false)` when the capture
    /// source has stopped delivering frames.
    pub async fn tick(&self) -> Result<bool, CoreError> {
        let Some(frame) = self.capture.next_frame().await? else {
            return Ok(false);
        };

        let count = {
            let mut g = self.frame_count.lock();
            *g += 1;
            *g
        };

        // Run the detector on every Nth frame; otherwise reuse the most-recent
        // detections.
        let detections = if count % self.config.detect_every == 0 || count == 1 {
            let raw = self.detector.detect(&frame).await?;
            let filtered = liveblock_detection::filter_by_score(&raw, self.config.score_threshold);
            let nms =
                liveblock_detection::non_max_suppression(&filtered, self.config.nms_iou_threshold);
            *self.last_detections.lock() = nms.clone();
            nms
        } else {
            self.last_detections.lock().clone()
        };

        let mask = build_mask_from_detections(&detections, frame.width, frame.height);
        self.inpainter.inpaint(&frame, &mask).await?;
        Ok(true)
    }

    /// Snapshot of the most recent detections (useful for UI overlays).
    pub fn last_detections(&self) -> Vec<Detection> {
        self.last_detections.lock().clone()
    }
}

/// Rasterize a list of normalized detections into a binary mask. This is a
/// reference implementation used by tests and by hosts that don't ship their
/// own rasterizer.
pub fn build_mask_from_detections(detections: &[Detection], width: u32, height: u32) -> Mask {
    let mut bits = vec![0u8; (width as usize) * (height as usize)];
    let w = width as f32;
    let h = height as f32;
    for d in detections {
        let x0 = ((d.x * w).floor() as i32).clamp(0, width as i32);
        let y0 = ((d.y * h).floor() as i32).clamp(0, height as i32);
        let x1 = (((d.x + d.width) * w).ceil() as i32).clamp(0, width as i32);
        let y1 = (((d.y + d.height) * h).ceil() as i32).clamp(0, height as i32);
        for y in y0..y1 {
            let row = (y as usize) * (width as usize);
            for x in x0..x1 {
                bits[row + x as usize] = 255;
            }
        }
    }
    Mask {
        width,
        height,
        bits: Arc::new(bits),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use std::sync::atomic::{AtomicU64, Ordering};

    struct StubCapture {
        frames_left: AtomicU64,
    }

    #[async_trait]
    impl Capture for StubCapture {
        async fn next_frame(&self) -> Result<Option<Frame>, CoreError> {
            let n = self.frames_left.fetch_sub(1, Ordering::SeqCst);
            if n == 0 {
                return Ok(None);
            }
            Ok(Some(Frame {
                width: 16,
                height: 16,
                format: PixelFormat::Bgra8,
                index: n,
                pixels: Arc::new(vec![0; 16 * 16 * 4]),
            }))
        }
    }

    struct StubDetector {
        calls: AtomicU64,
    }

    #[async_trait]
    impl Detector for StubDetector {
        async fn detect(&self, _frame: &Frame) -> Result<Vec<Detection>, CoreError> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            Ok(vec![Detection::new(0, 0.9, 0.0, 0.0, 0.5, 0.5)])
        }
    }

    struct StubInpainter {
        calls: AtomicU64,
    }

    #[async_trait]
    impl Inpainter for StubInpainter {
        async fn inpaint(&self, _frame: &Frame, _mask: &Mask) -> Result<(), CoreError> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            Ok(())
        }
    }

    #[tokio::test]
    async fn coordinator_runs_until_capture_drains() {
        let cap = Arc::new(StubCapture {
            frames_left: AtomicU64::new(5),
        });
        let det = Arc::new(StubDetector {
            calls: AtomicU64::new(0),
        });
        let inp = Arc::new(StubInpainter {
            calls: AtomicU64::new(0),
        });
        let coord = Coordinator::with_config(
            cap.clone(),
            det.clone(),
            inp.clone(),
            CoordinatorConfig {
                detect_every: 2,
                score_threshold: 0.0,
                nms_iou_threshold: 0.5,
            },
        );

        let mut frames = 0;
        while coord.tick().await.unwrap() {
            frames += 1;
        }
        assert_eq!(frames, 5);
        // Inpaint runs every frame.
        assert_eq!(inp.calls.load(Ordering::SeqCst), 5);
        // Detector runs at frame 1 and every 2nd after: 1, 2, 4 -> 3 calls.
        assert_eq!(det.calls.load(Ordering::SeqCst), 3);
    }

    #[test]
    fn mask_cache_reuses_when_signature_matches() {
        let cache = MaskCache::new();
        let mut builds = 0u32;
        let _ = cache.get_or_build(10, 10, 1, || {
            builds += 1;
            Mask {
                width: 10,
                height: 10,
                bits: Arc::new(vec![0; 100]),
            }
        });
        let _ = cache.get_or_build(10, 10, 1, || {
            builds += 1;
            Mask {
                width: 10,
                height: 10,
                bits: Arc::new(vec![0; 100]),
            }
        });
        assert_eq!(builds, 1);
    }

    #[test]
    fn mask_cache_rebuilds_on_size_change() {
        let cache = MaskCache::new();
        let mut builds = 0u32;
        let _ = cache.get_or_build(10, 10, 1, || {
            builds += 1;
            Mask {
                width: 10,
                height: 10,
                bits: Arc::new(vec![0; 100]),
            }
        });
        let _ = cache.get_or_build(20, 10, 1, || {
            builds += 1;
            Mask {
                width: 20,
                height: 10,
                bits: Arc::new(vec![0; 200]),
            }
        });
        assert_eq!(builds, 2);
    }

    #[test]
    fn build_mask_marks_detected_pixels() {
        let dets = vec![Detection::new(0, 1.0, 0.0, 0.0, 0.5, 0.5)];
        let m = build_mask_from_detections(&dets, 4, 4);
        // top-left 2x2 should be set
        assert_eq!(m.bits[0], 255);
        assert_eq!(m.bits[1], 255);
        assert_eq!(m.bits[4], 255);
        assert_eq!(m.bits[5], 255);
        // bottom-right corner should be zero
        assert_eq!(m.bits[15], 0);
    }
}
