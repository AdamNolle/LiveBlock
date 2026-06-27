//! The capture → detect → track → classify → mask → inpaint/paint-over → emit
//! hot loop. This is the canonical pipeline every platform wires; on Linux it
//! runs as a tokio task spawned by `start_capture` and torn down by
//! `stop_capture`.
//!
//! The loop routes detections through the SHARED CORE rather than reimplementing
//! any policy:
//!   - `liveblock_tracker::Tracker::update` associates detections into stable
//!     tracks (constant-velocity), so a flickering box doesn't thrash.
//!   - each newly-seen track is classified ONCE. Until the real sponsor/team/
//!     number models are wired we use a SAFE STUB: an EMPTY class allowlist, so
//!     no generic COCO class is ever auto-removed (this fixes the confirmed
//!     "erases people/cars" bug). User-drawn regions still inpaint as before.
//!   - `liveblock_core::decide_verdict` turns signals into a sticky verdict.
//!   - `liveblock_core::build_remove_mask_from_tracks` paints ONLY confirmed
//!     Remove tracks and carves OUT every Keep box.
//!   - the DRM-safe path: any region whose pixels are a flat HDCP black-out
//!     (`region_is_protected_black`) is covered with an opaque paint-over patch
//!     (`paint_over_regions`) instead of being mirror-blended (which would smear
//!     black). We NEVER capture-then-inpaint protected pixels.

use crate::capture::{open_capture, FrameView};
use crate::detection::DetBox;
use crate::inpainting::PatchPayload;
use crate::state::AppState;
use liveblock_core::{
    build_remove_mask_from_tracks, decide_verdict, paint_over_regions, region_is_protected_black,
    ClassifySignals, Fill, Frame, NormRect, PixelFormat, PolicyThresholds,
};
use liveblock_regions::NormalizedRegion;
use liveblock_tracker::{Observation, TrackBox, Verdict};
use std::sync::atomic::Ordering;
use std::sync::Arc;
use tauri::{AppHandle, Emitter};

/// Run a detection pass every Nth captured frame (≈ keeps a 60 Hz capture's
/// inference cost at ~15 Hz). The tracker coasts boxes on the in-between frames.
const DETECT_EVERY: u64 = 4;

/// `region_is_protected_black` luma threshold. Pixels at/below this on all
/// channels read as an HDCP black-out.
const DRM_LUMA_THRESHOLD: u8 = 8;

/// SAFE-STUB classifier allowlist. EMPTY by design: with the generic COCO
/// detector there is NO class we are willing to auto-erase, so every track
/// stays `Unsure` → no removals. When the real sponsor/team/number models land,
/// this gate is replaced by actual `ClassifySignals` gathering. User regions are
/// unaffected — they always paint/inpaint.
fn class_is_removable(_class_id: u32) -> bool {
    // Intentionally always false. DO NOT add COCO class ids here — that would
    // resurrect the "erases people/cars" bug. Removal must come from the real
    // sponsor classifier, gathered into ClassifySignals below.
    false
}

/// Gather classifier signals for a track. STUB: until the real models exist we
/// return all-zero signals for non-allowlisted classes, which `decide_verdict`
/// resolves to Keep in SportsBroadcast mode — i.e. nothing is removed.
fn gather_signals(class_id: u32) -> ClassifySignals {
    if class_is_removable(class_id) {
        // (Unreachable today: the allowlist is empty.) When wired, a confident
        // sponsor would arrive as e.g. sponsor_prob ~0.9, keep_prob ~0.1.
        ClassifySignals {
            team_gallery_sim: 0.0,
            is_protected_number: false,
            sponsor_prob: 0.9,
            keep_prob: 0.1,
        }
    } else {
        // All zero → not a protected number, no sponsor signal → KEEP/Unsure.
        ClassifySignals::default()
    }
}

/// Spawn the capture pipeline. Returns the Tauri async-runtime `JoinHandle` so
/// the caller can store it in `AppState::capture_task` and abort on stop.
/// Spawning via `tauri::async_runtime::spawn` (rather than bare `tokio::spawn`)
/// means it works even when called from a synchronous Tauri command handler.
pub fn spawn(app: AppHandle, state: Arc<AppState>) -> tauri::async_runtime::JoinHandle<()> {
    tauri::async_runtime::spawn(async move {
        if let Err(e) = run(app.clone(), state.clone()).await {
            tracing::error!("capture pipeline error: {e:#}");
        }
        // Whatever the exit reason, make sure the flag and overlay are cleared.
        state.capture_running.store(false, Ordering::SeqCst);
        let _ = app.emit("capture-state-changed", serde_json::json!({ "running": false }));
        let _ = app.emit("patches-updated", Vec::<PatchPayload>::new());
    })
}

async fn run(app: AppHandle, state: Arc<AppState>) -> anyhow::Result<()> {
    let mut capture = open_capture()
        .await
        .map_err(|e| anyhow::anyhow!("open capture: {e:#}"))?;

    let _ = app.emit("capture-state-changed", serde_json::json!({ "running": true }));

    let thresholds = PolicyThresholds::default();
    let policy_mode = state.policy_mode;
    let min_hits = 3u32;

    let mut frame_index: u64 = 0;
    // Most recent detections, reused on the frames we don't run inference.
    let mut last_dets: Vec<DetBox> = Vec::new();
    // Auto-remove regions derived on the last detection tick; held on coast
    // frames so we keep covering them between inference passes.
    let mut auto_regions: Vec<NormalizedRegion> = Vec::new();

    while state.capture_running.load(Ordering::SeqCst) {
        let view = match capture.next_frame().await {
            Ok(v) => v,
            Err(e) => {
                tracing::warn!("next_frame failed: {e:#}");
                break;
            }
        };
        if view.width == 0 || view.height == 0 || view.pixels.is_empty() {
            // Negotiation not finished yet; keep polling.
            continue;
        }
        frame_index += 1;
        let (w, h) = (view.width, view.height);

        // Detection + tracker + classify run only on detection TICKS (every Nth
        // frame). On the in-between frames the tracker would otherwise re-ingest
        // stale detections and corrupt its velocity/age/hits state, so we hold
        // the previously-derived `auto_regions` instead and just re-render. The
        // tracker's own predict step already coasts boxes forward, so a future
        // optimization can present `predicted_boxes` on coast frames.
        let is_detection_tick =
            state.detection_enabled.load(Ordering::Relaxed) && frame_index % DETECT_EVERY == 0;

        if is_detection_tick {
            // --- 1. Detection ---------------------------------------------
            if let Some(det) = state.detector.lock().as_mut() {
                match det.detect_bgra(&view.pixels, w, h) {
                    Ok(boxes) => last_dets = boxes,
                    Err(e) => tracing::debug!("detect failed: {e:#}"),
                }
            }

            // --- 2. Tracker update ----------------------------------------
            // Detections are pixel-space; the tracker works in normalized
            // [0..1] units (same space as Detection / TrackBox).
            let observations: Vec<Observation> = last_dets
                .iter()
                .map(|d| {
                    Observation::new(
                        TrackBox::new(
                            d.x / w as f32,
                            d.y / h as f32,
                            d.w / w as f32,
                            d.h / h as f32,
                        ),
                        d.class_id,
                    )
                })
                .collect();

            // --- 3. Classify newly-seen tracks (SAFE-STUB → no removals) ---
            let to_classify: Vec<(u64, u32)> = {
                let mut tracker = state.tracker.lock();
                tracker.update(&observations);
                tracker
                    .tracks()
                    .iter()
                    .filter(|t| t.needs_classification())
                    .map(|t| (t.id, t.class_id))
                    .collect()
            };
            if !to_classify.is_empty() {
                let mut tracker = state.tracker.lock();
                for (id, class_id) in to_classify {
                    let signals = gather_signals(class_id);
                    let verdict = decide_verdict(&signals, policy_mode, &thresholds);
                    tracker.set_verdict(id, verdict);
                }
            }

            // --- 4. Derive the auto-remove region list from confirmed tracks
            auto_regions = derive_auto_regions(&state, w, h, min_hits);
        }

        // --- 5. Merge with user-drawn regions -----------------------------
        let mut all_regions: Vec<NormalizedRegion> = state.region_store.current();
        // Clone (don't move) so `auto_regions` persists onto the next coast frame.
        all_regions.extend(auto_regions.iter().cloned());

        // --- 6. DRM split: protected (black) regions → paint-over cover;
        //         everything else → mirror-blend inpaint --------------------
        let frame = frame_from_view(&view, frame_index);
        let mut inpaint_regions: Vec<NormalizedRegion> = Vec::with_capacity(all_regions.len());
        let mut protected_rects: Vec<NormRect> = Vec::new();
        for r in &all_regions {
            let rect = NormRect::from(r);
            if region_is_protected_black(&frame, rect, DRM_LUMA_THRESHOLD) {
                // DRM-safe: cover with a flat opaque patch; never read/inpaint
                // the protected pixels (they'd smear black through the blend).
                protected_rects.push(rect);
            } else {
                inpaint_regions.push(r.clone());
            }
        }
        let paint_patches = paint_over_regions(protected_rects, Fill::opaque_black());

        // --- 7. Render + emit ---------------------------------------------
        let payloads: Vec<PatchPayload> = state
            .inpainter
            .lock()
            .render(&view.pixels, w, h, &inpaint_regions, &paint_patches)
            .unwrap_or_default();

        *state.current_patches.lock() = payloads.clone();
        let _ = app.emit("patches-updated", &payloads);
    }

    capture.stop();
    Ok(())
}

/// Derive the auto-remove region list from the tracker's confirmed Remove
/// tracks, mirroring `build_remove_mask_from_tracks`' rules (confirmed Remove
/// painted, every Keep carved out). With the empty-allowlist stub there are
/// never any Remove tracks, so this short-circuits to empty and skips the w*h
/// mask allocation; it engages automatically once the real classifier ships.
fn derive_auto_regions(
    state: &Arc<AppState>,
    w: u32,
    h: u32,
    min_hits: u32,
) -> Vec<NormalizedRegion> {
    let tracker = state.tracker.lock();
    let has_removable = tracker
        .tracks()
        .iter()
        .any(|t| t.verdict == Verdict::Remove && t.hits >= min_hits);
    if !has_removable {
        return Vec::new();
    }
    // Build the authoritative core mask (keeps that code path live + correct)
    // then translate the same Remove-not-carved tracks into regions the
    // inpainter/paint-over path acts on (it consumes regions, not a bitmask).
    let _mask = build_remove_mask_from_tracks(tracker.tracks(), w, h, min_hits);
    let tracks = tracker.tracks();
    tracks
        .iter()
        .filter(|t| t.verdict == Verdict::Remove && t.hits >= min_hits)
        .filter(|t| {
            // Never emit a remove region a Keep track overlaps — mirrors the
            // mask's carve-out so identity is never erased.
            !tracks
                .iter()
                .any(|k| k.verdict == Verdict::Keep && k.bbox.iou(&t.bbox) > 0.0)
        })
        .map(|t| {
            NormalizedRegion::new(
                t.bbox.x as f64,
                t.bbox.y as f64,
                t.bbox.width as f64,
                t.bbox.height as f64,
            )
        })
        .collect()
}

/// Wrap a `FrameView` (always packed BGRA8) as a core `Frame` so the shared DRM
/// detector can sample it. Reuses the same backing bytes (the core copies into
/// an `Arc<Vec<u8>>`; we accept one copy per protected check pass).
fn frame_from_view(view: &FrameView, index: u64) -> Frame {
    Frame {
        width: view.width,
        height: view.height,
        format: PixelFormat::Bgra8,
        index,
        pixels: Arc::new(view.pixels.to_vec()),
    }
}
