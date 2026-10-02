//! ONNX Runtime YOLOv8 detection. EP picked at compile time via cargo features:
//! `cuda` / `rocm` / `openvino` / `tensorrt` / `cpu` (default). Uses the shared
//! `liveblock-detection` crate for filtering and NMS so the algorithm matches
//! the Windows port byte-for-byte.

use anyhow::{anyhow, Context, Result};
use liveblock_detection::{yolo_input_size, letterbox_bgra, decode_yolov8, ModelClasses, ModelClassPolicy};
use ndarray::Array4;
use ort::session::{Session, builder::SessionBuilder};
use ort::value::Tensor;
use std::path::Path;

#[derive(Debug, Clone, Copy)]
pub struct DetBox {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
    pub class_id: u32,
    pub score: f32,
}

pub struct Detector {
    session: Session,
    input_name: String,
    output_name: String,
    input_size: u32,
    score_threshold: f32,
    iou_threshold: f32,
    classes: ModelClasses,
}

impl Detector {
    pub fn load(model_path: &Path) -> Result<Self> {
        let mut builder = Session::builder().context("ort SessionBuilder")?;
        builder = configure_execution_providers(builder)?;

        let session = builder
            .commit_from_file(model_path)
            .with_context(|| format!("load model {}", model_path.display()))?;

        let input_shape = session.inputs().first().context("model has no inputs")?
            .dtype().tensor_shape().context("model input is not a tensor")?;
        let input_size = yolo_input_size(&input_shape[..]).map_err(|e| anyhow!(e))?;

        let input_name = session.inputs().first().context("model has no inputs")?.name().to_string();
        let output_name = session.outputs().first().context("model has no outputs")?.name().to_string();

        Ok(Self {
            session,
            input_name,
            output_name,
            input_size,
            score_threshold: 0.5,
            iou_threshold: 0.45,
            classes: ModelClasses::load(model_path),
        })
    }

    /// Run a forward pass on a BGRA frame; returns post-processed boxes.
    pub fn detect_bgra(&mut self, bgra: &[u8], width: u32, height: u32) -> Result<Vec<DetBox>> {
        if width == 0 || height == 0 || (width as usize).checked_mul(height as usize).and_then(|n| n.checked_mul(4)).is_none_or(|n| bgra.len() < n) {
            return Ok(Vec::new());
        }
        let pixels = letterbox_bgra(bgra, width, height, self.input_size).map_err(|e| anyhow!(e))?;
        let size = self.input_size as usize;
        let tensor = Array4::from_shape_vec((1,3,size,size), pixels)?;
        let input = Tensor::from_array(tensor)?;
        let outputs = self.session.run(ort::inputs![self.input_name.as_str() => input])?;
        let (shape, raw) = outputs[self.output_name.as_str()].try_extract_tensor::<f32>()?;
        let decoded = decode_yolov8(raw, &shape[..], width, height, self.input_size,
            self.score_threshold, self.iou_threshold).map_err(|e| anyhow!(e))?;
        Ok(decoded.into_iter().map(|d| DetBox {
            x: d.x * width as f32, y: d.y * height as f32,
            w: d.width * width as f32, h: d.height * height as f32,
            class_id: d.class_id, score: d.score,
        }).collect())
    }

    pub fn class_policy(&self, class_id: u32) -> ModelClassPolicy {
        self.classes.policy(class_id)
    }

    pub fn set_thresholds(&mut self, score: f32, iou: f32) {
        self.score_threshold = score.clamp(0.05, 0.95);
        self.iou_threshold = iou.clamp(0.10, 0.90);
    }
}

#[cfg(feature = "cuda")]
fn configure_execution_providers(b: SessionBuilder) -> Result<SessionBuilder> {
    use ort::ep::CUDA;
    b.with_execution_providers([CUDA::default().build()]).map_err(|e| anyhow!("{e}"))
}

#[cfg(feature = "rocm")]
fn configure_execution_providers(b: SessionBuilder) -> Result<SessionBuilder> {
    use ort::ep::ROCm;
    b.with_execution_providers([ROCm::default().build()]).map_err(|e| anyhow!("{e}"))
}

#[cfg(feature = "openvino")]
fn configure_execution_providers(b: SessionBuilder) -> Result<SessionBuilder> {
    use ort::ep::OpenVINO;
    b.with_execution_providers([OpenVINO::default().build()]).map_err(|e| anyhow!("{e}"))
}

#[cfg(feature = "tensorrt")]
fn configure_execution_providers(b: SessionBuilder) -> Result<SessionBuilder> {
    use ort::ep::TensorRT;
    b.with_execution_providers([TensorRT::default().build()]).map_err(|e| anyhow!("{e}"))
}

#[cfg(not(any(feature = "cuda", feature = "rocm", feature = "openvino", feature = "tensorrt")))]
fn configure_execution_providers(b: SessionBuilder) -> Result<SessionBuilder> {
    Ok(b)
}
