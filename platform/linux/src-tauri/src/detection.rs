//! ONNX Runtime YOLOv8 detection. EP picked at compile time via cargo features:
//! `cuda` / `rocm` / `openvino` / `tensorrt` / `cpu` (default).
//!
//! Pixel conversion, letterboxing, output decoding, filtering, and NMS are all
//! owned by `liveblock-detection`; this adapter only binds ONNX Runtime and
//! resolves class names.

use anyhow::{anyhow, Context, Result};
use liveblock_config::Vocabulary;
use liveblock_detection::{
    decode_yolov8_head, preprocess_bgra_letterbox, to_pixel_rect_top_left,
    DetectorProcessingContract,
};
use ndarray::Array4;
use ort::{Session, SessionBuilder, Value};
use std::path::Path;

#[cfg(any(
    all(feature = "cuda", feature = "rocm"),
    all(feature = "cuda", feature = "openvino"),
    all(feature = "cuda", feature = "tensorrt"),
    all(feature = "rocm", feature = "openvino"),
    all(feature = "rocm", feature = "tensorrt"),
    all(feature = "openvino", feature = "tensorrt"),
))]
compile_error!("select at most one optional Linux ONNX Runtime execution provider");

pub const MODEL_FILE_NAME: &str = "liveblock-detector.onnx";
const DEFAULT_VOCAB_JSON: &str = include_str!("../../../../tools/vocab/liveblock-vocab.json");

#[derive(Debug, Clone)]
pub struct DetBox {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
    pub class_id: u32,
    pub class_name: String,
    pub score: f32,
}

pub struct Detector {
    session: Session,
    input_name: String,
    output_name: String,
    processing: DetectorProcessingContract,
    score_threshold: f32,
    iou_threshold: f32,
    class_names: Vec<String>,
}

pub fn initialize_runtime(runtime_library: Option<&Path>) -> Result<()> {
    let Some(path) = runtime_library else {
        if cfg!(debug_assertions) {
            return Ok(());
        }
        return Err(anyhow!(
            "release package is missing resources/onnxruntime/libonnxruntime.so"
        ));
    };
    let metadata = std::fs::symlink_metadata(path)
        .with_context(|| format!("inspect packaged ONNX Runtime {}", path.display()))?;
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        return Err(anyhow!(
            "packaged ONNX Runtime must be a regular non-symlink file"
        ));
    }
    ort::init_from(path.to_string_lossy())
        .with_name("LiveBlock")
        .commit()
        .context("initialize packaged ONNX Runtime")?;
    Ok(())
}

pub fn configured_inference_backends() -> &'static [&'static str] {
    #[cfg(feature = "cuda")]
    return &["onnx_cuda_preferred_experimental", "onnx_cpu_fallback"];
    #[cfg(feature = "rocm")]
    return &["onnx_rocm_preferred_experimental", "onnx_cpu_fallback"];
    #[cfg(feature = "openvino")]
    return &["onnx_openvino_preferred_experimental", "onnx_cpu_fallback"];
    #[cfg(feature = "tensorrt")]
    return &["onnx_tensorrt_preferred_experimental", "onnx_cpu_fallback"];
    #[cfg(not(any(
        feature = "cuda",
        feature = "rocm",
        feature = "openvino",
        feature = "tensorrt"
    )))]
    return &["onnx_cpu"];
}

fn vocab_class_names() -> Vec<String> {
    Vocabulary::from_json(DEFAULT_VOCAB_JSON)
        .map(|vocabulary| vocabulary.class_names_by_id())
        .unwrap_or_default()
}

impl Detector {
    pub fn load(model_path: &Path) -> Result<Self> {
        let mut builder = Session::builder().context("ort SessionBuilder")?;
        builder = configure_execution_providers(builder)?;
        let session = builder
            .commit_from_file(model_path)
            .with_context(|| format!("load model {}", model_path.display()))?;
        let input_name = session
            .inputs
            .first()
            .map(|input| input.name.clone())
            .context("model has no inputs")?;
        let output_name = session
            .outputs
            .first()
            .map(|output| output.name.clone())
            .context("model has no outputs")?;

        Ok(Self {
            session,
            input_name,
            output_name,
            processing: DetectorProcessingContract::YOLOV8_BGRA_640,
            score_threshold: 0.35,
            iou_threshold: 0.45,
            class_names: vocab_class_names(),
        })
    }

    pub fn detect_bgra(&mut self, bgra: &[u8], width: u32, height: u32) -> Result<Vec<DetBox>> {
        let preprocessed = preprocess_bgra_letterbox(bgra, width, height, self.processing)
            .map_err(|error| anyhow!(error))?;
        let tensor = Array4::from_shape_vec(
            (
                1,
                self.processing.input_channels as usize,
                self.processing.input_height as usize,
                self.processing.input_width as usize,
            ),
            preprocessed.chw_rgb,
        )
        .context("build detector input tensor")?;
        let input = Value::from_array(tensor)?;
        let outputs = self
            .session
            .run(ort::inputs![self.input_name.as_str() => input]?)?;
        let raw_owned = outputs[self.output_name.as_str()].try_extract_tensor::<f32>()?;
        let raw = raw_owned.view().into_dyn();
        let (channels, anchors) = match raw.shape() {
            &[1, channels, anchors] => (channels, anchors),
            shape => return Err(anyhow!("unexpected YOLO output shape {shape:?}")),
        };
        let contiguous = raw
            .as_slice()
            .context("YOLO output tensor is not contiguous")?;
        let detections = decode_yolov8_head(
            contiguous,
            channels,
            anchors,
            preprocessed.transform,
            self.score_threshold,
            self.iou_threshold,
        )
        .map_err(|error| anyhow!(error))?;

        Ok(detections
            .into_iter()
            .map(|detection| {
                let rect = to_pixel_rect_top_left(&detection, width as f32, height as f32);
                DetBox {
                    x: rect.x,
                    y: rect.y,
                    w: rect.width,
                    h: rect.height,
                    class_id: detection.class_id,
                    class_name: self
                        .class_names
                        .get(detection.class_id as usize)
                        .filter(|name| !name.is_empty())
                        .cloned()
                        .unwrap_or_else(|| format!("class_{}", detection.class_id)),
                    score: detection.score,
                }
            })
            .collect())
    }

    pub fn set_thresholds(&mut self, score: f32, iou: f32) {
        self.score_threshold = score.clamp(0.05, 0.95);
        self.iou_threshold = iou.clamp(0.10, 0.90);
    }
}

#[cfg(feature = "cuda")]
fn configure_execution_providers(b: SessionBuilder) -> Result<SessionBuilder> {
    use ort::CUDAExecutionProvider;
    Ok(b.with_execution_providers([CUDAExecutionProvider::default().build()])?)
}

#[cfg(feature = "rocm")]
fn configure_execution_providers(b: SessionBuilder) -> Result<SessionBuilder> {
    use ort::ROCmExecutionProvider;
    Ok(b.with_execution_providers([ROCmExecutionProvider::default().build()])?)
}

#[cfg(feature = "openvino")]
fn configure_execution_providers(b: SessionBuilder) -> Result<SessionBuilder> {
    use ort::OpenVINOExecutionProvider;
    Ok(b.with_execution_providers([OpenVINOExecutionProvider::default().build()])?)
}

#[cfg(feature = "tensorrt")]
fn configure_execution_providers(b: SessionBuilder) -> Result<SessionBuilder> {
    use ort::TensorRTExecutionProvider;
    Ok(b.with_execution_providers([TensorRTExecutionProvider::default().build()])?)
}

#[cfg(not(any(
    feature = "cuda",
    feature = "rocm",
    feature = "openvino",
    feature = "tensorrt"
)))]
fn configure_execution_providers(b: SessionBuilder) -> Result<SessionBuilder> {
    Ok(b)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn source_debug_allows_explicit_system_runtime_fallback() {
        assert!(cfg!(debug_assertions));
        initialize_runtime(None).unwrap();
    }

    #[test]
    fn configured_backends_always_include_cpu() {
        assert!(configured_inference_backends()
            .iter()
            .any(|backend| backend.contains("cpu")));
    }
}
