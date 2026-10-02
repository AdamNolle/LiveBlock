//! The capture → detect → track → classify → mask → inpaint/paint-over → emit
//! hot loop. This is the canonical pipeline every platform wires; on Linux it
//! runs as a tokio task spawned by `start_capture` and torn down by
//! `stop_capture`.
//!
//! Explicit model classes become sticky remove/protect verdicts only after
//! fresh detection observations. Opaque covering handles capture-black regions;
//! it does not restore protected video pixels or bypass content protection.

use crate::capture::{open_capture, FrameView};
use crate::detection::DetBox;
use crate::inpainting::PatchPayload;
use crate::state::AppState;
use liveblock_core::{
    paint_over_regions, region_is_protected_black,
    Fill, Frame, NormRect, PixelFormat,
};
use liveblock_regions::NormalizedRegion;
use liveblock_tracker::{Observation, TrackBox, Verdict, Tracker};
use liveblock_detection::ModelClassPolicy;
use std::sync::atomic::Ordering;
use std::sync::Arc;
use tauri::{AppHandle, Emitter, Manager};

/// Run a detection pass every Nth captured frame (≈ keeps a 60 Hz capture's
/// inference cost at ~15 Hz). The tracker coasts boxes on the in-between frames.
const DETECT_EVERY: u64 = 4;

/// `region_is_protected_black` luma threshold. Pixels at/below this on all
/// channels read as an HDCP black-out.
const DRM_LUMA_THRESHOLD: u8 = 8;

/// Stop either blocking mode from IPC, hotkeys, or compositor-loss callbacks.
pub fn stop_capture(app: &AppHandle, state: &AppState) {
    state.capture_generation.fetch_add(1, Ordering::SeqCst);
    state.capture_running.store(false, Ordering::SeqCst);
    state.cover_only_active.store(false, Ordering::SeqCst);
    if let Some(handle) = state.capture_task.lock().take() {
        handle.join.abort();
    }
    if let Some(window) = app.get_webview_window("render") {
        let _ = window.hide();
    }
    state.latest_frame.store(None);
    state.current_patches.lock().clear();
    *state.tracker.lock() = liveblock_core::Tracker::default();
    state.last_detections.lock().clear();
    let _ = app.emit("capture-state-changed", false);
    let _ = app.emit("patches-updated", Vec::<PatchPayload>::new());
}

/// Spawn the capture pipeline. Returns the Tauri async-runtime `JoinHandle` so
/// the caller can store it in `AppState::capture_task` and abort on stop.
/// Spawning via `tauri::async_runtime::spawn` (rather than bare `tokio::spawn`)
/// means it works even when called from a synchronous Tauri command handler.
pub fn spawn(app: AppHandle, state: Arc<AppState>, generation: u64) -> tauri::async_runtime::JoinHandle<()> {
    tauri::async_runtime::spawn(async move {
        if let Err(e) = run(app.clone(), state.clone(), generation).await {
            tracing::error!("capture pipeline error: {e:#}");
        }
        if state.capture_generation.load(Ordering::SeqCst) != generation { return; }
        if let Some(w) = app.get_webview_window("render") { let _ = w.hide(); }
        state.latest_frame.store(None);
        // Whatever the exit reason, make sure the flag and overlay are cleared.
        state.capture_running.store(false, Ordering::SeqCst);
        state.current_patches.lock().clear();
        *state.tracker.lock() = Tracker::default();
        let _ = app.emit("capture-state-changed", false);
        let _ = app.emit("patches-updated", Vec::<PatchPayload>::new());
    })
}

async fn run(app: AppHandle, state: Arc<AppState>, generation: u64) -> anyhow::Result<()> {
    let mut capture = open_capture()
        .await
        .map_err(|e| anyhow::anyhow!("open capture: {e:#}"))?;

    if state.capture_generation.load(Ordering::SeqCst) != generation { capture.stop(); return Ok(()); }
    if let Some(w) = app.get_webview_window("render") { w.show()?; }
    let _ = app.emit("capture-state-changed", true);

    let min_hits = 3u32;

    let mut previous_pixels: Option<Arc<[u8]>> = None;
    let mut frame_index: u64 = 0;
    // Most recent detections, reused on the frames we don't run inference.
    let mut last_dets: Vec<DetBox> = Vec::new();
    // Auto-remove regions derived on the last detection tick; held on coast
    // frames so we keep covering them between inference passes.
    let mut auto_regions: Vec<NormalizedRegion> = Vec::new();

    while state.capture_running.load(Ordering::SeqCst) && state.capture_generation.load(Ordering::SeqCst) == generation {
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
        if state.capture_generation.load(Ordering::SeqCst) != generation { break; }
        if previous_pixels.as_ref().is_some_and(|pixels| Arc::ptr_eq(pixels, &view.pixels)) { continue; }
        previous_pixels = Some(view.pixels.clone());
        state.latest_frame.store(Some(Arc::new(view.clone())));
        frame_index += 1;
        let (w, h) = (view.width, view.height);

        // Detection + tracker + classify run only on detection TICKS (every Nth
        // frame). On the in-between frames the tracker would otherwise re-ingest
        // stale detections and corrupt its velocity/age/hits state, so we hold
        // the previously-derived `auto_regions` instead and just re-render. The
        // tracker's own predict step already coasts boxes forward, so a future
        // optimization can present `predicted_boxes` on coast frames.
        let detection_enabled = state.detection_enabled.load(Ordering::Relaxed);
        if !detection_enabled {
            auto_regions.clear();
            last_dets.clear();
            *state.tracker.lock() = Tracker::default();
        }
        let is_detection_tick = detection_enabled && (frame_index % DETECT_EVERY == 0 || frame_index == 1);

        if is_detection_tick {
            last_dets.clear();
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

            // --- 3. Classify using checksum-bound model semantics ---
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
                    let policy = state.detector.lock().as_ref().map(|d| d.class_policy(class_id))
                        .unwrap_or(ModelClassPolicy::Unknown);
                    let verdict = match policy {
                        ModelClassPolicy::Remove => Verdict::Remove,
                        ModelClassPolicy::Keep => Verdict::Keep,
                        ModelClassPolicy::Unknown => Verdict::Unsure,
                    };
                    tracker.set_verdict(id, verdict);
                }
            }

            // --- 4. Derive the auto-remove region list from confirmed tracks
            auto_regions = derive_auto_regions(&state, min_hits);
        }

        // --- 5. Merge with user-drawn regions -----------------------------
        let mut all_regions: Vec<NormalizedRegion> = state.region_store.current();
        // Clone (don't move) so `auto_regions` persists onto the next coast frame.
        all_regions.extend(auto_regions.iter().cloned());
        if all_regions.is_empty() {
            let had_patches = !state.current_patches.lock().is_empty();
            state.current_patches.lock().clear();
            if had_patches { let _ = app.emit("patches-updated", Vec::<PatchPayload>::new()); }
            continue;
        }

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

        if state.capture_generation.load(Ordering::SeqCst) != generation { break; }
        *state.current_patches.lock() = payloads.clone();
        let _ = app.emit("patches-updated", &payloads);
    }

    capture.stop();
    Ok(())
}

/// Cover confirmed removal tracks only when no protected track overlaps.
fn derive_auto_regions(
    state: &Arc<AppState>,
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
