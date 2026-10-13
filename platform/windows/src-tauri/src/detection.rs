//! Object detection via ONNX Runtime + DirectML EP. Mirrors `VisionProcessor.swift`.
//!
//! Bundled model is COCO-trained yolov8n until the user trains a logo model
//! via `tools/train_logos.py` and exports to ONNX.
//!
//! ## ort rc.12 API
//! This file targets `ort = "=2.0.0-rc.12"` (the version pinned in Cargo.lock):
//!   * sessions live under `ort::session::{Session, builder::GraphOptimizationLevel}`,
//!   * execution providers live under `ort::ep::{DirectML, CPU}` (with `.build()`),
//!   * `Session::inputs()` / `outputs()` are *methods* returning `&[Outlet]`,
//!   * `Session::run` takes `&mut self`,
//!   * `inputs!` no longer returns a `Result`,
//!   * tensor extraction returns `(&Shape, &[T])` — no ndarray bridge needed.
//!
//! ## Output type
//! `detect()` returns shared-core [`liveblock_detection::Detection`]s in
//! **normalized [0..1] top-left** coordinates so the rest of the pipeline
//! (Tracker -> decide_verdict -> build_remove_mask_from_tracks) consumes them
//! verbatim. NMS is the shared-core class-aware implementation; the old broken
//! `remove(0)` loop is gone.

use anyhow::{anyhow, Context, Result};
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::path::Path;

use liveblock_detection::{filter_by_score, non_max_suppression, Detection};

use ort::ep::{CPUExecutionProvider, DirectMLExecutionProvider};
use ort::session::{builder::GraphOptimizationLevel, Session};
use ort::value::Tensor;

/// A scored, normalized detection plus its human-readable class name. Thin
/// wrapper over the shared-core [`Detection`] so callers that only need
/// geometry/score/class_id can `.det`-project, while UI/labeling can show the
/// name. Coordinates are normalized [0..1] top-left.
///
/// `class_name` and the convenience accessors are part of the labeling/overlay
/// UI surface (not yet consumed by the realtime worker, which projects `.det`).
#[derive(Debug, Clone)]
pub struct DetBox {
    pub det: Detection,
    #[allow(dead_code)]
    pub class_name: String,
}

#[allow(dead_code)]
impl DetBox {
    #[inline]
    pub fn class_id(&self) -> u32 {
        self.det.class_id
    }
    #[inline]
    pub fn confidence(&self) -> f32 {
        self.det.score
    }
}

pub struct Detector {
    session: Session,
    input_name: String,
    output_name: String,
    input_size: u32,
    min_confidence: f32,
    iou_threshold: f32,
    class_names: Vec<String>,
    auto_remove_ids: Vec<u32>,
    keep_ids: Vec<u32>,
}

#[derive(Deserialize)]
struct ModelManifest {
    schema_version: u32,
    validated: bool,
    classes: Vec<String>,
    auto_block_labels: Vec<String>,
    precision: f32,
    map50: f32,
    onnx_sha256: Option<String>,
    #[serde(default = "default_input_size")]
    input_size: u32,
}

fn default_input_size() -> u32 { 640 }

fn matching_manifest(model_path: &Path) -> Option<ModelManifest> {
    let path = model_path.with_file_name("yolov8n.manifest.json");
    let manifest: ModelManifest = serde_json::from_slice(&std::fs::read(path).ok()?).ok()?;
    if manifest.schema_version != 1
        || !manifest.precision.is_finite() || !manifest.map50.is_finite()
        || !(320..=1280).contains(&manifest.input_size)
    {
        return None;
    }
    let expected = manifest.onnx_sha256.as_ref()?;
    let actual = format!("{:x}", Sha256::digest(std::fs::read(model_path).ok()?));
    (actual == *expected).then_some(manifest)
}

impl Detector {
    /// Load an ONNX model. Tries DirectML (Windows GPU) first, then CPU.
    pub fn load(model_path: &Path) -> Result<Self> {
        if !model_path.exists() {
            return Err(anyhow!(
                "Model not found at {}. Drop a yolov8n.onnx (or your trained logo model) into src-tauri/resources/.",
                model_path.display()
            ));
        }

        // rc.12 builder chaining: each `with_*` consumes and returns the builder
        // (wrapped in `Result<SessionBuilder, Error<SessionBuilder>>`), and
        // `commit_from_file` takes `&mut self`. The `Error<SessionBuilder>`
        // recover-error is NOT Send+Sync (it carries the builder), so it can't
        // flow through `?` into anyhow — flatten it to a string-backed error.
        let mut builder = Session::builder()?
            .with_execution_providers([
                DirectMLExecutionProvider::default().build(),
                CPUExecutionProvider::default().build(),
            ])
            .map_err(|e| anyhow!("with_execution_providers: {e}"))?
            .with_optimization_level(GraphOptimizationLevel::Level3)
            .map_err(|e| anyhow!("with_optimization_level: {e}"))?;
        let session = builder
            .commit_from_file(model_path)
            .with_context(|| format!("load ONNX model {}", model_path.display()))?;

        let input_name = session
            .inputs()
            .first()
            .map(|i| i.name().to_string())
            .context("model has no inputs")?;
        let output_name = session
            .outputs()
            .first()
            .map(|o| o.name().to_string())
            .context("model has no outputs")?;

        // A checksum-matched but not-yet-validated model may be loaded for
        // visible detection/label suggestions. It receives no erase or keep
        // authority unless its quality gate passed.
        let manifest = matching_manifest(model_path);
        let input_size = manifest.as_ref().map_or(640, |m| m.input_size);
        let class_names = manifest.as_ref().map(|m| m.classes.clone())
            .unwrap_or_else(coco_class_names);
        let mut auto_remove_ids = Vec::new();
        let mut keep_ids = Vec::new();
        if let Some(manifest) = &manifest {
            let has_keeps = class_names.iter().any(|name| name == "team_keep")
                && class_names.iter().any(|name| name == "number_keep");
            for (id, name) in class_names.iter().enumerate() {
                if manifest.validated && (name == "team_keep" || name == "number_keep") {
                    keep_ids.push(id as u32);
                }
                if manifest.validated && manifest.precision >= 0.8 && manifest.map50 >= 0.5
                    && (name == "ad" || (name == "sponsor_remove" && has_keeps))
                    && manifest.auto_block_labels.contains(name)
                {
                    auto_remove_ids.push(id as u32);
                }
            }
        }

        Ok(Self {
            session,
            input_name,
            output_name,
            input_size,
            min_confidence: 0.45,
            iou_threshold: 0.45,
            class_names,
            auto_remove_ids,
            keep_ids,
        })
    }

    pub fn class_is_auto_removable(&self, class_id: u32) -> bool {
        self.auto_remove_ids.contains(&class_id)
    }

    pub fn class_is_keep(&self, class_id: u32) -> bool {
        self.keep_ids.contains(&class_id)
    }

    /// Run inference on a packed BGRA frame. Returns normalized detections.
    pub fn detect(&mut self, bgra: &[u8], width: u32, height: u32) -> Result<Vec<DetBox>> {
        if width == 0 || height == 0 || bgra.len() < (width * height * 4) as usize {
            return Ok(Vec::new());
        }

        let s = self.input_size as usize;
        let sf = self.input_size as f32;
        // Letterbox: scale preserving aspect, pad to square `s`.
        let scale = (sf / width as f32).min(sf / height as f32);
        let new_w = (width as f32 * scale).round() as u32;
        let new_h = (height as f32 * scale).round() as u32;
        let pad_x = ((self.input_size - new_w) / 2) as i32;
        let pad_y = ((self.input_size - new_h) / 2) as i32;

        // CHW f32 input buffer. Fill letterbox padding with mid-grey (114/255),
        // the Ultralytics convention. Layout: [1, 3, s, s].
        let plane = s * s;
        let mut input = vec![114.0f32 / 255.0; 3 * plane];

        // Nearest-neighbor resize + BGRA->RGB + normalize. Cheap; good enough for
        // detection input. (Bilinear via the `image` crate is a later refinement.)
        for ty in 0..new_h {
            let sy = ((ty as f32 / scale) as u32).min(height - 1);
            let dy = (ty as i32 + pad_y) as usize;
            if dy >= s {
                continue;
            }
            for tx in 0..new_w {
                let sx = ((tx as f32 / scale) as u32).min(width - 1);
                let dx = (tx as i32 + pad_x) as usize;
                if dx >= s {
                    continue;
                }
                let i = ((sy * width + sx) * 4) as usize;
                let b = bgra[i] as f32 / 255.0;
                let g = bgra[i + 1] as f32 / 255.0;
                let r = bgra[i + 2] as f32 / 255.0;
                let off = dy * s + dx;
                input[off] = r; // channel 0 (R)
                input[plane + off] = g; // channel 1 (G)
                input[2 * plane + off] = b; // channel 2 (B)
            }
        }

        // Build an owned tensor from `(shape, data)` — no ndarray feature needed.
        let tensor = Tensor::from_array((vec![1i64, 3, s as i64, s as i64], input))
            .context("build input tensor")?;

        // Run. rc.12 `inputs!` builds the value array directly (no Result).
        let outputs = self
            .session
            .run(ort::inputs![self.input_name.as_str() => tensor])
            .context("session.run")?;

        let value = outputs
            .get(self.output_name.as_str())
            .context("output missing")?;
        let (shape, data) = value
            .try_extract_tensor::<f32>()
            .context("extract output tensor")?;

        // YOLOv8 head: shape [1, 4 + nc, N] where nc=80, N=8400. `Shape` derefs
        // to `[i64]` in rc.12 (`shape` here is `&Shape`).
        let dims: &[i64] = &shape[..];
        let (channels, n_anchors) = match dims {
            [1, c, n] => (*c as usize, *n as usize),
            other => return Err(anyhow!("unexpected output shape {:?}", other)),
        };
        if channels < 5 {
            return Err(anyhow!("output channels {channels} < 5; not a YOLO head"));
        }
        let nc = channels - 4;

        // Helper to index the row-major [channels, n_anchors] block (batch 1).
        let at = |c: usize, a: usize| -> f32 { data[c * n_anchors + a] };

        let mut raw: Vec<DetBox> = Vec::new();
        for a in 0..n_anchors {
            // bbox in letterboxed pixels: cx, cy, w, h.
            let cx = at(0, a);
            let cy = at(1, a);
            let w = at(2, a);
            let h = at(3, a);

            // Pick best class.
            let mut best_score: f32 = 0.0;
            let mut best_cls: usize = 0;
            for c in 0..nc {
                let sc = at(4 + c, a);
                if sc > best_score {
                    best_score = sc;
                    best_cls = c;
                }
            }
            if best_score < self.min_confidence {
                continue;
            }

            // Un-letterbox to source-frame pixels.
            let lx = cx - w * 0.5;
            let ly = cy - h * 0.5;
            let px = (lx - pad_x as f32).max(0.0) / scale;
            let py = (ly - pad_y as f32).max(0.0) / scale;
            let pw = (w / scale).min(width as f32 - px);
            let ph = (h / scale).min(height as f32 - py);
            if pw <= 1.0 || ph <= 1.0 {
                continue;
            }

            // Normalize to [0..1] top-left for the shared-core pipeline.
            let det = Detection::new(
                best_cls as u32,
                best_score,
                px / width as f32,
                py / height as f32,
                pw / width as f32,
                ph / height as f32,
            );
            let class_name = self
                .class_names
                .get(best_cls)
                .cloned()
                .unwrap_or_else(|| format!("class_{best_cls}"));
            raw.push(DetBox { det, class_name });
        }

        // Score filter + class-aware NMS via the shared-core implementation
        // (replaces the previous broken `remove(0)` loop). Operate on plain
        // `Detection`s, then re-attach class names.
        let dets: Vec<Detection> = raw.iter().map(|d| d.det.clone()).collect();
        let dets = filter_by_score(&dets, self.min_confidence);
        let kept = non_max_suppression(&dets, self.iou_threshold);

        let out = kept
            .into_iter()
            .map(|det| {
                let class_name = self
                    .class_names
                    .get(det.class_id as usize)
                    .cloned()
                    .unwrap_or_else(|| format!("class_{}", det.class_id));
                DetBox { det, class_name }
            })
            .collect();
        Ok(out)
    }
}

/// SAFE CLASSIFIER STUB — empty sponsor/remove allowlist.
///
/// The bundled model is generic COCO (person, car, tv, ...). Auto-erasing any
/// of those is the confirmed "inpaints people/cars" bug. Until the real
/// sponsor-vs-team/number classifier (liveblock-core `decide_verdict` fed by
/// real `ClassifySignals`) is wired with a logo-trained model, NO COCO class is
/// on the remove allowlist, so detections never auto-promote to `Verdict::Remove`
/// — they stay `Unsure` in the tracker and erase nothing. User-drawn regions are
/// unaffected (they paint/inpaint as before).
///
/// TODO(windows-port): when a logo/sponsor model is bundled, populate this with
/// the sponsor class ids (or switch to gathering real `ClassifySignals` per
/// track and calling `liveblock_core::decide_verdict`).
fn coco_class_names() -> Vec<String> {
    [
        "person","bicycle","car","motorcycle","airplane","bus","train","truck","boat",
        "traffic light","fire hydrant","stop sign","parking meter","bench","bird","cat",
        "dog","horse","sheep","cow","elephant","bear","zebra","giraffe","backpack",
        "umbrella","handbag","tie","suitcase","frisbee","skis","snowboard","sports ball",
        "kite","baseball bat","baseball glove","skateboard","surfboard","tennis racket",
        "bottle","wine glass","cup","fork","knife","spoon","bowl","banana","apple",
        "sandwich","orange","broccoli","carrot","hot dog","pizza","donut","cake","chair",
        "couch","potted plant","bed","dining table","toilet","tv","laptop","mouse",
        "remote","keyboard","cell phone","microwave","oven","toaster","sink","refrigerator",
        "book","clock","vase","scissors","teddy bear","hair drier","toothbrush",
    ]
    .iter().map(|s| s.to_string()).collect()
}

#[cfg(test)]
mod manifest_tests {
    use super::*;

    #[test]
    fn manifest_only_authorizes_matching_model_bytes() {
        let dir = std::env::temp_dir().join(format!("liveblock-manifest-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir(&dir).unwrap();
        let model = dir.join("yolov8n.onnx");
        std::fs::write(&model, b"trained").unwrap();
        let digest = format!("{:x}", Sha256::digest(b"trained"));
        let manifest = serde_json::json!({
            "schema_version": 1, "validated": true,
            "classes": ["ad", "sponsor_remove", "team_keep", "number_keep"],
            "auto_block_labels": ["ad", "sponsor_remove"],
            "precision": 0.9, "map50": 0.8, "onnx_sha256": digest,
        });
        std::fs::write(dir.join("yolov8n.manifest.json"), manifest.to_string()).unwrap();
        assert!(matching_manifest(&model).is_some());
        std::fs::write(&model, b"different model").unwrap();
        assert!(matching_manifest(&model).is_none());
        std::fs::remove_dir_all(dir).unwrap();
    }
}
