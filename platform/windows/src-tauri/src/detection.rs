//! Object detection via ONNX Runtime + DirectML EP. Mirrors `VisionProcessor.swift`.
//!
//! Model semantics come from checksum-bound class metadata. Only explicit
//! ad/sponsor labels are eligible for automatic removal.
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
use std::path::Path;

use liveblock_detection::{yolo_input_size, letterbox_bgra, decode_yolov8, Detection, ModelClasses, ModelClassPolicy};

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
    input_size: u32, // square; 640 for yolov8n
    min_confidence: f32,
    iou_threshold: f32,
    classes: ModelClasses,
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
            .with_memory_pattern(false)
            .map_err(|e| anyhow!("disable DirectML memory patterns: {e}"))?
            .with_parallel_execution(false)
            .map_err(|e| anyhow!("configure sequential DirectML execution: {e}"))?
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

        let input_shape = session.inputs().first().context("model has no inputs")?
            .dtype().tensor_shape().context("model input is not a tensor")?;
        let input_size = yolo_input_size(&input_shape[..]).map_err(|e| anyhow!(e))?;

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

        // Reject stale class semantics when the installed weights change.
        let classes = ModelClasses::load(model_path);

        Ok(Self {
            session,
            input_name,
            output_name,
            input_size,
            min_confidence: 0.5,
            iou_threshold: 0.45,
            classes,
        })
    }

    /// Run inference on a packed BGRA frame. Returns normalized detections.
    pub fn detect(&mut self, bgra: &[u8], width: u32, height: u32) -> Result<Vec<DetBox>> {
        if width == 0 || height == 0 || (width as usize).checked_mul(height as usize).and_then(|n| n.checked_mul(4)).is_none_or(|n| bgra.len() < n) {
            return Ok(Vec::new());
        }

        let s = self.input_size as usize;
        let input = letterbox_bgra(bgra, width, height, self.input_size).map_err(|e| anyhow!(e))?;

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

        let kept = decode_yolov8(data, &shape[..], width, height, self.input_size,
            self.min_confidence, self.iou_threshold).map_err(|e| anyhow!(e))?;
        Ok(kept.into_iter().map(|det| {
            let class_name = self.classes.names.get(det.class_id as usize).cloned()
                .unwrap_or_else(|| format!("class_{}", det.class_id));
            DetBox { det, class_name }
        }).collect())
    }

    pub fn class_policy(&self, class_id: u32) -> ModelClassPolicy {
        self.classes.policy(class_id)
    }
}
