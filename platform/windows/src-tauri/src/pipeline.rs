//! The off-thread realtime pipeline worker.
//!
//! Wires the CANONICAL shared-core pipeline:
//! ```text
//! capture(frame) -> Detector(ort) -> Vec<Detection>
//!   -> Tracker::update(observations)
//!   -> for tracks needing classification: SAFE STUB (empty allowlist) -> stay Unsure
//!   -> mask = build_remove_mask_from_tracks(tracks, w, h, min_hits)
//!   -> regions = user regions; per region: region_is_protected_black?
//!        protected -> paint_over_regions(Fill::opaque_black)   [PaintOver, DRM-safe]
//!        capturable -> content-aware flat cover                [Inpaint]
//!   -> overlay.present(cover patches)   [native layered window, NEVER a webview]
//! ```
//!
//! Everything here runs on a dedicated worker thread, woken by the capture
//! session's bounded tick channel — so detection/inpaint/encode never block the
//! WGC FrameArrived thread.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread::JoinHandle;

use arc_swap::ArcSwapOption;
use crossbeam_channel::{Receiver, RecvTimeoutError};
use parking_lot::Mutex;
use std::time::Duration;

use liveblock_core::{
    build_remove_mask_from_tracks, paint_over_regions, region_is_protected_black, Fill, Frame,
    NormRect, OverlayMode, PixelFormat, Track, Verdict,
};
use liveblock_tracker::{Observation, TrackBox, Tracker, TrackerConfig};

use crate::capture::{FrameTick, FrameView};
use crate::detection::{class_is_auto_removable, Detector};
use crate::overlay::{CoverPatch, OverlayWindow};
use crate::regions::NormalizedRegion;

/// How often (in worker ticks) the detector runs. Mirrors `CoordinatorConfig`.
const DETECT_EVERY: u64 = 4;
/// Luma threshold under which a region is treated as a DRM black-out.
const DRM_LUMA_THRESHOLD: u8 = 10;
/// Confirmation hits required before a Remove track erases anything.
const MIN_HITS: u32 = 3;

/// Shared handles the worker reads each tick. Cloneable; cheap.
pub struct PipelineContext {
    pub latest: Arc<ArcSwapOption<FrameView>>,
    pub overlay: Arc<OverlayWindow>,
    pub detector: Arc<Mutex<Option<Detector>>>,
    pub detection_enabled: Arc<AtomicBool>,
    /// Snapshot of user regions, refreshed by the command layer.
    pub regions: Arc<ArcSwapOption<Vec<NormalizedRegion>>>,
}

/// Owns the worker thread; stop by dropping or calling [`stop`](Self::stop).
pub struct PipelineWorker {
    running: Arc<AtomicBool>,
    handle: Option<JoinHandle<()>>,
}

impl PipelineWorker {
    pub fn spawn(ctx: PipelineContext, ticks: Receiver<FrameTick>) -> Self {
        let running = Arc::new(AtomicBool::new(true));
        let running_t = running.clone();
        let handle = std::thread::Builder::new()
            .name("liveblock-pipeline".into())
            .spawn(move || worker_loop(ctx, ticks, running_t))
            .expect("spawn pipeline worker");
        Self {
            running,
            handle: Some(handle),
        }
    }

    pub fn stop(mut self) {
        self.running.store(false, Ordering::SeqCst);
        if let Some(h) = self.handle.take() {
            let _ = h.join();
        }
    }
}

impl Drop for PipelineWorker {
    fn drop(&mut self) {
        self.running.store(false, Ordering::SeqCst);
        if let Some(h) = self.handle.take() {
            let _ = h.join();
        }
    }
}

fn worker_loop(ctx: PipelineContext, ticks: Receiver<FrameTick>, running: Arc<AtomicBool>) {
    let mut tracker = Tracker::with_config(TrackerConfig {
        min_hits: MIN_HITS,
        ..TrackerConfig::default()
    });
    let mut worker_tick: u64 = 0;
    // Last normalized detections, reused on non-detect ticks (Tracker still
    // predicts between detections via its learned velocity).
    let mut last_obs: Vec<Observation> = Vec::new();

    while running.load(Ordering::SeqCst) {
        // Wake on a fresh frame, but time out so we can re-check `running` and
        // still present (covers move via the tracker's predicted boxes even
        // when the detector is idle).
        match ticks.recv_timeout(Duration::from_millis(100)) {
            Ok(_) => {}
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => break,
        }

        let Some(frame_view) = ctx.latest.load_full() else {
            continue;
        };
        worker_tick = worker_tick.wrapping_add(1);

        let width = frame_view.width;
        let height = frame_view.height;
        if width == 0 || height == 0 {
            continue;
        }

        // ---- 1. Detection (every Nth tick), gated by the enable flag. ----
        let run_detect = ctx.detection_enabled.load(Ordering::Relaxed)
            && (worker_tick.is_multiple_of(DETECT_EVERY) || worker_tick == 1);
        if run_detect {
            if let Some(det) = ctx.detector.lock().as_mut() {
                match det.detect(&frame_view.bytes, width, height) {
                    Ok(boxes) => {
                        last_obs = boxes
                            .iter()
                            .map(|b| {
                                Observation::new(
                                    TrackBox::new(
                                        b.det.x,
                                        b.det.y,
                                        b.det.width,
                                        b.det.height,
                                    ),
                                    b.det.class_id,
                                )
                            })
                            .collect();
                    }
                    Err(e) => {
                        tracing::warn!("detect failed: {e}");
                        last_obs.clear();
                    }
                }
            }
        }

        // ---- 2. Tracker update + SAFE classifier stub. ----
        // Feed observations only on detect ticks; on intermediate ticks pass an
        // empty slice so tracks coast on their learned velocity (predicted_boxes
        // keep masks glued to moving content between detections).
        let obs_this_tick: &[Observation] = if run_detect { &last_obs } else { &[] };
        let matched = tracker.update(obs_this_tick);
        let _ = matched;

        // SAFE STUB: classify any track still `Unsure`. With an EMPTY sponsor
        // allowlist, no COCO class is auto-removable, so every track stays
        // `Unsure` -> build_remove_mask_from_tracks paints NOTHING from the
        // generic model. This is the fix for the "erases people/cars" bug.
        //
        // TODO(windows-port): replace with real ClassifySignals gathered per
        // track (team gallery sim / number gate / sponsor prob) fed to
        // liveblock_core::decide_verdict once a logo model is bundled.
        let pending: Vec<(u64, u32)> = tracker
            .tracks()
            .iter()
            .filter(|t| t.needs_classification())
            .map(|t| (t.id, t.class_id))
            .collect();
        for (id, class_id) in pending {
            if class_is_auto_removable(class_id) {
                // Currently unreachable (allowlist is empty) but keeps the wiring
                // honest for when the real classifier lands.
                tracker.set_verdict(id, Verdict::Remove);
            }
            // else: leave it Unsure — never auto-erase unclassified content.
        }

        // ---- 3. Build the detection remove-mask from tracks (carves Keep). ----
        // (Kept available for an inpaint backend that consumes a pixel mask; the
        // native cover path below derives rects from confirmed Remove tracks.)
        let tracks_snapshot: Vec<Track> = tracker.tracks().to_vec();
        let _detection_mask =
            build_remove_mask_from_tracks(&tracks_snapshot, width, height, MIN_HITS);

        // ---- 4. Assemble cover patches. ----
        let mut patches: Vec<CoverPatch> = Vec::new();

        // Build a core Frame view for DRM probing (Bgra8, top-left).
        let frame = Frame {
            width,
            height,
            format: PixelFormat::Bgra8,
            index: frame_view.index,
            pixels: frame_view.bytes.clone(),
        };

        // 4a. User regions: each is covered. Protected (DRM black) regions use
        //     the capture-free PaintOver path; capturable regions get a
        //     content-aware flat cover (Inpaint mode).
        if let Some(regions) = ctx.regions.load_full() {
            for r in regions.iter() {
                let rect = NormRect {
                    x: r.x as f32,
                    y: r.y as f32,
                    width: r.width as f32,
                    height: r.height as f32,
                };
                if region_is_protected_black(&frame, rect, DRM_LUMA_THRESHOLD) {
                    // DRM-safe: flat opaque cover, never reads protected pixels.
                    let pp = paint_over_regions(std::iter::once(rect), Fill::opaque_black());
                    patches.extend(pp.into_iter().map(CoverPatch::from));
                } else {
                    // Capturable: content-aware flat fill from the border.
                    let fill = border_fill(&frame_view.bytes, width, height, rect);
                    patches.push(CoverPatch { rect, fill, mode: OverlayMode::Inpaint });
                }
            }
        }

        // 4b. Confirmed Remove tracks (from the model). With the empty allowlist
        //     this is currently always empty; included so the wiring is complete.
        for t in &tracks_snapshot {
            if t.verdict == Verdict::Remove && t.hits >= MIN_HITS {
                let rect = NormRect {
                    x: t.bbox.x,
                    y: t.bbox.y,
                    width: t.bbox.width,
                    height: t.bbox.height,
                };
                let fill = border_fill(&frame_view.bytes, width, height, rect);
                patches.push(CoverPatch { rect, fill, mode: OverlayMode::Inpaint });
            }
        }

        // ---- 5. Present on the native overlay. ----
        if let Err(e) = ctx.overlay.present(&patches) {
            tracing::warn!("overlay present failed: {e}");
        }
    }
}

/// Average BGRA colour sampled just outside `rect` (a cheap content-aware flat
/// fill — the visual stand-in until the GPU mirror-blend/LaMa inpaint lands).
/// Returns an opaque `Fill::Solid`.
fn border_fill(bgra: &[u8], width: u32, height: u32, rect: NormRect) -> Fill {
    let w = width as i64;
    let h = height as i64;
    let stride = (width as usize) * 4;
    let x0 = (rect.x.clamp(0.0, 1.0) * width as f32) as i64;
    let y0 = (rect.y.clamp(0.0, 1.0) * height as f32) as i64;
    let x1 = ((rect.x + rect.width).clamp(0.0, 1.0) * width as f32) as i64;
    let y1 = ((rect.y + rect.height).clamp(0.0, 1.0) * height as f32) as i64;
    if x1 <= x0 || y1 <= y0 {
        return Fill::opaque_black();
    }
    let inset = (((x1 - x0).min(y1 - y0)) / 20).clamp(2, 16);

    let mut sum = [0u64; 3];
    let mut count: u64 = 0;
    let mut sample = |x: i64, y: i64| {
        if x < 0 || y < 0 || x >= w || y >= h {
            return;
        }
        let o = (y as usize) * stride + (x as usize) * 4;
        if o + 3 < bgra.len() {
            sum[0] += bgra[o] as u64; // B
            sum[1] += bgra[o + 1] as u64; // G
            sum[2] += bgra[o + 2] as u64; // R
            count += 1;
        }
    };
    // Top + bottom border strips.
    for y in (y0 - inset).max(0)..y0 {
        for x in x0..x1 {
            sample(x, y);
        }
    }
    for y in y1..(y1 + inset).min(h) {
        for x in x0..x1 {
            sample(x, y);
        }
    }
    if count == 0 {
        return Fill::opaque_black();
    }
    Fill::Solid {
        r: (sum[2] / count) as u8,
        g: (sum[1] / count) as u8,
        b: (sum[0] / count) as u8,
        a: 255,
    }
}
