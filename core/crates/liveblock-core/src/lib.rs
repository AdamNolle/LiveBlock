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
pub use liveblock_labels::{self as labels, Iso8601, LabelBox, LabelClass, LabelDocument};
pub use liveblock_regions::{self as regions, NormalizedRegion, RegionStore};
pub use liveblock_tracker::{
    self as tracker, Observation, Track, TrackBox, TrackId, Tracker, TrackerConfig, Verdict,
};

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

/// Cached masks keyed by `(width, height, signature)`. Retains a small bounded
/// set (most-recently-used) rather than a single slot, so alternating capture
/// sizes or region signatures don't evict each other and force a rebuild every
/// frame. A single-slot cache thrashes to 0% hit-rate the moment two keys
/// interleave (e.g. a detection mask alternating with a user-region mask, or
/// two displays of different size).
#[derive(Debug)]
pub struct MaskCache {
    inner: Mutex<Vec<CachedMask>>,
    capacity: usize,
}

impl Default for MaskCache {
    fn default() -> Self {
        Self::with_capacity(4)
    }
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

    /// Create a cache retaining up to `capacity` distinct masks (clamped to >= 1).
    pub fn with_capacity(capacity: usize) -> Self {
        Self {
            inner: Mutex::new(Vec::new()),
            capacity: capacity.max(1),
        }
    }

    /// Look up a cached mask matching `(width, height, signature)` or compute
    /// it via `build`. On a hit the entry is promoted to most-recently-used.
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
        if let Some(idx) = g
            .iter()
            .position(|c| c.width == width && c.height == height && c.region_signature == signature)
        {
            let hit = g.remove(idx);
            let mask = hit.mask.clone();
            g.insert(0, hit); // promote to MRU
            return mask;
        }
        let m = Arc::new(build());
        g.insert(
            0,
            CachedMask {
                width,
                height,
                region_signature: signature,
                mask: m.clone(),
            },
        );
        g.truncate(self.capacity);
        m
    }

    pub fn invalidate(&self) {
        self.inner.lock().clear();
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

/// The most-recently-built inpaint mask plus the inputs it was built from, so
/// `tick` can reuse it instead of allocating a fresh `width*height` byte buffer
/// every frame. In steady state the detector runs only every Nth frame and the
/// detections rarely change, so the overwhelming majority of frames reuse this
/// cached `Arc<Mask>` and allocate nothing.
struct CachedFrameMask {
    detections: Vec<Detection>,
    width: u32,
    height: u32,
    mask: Arc<Mask>,
}

/// Drives one tick of the capture -> detect -> inpaint pipeline.
pub struct Coordinator<C: Capture, D: Detector, I: Inpainter> {
    capture: Arc<C>,
    detector: Arc<D>,
    inpainter: Arc<I>,
    config: CoordinatorConfig,
    last_detections: Mutex<Vec<Detection>>,
    frame_count: Mutex<u64>,
    last_mask: Mutex<Option<CachedFrameMask>>,
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
            last_mask: Mutex::new(None),
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

        // Reuse the last mask when neither the detections nor the frame size
        // changed; otherwise rebuild once and cache. Detection derives PartialEq
        // and the box count is tiny, so the per-frame equality check is far
        // cheaper than reallocating a width*height mask on every frame.
        let mask = {
            let mut g = self.last_mask.lock();
            let reuse = g.as_ref().is_some_and(|c| {
                c.width == frame.width && c.height == frame.height && c.detections == detections
            });
            if reuse {
                g.as_ref().unwrap().mask.clone()
            } else {
                let m = Arc::new(build_mask_from_detections(
                    &detections,
                    frame.width,
                    frame.height,
                ));
                *g = Some(CachedFrameMask {
                    detections: detections.clone(),
                    width: frame.width,
                    height: frame.height,
                    mask: m.clone(),
                });
                m
            }
        };
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

// ===========================================================================
// Capture-free paint-over (DRM/HDCP-safe path)
// ===========================================================================
//
// DRM/HDCP content protection blacks out a region only when an app tries to
// *capture* (read back) the protected surface. It does NOT prevent another app
// from drawing ON TOP of the screen. So for protected content LiveBlock runs in
// `OverlayMode::PaintOver`: it never captures the protected pixels (nothing to
// black out), and the always-on-top click-through overlay simply fills the
// sponsor regions with an opaque patch. This is ordinary screen drawing, not
// circumvention — no protected video is decrypted, captured, or stored.
//
// The cost is honest: with no pixels to read, the detector can't *find* logos on
// fully-protected video, so PaintOver is driven by regions the user marks (or
// static per-broadcast templates), and the fill is a flat opaque cover rather
// than a content-aware inpaint. On non-protected content the normal
// capture -> detect -> inpaint path (`OverlayMode::Inpaint`) still runs.

/// How the overlay covers a region.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OverlayMode {
    /// Read the captured frame and synthesize a content-aware fill
    /// (mirror-blend / LaMa). Best quality; requires capturing the pixels.
    Inpaint,
    /// Cover the region with an opaque patch WITHOUT reading its pixels —
    /// the DRM-safe path (see module note above).
    PaintOver,
}

/// A normalized rectangle, [0..1], origin top-left. Lightweight cover geometry
/// decoupled from `NormalizedRegion` (which carries an id) and `Detection`
/// (which carries a class/score).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct NormRect {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}

impl From<&NormalizedRegion> for NormRect {
    fn from(r: &NormalizedRegion) -> Self {
        NormRect {
            x: r.x as f32,
            y: r.y as f32,
            width: r.width as f32,
            height: r.height as f32,
        }
    }
}

impl From<&Detection> for NormRect {
    fn from(d: &Detection) -> Self {
        NormRect {
            x: d.x,
            y: d.y,
            width: d.width,
            height: d.height,
        }
    }
}

/// Opaque cover fill for a paint-over patch.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Fill {
    /// Opaque RGBA. For DRM content the alpha should be 255 (fully cover).
    Solid { r: u8, g: u8, b: u8, a: u8 },
}

impl Fill {
    /// A fully-opaque black cover — the safe default for protected content.
    pub fn opaque_black() -> Self {
        Fill::Solid { r: 0, g: 0, b: 0, a: 255 }
    }
}

/// An opaque cover the overlay compositor draws over `rect`, computed WITHOUT
/// reading any captured pixels.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PaintPatch {
    pub rect: NormRect,
    pub fill: Fill,
}

/// Build paint patches for normalized regions WITHOUT a captured frame. This is
/// the DRM-safe path: it never touches protected pixels. Feed it user regions
/// (and, on mixed content, detections from the non-protected parts of the
/// screen); the overlay fills each rect with `fill`.
pub fn paint_over_regions<I, R>(regions: I, fill: Fill) -> Vec<PaintPatch>
where
    I: IntoIterator<Item = R>,
    R: Into<NormRect>,
{
    regions
        .into_iter()
        .map(|r| PaintPatch {
            rect: r.into(),
            fill,
        })
        .collect()
}

/// True when every sampled pixel of `rect` in `frame` is at/below
/// `luma_threshold` on all colour channels — the signature of a DRM/HDCP
/// blacked-out region. The capture pipeline routes such regions to
/// `OverlayMode::PaintOver` (a flat cover) instead of mirror-blend, which would
/// otherwise smear black across the patch. Samples a bounded grid so the check
/// stays cheap at frame rate. Returns false for formats it can't sample
/// (e.g. NV12) and for empty/out-of-bounds rects.
pub fn region_is_protected_black(frame: &Frame, rect: NormRect, luma_threshold: u8) -> bool {
    let bytes_per_pixel = match frame.format {
        PixelFormat::Bgra8 | PixelFormat::Rgba8 => 4usize,
        // NV12 colour lives in a separate plane; not worth sampling here.
        PixelFormat::Nv12 => return false,
    };
    let w = frame.width as usize;
    let h = frame.height as usize;
    if w == 0 || h == 0 {
        return false;
    }
    let stride = w * bytes_per_pixel;
    if frame.pixels.len() < stride * h {
        return false;
    }

    let clamp01 = |v: f32| v.clamp(0.0, 1.0);
    let x0 = (clamp01(rect.x) * w as f32).floor() as usize;
    let y0 = (clamp01(rect.y) * h as f32).floor() as usize;
    let x1 = (clamp01(rect.x + rect.width) * w as f32).ceil() as usize;
    let y1 = (clamp01(rect.y + rect.height) * h as f32).ceil() as usize;
    let x1 = x1.min(w);
    let y1 = y1.min(h);
    if x1 <= x0 || y1 <= y0 {
        return false;
    }

    // Subsample to ~<= 1024 probes so the per-frame cost is bounded.
    let area = (x1 - x0) * (y1 - y0);
    let step = (((area / 1024).max(1) as f64).sqrt().ceil()) as usize;
    let pixels = &frame.pixels;
    let mut sampled = 0usize;
    let mut y = y0;
    while y < y1 {
        let row = y * stride;
        let mut x = x0;
        while x < x1 {
            let idx = row + x * bytes_per_pixel;
            // First three bytes are colour in both BGRA and RGBA; any of them
            // above threshold means the region is NOT a flat black-out.
            if pixels[idx] > luma_threshold
                || pixels[idx + 1] > luma_threshold
                || pixels[idx + 2] > luma_threshold
            {
                return false;
            }
            sampled += 1;
            x += step;
        }
        y += step;
    }
    sampled > 0
}

// ===========================================================================
// Keep-vs-remove policy + protect-mask erosion (the "never erase identity" core)
// ===========================================================================

/// Which co-primary mode's policy applies when classifier signals are ambiguous.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PolicyMode {
    /// Live sports: erasing a team emblem or player/car number is catastrophic,
    /// so default to KEEP on uncertainty; only a clear, margin-beating sponsor
    /// signal removes.
    SportsBroadcast,
    /// General screen ad-blocking: a briefly-flickered content card is cheap, so
    /// default to REMOVE (block) on uncertainty to favor recall.
    GeneralAds,
}

/// Per-track classifier signals, gathered ONCE when a track first appears
/// (pipeline Stages 2a / 2b / 3). Fed to [`decide_verdict`].
#[derive(Debug, Clone, Copy, Default)]
pub struct ClassifySignals {
    /// Max cosine similarity to the protected team/league gallery (Stage 2a).
    pub team_gallery_sim: f32,
    /// The number-protect gate fired: digits-only, short, bold (Stage 2b).
    pub is_protected_number: bool,
    /// Zero-shot "commercial sponsor / advertisement" probability (Stage 3).
    pub sponsor_prob: f32,
    /// Zero-shot "team identity / content to keep" probability (Stage 3).
    pub keep_prob: f32,
}

/// Decision thresholds.
#[derive(Debug, Clone, Copy)]
pub struct PolicyThresholds {
    /// Gallery similarity at/above which a mark is force-KEPT (tau_keep).
    pub team_keep_sim: f32,
    /// Sponsor probability required to even consider removal (tau_block).
    pub sponsor_block: f32,
    /// Sponsor prob must beat keep prob by this margin in sports mode (tau_margin).
    pub sponsor_margin: f32,
}

impl Default for PolicyThresholds {
    fn default() -> Self {
        Self {
            team_keep_sim: 0.6,
            sponsor_block: 0.55,
            sponsor_margin: 0.15,
        }
    }
}

/// Decide a track's keep-vs-remove verdict from its signals. HARD PROTECT
/// OVERRIDES always win: a recognized team mark (gallery match) or a protected
/// number is KEPT regardless of any sponsor score — the spec-critical rule that
/// we prefer leaving a sponsor sliver over destroying team identity.
pub fn decide_verdict(
    signals: &ClassifySignals,
    mode: PolicyMode,
    th: &PolicyThresholds,
) -> Verdict {
    if signals.is_protected_number || signals.team_gallery_sim >= th.team_keep_sim {
        return Verdict::Keep;
    }
    match mode {
        PolicyMode::SportsBroadcast => {
            let confident = signals.sponsor_prob >= th.sponsor_block;
            let beats_keep = (signals.sponsor_prob - signals.keep_prob) >= th.sponsor_margin;
            if confident && beats_keep {
                Verdict::Remove
            } else {
                Verdict::Keep // never erase identity on a guess
            }
        }
        PolicyMode::GeneralAds => {
            if signals.keep_prob > signals.sponsor_prob {
                Verdict::Keep
            } else {
                Verdict::Remove // block on uncertainty (favor recall)
            }
        }
    }
}

/// Rasterize a normalized box into a mask buffer, setting covered pixels to `value`.
fn rasterize_box(bits: &mut [u8], b: &TrackBox, width: u32, height: u32, value: u8) {
    let w = width as f32;
    let h = height as f32;
    let x0 = ((b.x * w).floor() as i32).clamp(0, width as i32);
    let y0 = ((b.y * h).floor() as i32).clamp(0, height as i32);
    let x1 = (((b.x + b.width) * w).ceil() as i32).clamp(0, width as i32);
    let y1 = (((b.y + b.height) * h).ceil() as i32).clamp(0, height as i32);
    for y in y0..y1 {
        let row = (y as usize) * (width as usize);
        for x in x0..x1 {
            bits[row + x as usize] = value;
        }
    }
}

/// Build the inpaint/cover mask from tracks. Only **confirmed** (`hits >=
/// min_hits` — the N-consecutive-frames safety) **Remove**-verdict tracks are
/// painted; then every **Keep**-verdict (protected) track box is carved back
/// OUT, so a team emblem or number overlapping a sponsor patch is never painted
/// over. Keep always wins, and an unconfirmed Remove never erases anything.
pub fn build_remove_mask_from_tracks(
    tracks: &[Track],
    width: u32,
    height: u32,
    min_hits: u32,
) -> Mask {
    let mut bits = vec![0u8; (width as usize) * (height as usize)];
    for t in tracks {
        if t.verdict == Verdict::Remove && t.hits >= min_hits {
            rasterize_box(&mut bits, &t.bbox, width, height, 255);
        }
    }
    for t in tracks {
        if t.verdict == Verdict::Keep {
            rasterize_box(&mut bits, &t.bbox, width, height, 0);
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

    /// Records the address of each mask it receives, so a test can assert the
    /// coordinator reused one `Arc<Mask>` rather than reallocating per frame.
    struct RecordingInpainter {
        mask_ptrs: Mutex<Vec<usize>>,
    }

    #[async_trait]
    impl Inpainter for RecordingInpainter {
        async fn inpaint(&self, _frame: &Frame, mask: &Mask) -> Result<(), CoreError> {
            self.mask_ptrs.lock().push(mask as *const Mask as usize);
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
    fn mask_cache_no_thrash_on_alternating_keys() {
        let cache = MaskCache::with_capacity(4);
        let mut builds = 0u32;
        for _ in 0..3 {
            let _ = cache.get_or_build(10, 10, 1, || {
                builds += 1;
                Mask { width: 10, height: 10, bits: Arc::new(vec![0; 100]) }
            });
            let _ = cache.get_or_build(20, 20, 2, || {
                builds += 1;
                Mask { width: 20, height: 20, bits: Arc::new(vec![0; 400]) }
            });
        }
        // Both keys stay resident across the alternation: each builds exactly
        // once. A single-slot cache would evict and rebuild every call -> 6.
        assert_eq!(builds, 2);
    }

    #[tokio::test]
    async fn coordinator_reuses_mask_when_detections_unchanged() {
        let cap = Arc::new(StubCapture {
            frames_left: AtomicU64::new(5),
        });
        let det = Arc::new(StubDetector {
            calls: AtomicU64::new(0),
        });
        let inp = Arc::new(RecordingInpainter {
            mask_ptrs: Mutex::new(Vec::new()),
        });
        let coord = Coordinator::with_config(
            cap,
            det,
            inp.clone(),
            CoordinatorConfig {
                detect_every: 2,
                score_threshold: 0.0,
                nms_iou_threshold: 0.5,
            },
        );
        while coord.tick().await.unwrap() {}

        let ptrs = inp.mask_ptrs.lock().clone();
        assert_eq!(ptrs.len(), 5, "inpaint runs once per frame");
        // The stub detector returns a constant detection, so the mask is built
        // once and reused: every frame must observe the same live Arc<Mask>
        // allocation (no per-frame width*height realloc).
        assert!(
            ptrs.iter().all(|&p| p == ptrs[0]),
            "mask should be reused across frames, got {ptrs:?}"
        );
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

    #[test]
    fn paint_over_regions_needs_no_frame() {
        // Capture-free: patches are built from regions alone, never a Frame.
        let regions = [
            NormalizedRegion::new(0.1, 0.1, 0.2, 0.1),
            NormalizedRegion::new(0.5, 0.6, 0.2, 0.1),
        ];
        let fill = Fill::opaque_black();
        let patches = paint_over_regions(regions.iter(), fill);
        assert_eq!(patches.len(), 2);
        assert!(patches.iter().all(|p| p.fill == fill));
        assert!((patches[0].rect.x - 0.1).abs() < 1e-6);
    }

    #[test]
    fn protected_black_region_is_detected() {
        // A fully black BGRA frame is the signature of a DRM blacked-out surface.
        let frame = Frame {
            width: 8,
            height: 8,
            format: PixelFormat::Bgra8,
            index: 0,
            pixels: Arc::new(vec![0u8; 8 * 8 * 4]),
        };
        let full = NormRect { x: 0.0, y: 0.0, width: 1.0, height: 1.0 };
        assert!(region_is_protected_black(&frame, full, 8));
    }

    #[test]
    fn bright_region_is_not_protected() {
        let mut px = vec![0u8; 8 * 8 * 4];
        let idx = (2 * 8 + 3) * 4; // one bright pixel
        px[idx] = 255;
        px[idx + 1] = 255;
        px[idx + 2] = 255;
        px[idx + 3] = 255;
        let frame = Frame {
            width: 8,
            height: 8,
            format: PixelFormat::Bgra8,
            index: 0,
            pixels: Arc::new(px),
        };
        let full = NormRect { x: 0.0, y: 0.0, width: 1.0, height: 1.0 };
        // 64 pixels < 1024 -> step 1 -> the white pixel is examined and found.
        assert!(!region_is_protected_black(&frame, full, 8));
    }

    // ---- keep-vs-remove policy + protect-mask erosion ----

    #[test]
    fn protected_number_and_gallery_match_force_keep() {
        let th = PolicyThresholds::default();
        let num = ClassifySignals {
            is_protected_number: true,
            sponsor_prob: 0.99,
            ..Default::default()
        };
        // A 99%-sponsor score can NEVER erase a recognized number, in either mode.
        assert_eq!(decide_verdict(&num, PolicyMode::SportsBroadcast, &th), Verdict::Keep);
        assert_eq!(decide_verdict(&num, PolicyMode::GeneralAds, &th), Verdict::Keep);
        let team = ClassifySignals {
            team_gallery_sim: 0.9,
            sponsor_prob: 0.99,
            ..Default::default()
        };
        assert_eq!(decide_verdict(&team, PolicyMode::SportsBroadcast, &th), Verdict::Keep);
    }

    #[test]
    fn sports_removes_only_on_confident_margin() {
        let th = PolicyThresholds::default();
        let clear = ClassifySignals { sponsor_prob: 0.8, keep_prob: 0.1, ..Default::default() };
        assert_eq!(decide_verdict(&clear, PolicyMode::SportsBroadcast, &th), Verdict::Remove);
        // High sponsor but keep is close -> no margin -> KEEP (don't erase on a guess).
        let ambiguous = ClassifySignals { sponsor_prob: 0.6, keep_prob: 0.55, ..Default::default() };
        assert_eq!(decide_verdict(&ambiguous, PolicyMode::SportsBroadcast, &th), Verdict::Keep);
        // Below the block threshold -> KEEP.
        let weak = ClassifySignals { sponsor_prob: 0.5, keep_prob: 0.0, ..Default::default() };
        assert_eq!(decide_verdict(&weak, PolicyMode::SportsBroadcast, &th), Verdict::Keep);
    }

    #[test]
    fn general_blocks_on_uncertainty() {
        let th = PolicyThresholds::default();
        let ambiguous = ClassifySignals { sponsor_prob: 0.4, keep_prob: 0.3, ..Default::default() };
        assert_eq!(decide_verdict(&ambiguous, PolicyMode::GeneralAds, &th), Verdict::Remove);
        let content = ClassifySignals { sponsor_prob: 0.2, keep_prob: 0.7, ..Default::default() };
        assert_eq!(decide_verdict(&content, PolicyMode::GeneralAds, &th), Verdict::Keep);
    }

    fn track(id: u64, bbox: TrackBox, hits: u32, verdict: Verdict) -> Track {
        Track {
            id,
            bbox,
            velocity: (0.0, 0.0),
            age: hits,
            hits,
            time_since_update: 0,
            class_id: 0,
            verdict,
        }
    }

    #[test]
    fn unconfirmed_remove_track_paints_nothing() {
        let t = track(1, TrackBox::new(0.0, 0.0, 0.5, 0.5), 1, Verdict::Remove);
        let m = build_remove_mask_from_tracks(&[t], 4, 4, 3); // needs 3 hits, has 1
        assert!(m.bits.iter().all(|&b| b == 0), "unconfirmed track must not erase");
    }

    #[test]
    fn confirmed_remove_track_paints_its_box() {
        let t = track(1, TrackBox::new(0.0, 0.0, 0.5, 0.5), 5, Verdict::Remove);
        let m = build_remove_mask_from_tracks(&[t], 4, 4, 3);
        assert_eq!(m.bits[0], 255, "top-left covered");
        assert_eq!(m.bits[15], 0, "bottom-right clear");
    }

    #[test]
    fn keep_track_carves_out_overlapping_remove() {
        // A sponsor (Remove) covering the whole frame, with a team emblem (Keep)
        // in the top-left quadrant. The emblem pixels must NOT be erased.
        let sponsor = track(1, TrackBox::new(0.0, 0.0, 1.0, 1.0), 5, Verdict::Remove);
        let emblem = track(2, TrackBox::new(0.0, 0.0, 0.5, 0.5), 5, Verdict::Keep);
        let m = build_remove_mask_from_tracks(&[sponsor, emblem], 4, 4, 3);
        assert_eq!(m.bits[0], 0, "emblem pixels must be protected");
        assert_eq!(m.bits[15], 255, "sponsor pixels elsewhere still removed");
    }
}
