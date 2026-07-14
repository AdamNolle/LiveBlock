//! Object detection via ONNX Runtime with DirectML and CPU fallback.
//!
//! Pixel conversion, letterboxing, output decoding, filtering, and NMS are
//! shared with Linux through `liveblock-detection`; this adapter only binds ORT
//! and resolves runtime class names.

use anyhow::{anyhow, Context, Result};
use liveblock_config::Vocabulary;
use liveblock_detection::{
    decode_yolov8_head, preprocess_bgra_letterbox, to_pixel_rect_top_left,
    DetectorProcessingContract,
};
use ndarray::Array4;
use std::path::Path;

const DEFAULT_VOCAB_JSON: &str = include_str!("../../../../tools/vocab/liveblock-vocab.json");

#[derive(Debug, Clone)]
pub struct DetBox {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
    pub class_id: u32,
    pub class_name: String,
    pub confidence: f32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DetectorBackend {
    DirectMlRegisteredCpuInput,
    CpuAfterDirectMlLoadFailure,
}

impl DetectorBackend {
    fn status(self) -> &'static str {
        match self {
            Self::DirectMlRegisteredCpuInput => "directml_registered_cpu_uploaded_tensor",
            Self::CpuAfterDirectMlLoadFailure => "cpu_after_directml_load_failure",
        }
    }
}

pub struct Detector {
    session: ort::Session,
    backend: DetectorBackend,
    input_name: String,
    output_name: String,
    processing: DetectorProcessingContract,
    min_confidence: f32,
    iou_threshold: f32,
    class_names: Vec<String>,
}

impl Detector {
    /// Load an ONNX model. DirectML is preferred; CPU remains the fallback.
    pub fn load(model_path: &Path) -> Result<Self> {
        if !model_path.exists() {
            return Err(anyhow!("model not found: {}", model_path.display()));
        }
        // ORT's DirectML contract requires sequential execution and disabled
        // memory patterns. Make registration failure observable instead of
        // silently claiming DirectML, then construct a separate CPU session.
        // Inputs remain CPU-preprocessed NCHW tensors; D3D texture binding is
        // deliberately not claimed by this path.
        let directml = ort::Session::builder()
            .and_then(|builder| {
                builder.with_execution_providers([ort::DirectMLExecutionProvider::default()
                    .build()
                    .error_on_failure()])
            })
            .and_then(|builder| builder.with_parallel_execution(false))
            .and_then(|builder| builder.with_memory_pattern(false))
            .and_then(|builder| {
                builder.with_optimization_level(ort::GraphOptimizationLevel::Level3)
            })
            .and_then(|builder| builder.commit_from_file(model_path));
        let (session, backend) = match directml {
            Ok(session) => (session, DetectorBackend::DirectMlRegisteredCpuInput),
            Err(directml_error) => {
                tracing::warn!(
                    "DirectML session load failed; constructing explicit CPU fallback: {directml_error}"
                );
                let session = ort::Session::builder()?
                    .with_execution_providers([
                        ort::CPUExecutionProvider::default().build().error_on_failure(),
                    ])?
                    .with_optimization_level(ort::GraphOptimizationLevel::Level3)?
                    .commit_from_file(model_path)
                    .with_context(|| {
                        format!(
                            "DirectML load failed ({directml_error}); CPU fallback could not load ONNX model {}",
                            model_path.display()
                        )
                    })?;
                (session, DetectorBackend::CpuAfterDirectMlLoadFailure)
            }
        };
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
        let class_names = Vocabulary::from_json(DEFAULT_VOCAB_JSON)
            .map(|vocabulary| vocabulary.class_names_by_id())
            .unwrap_or_default();

        Ok(Self {
            session,
            backend,
            input_name,
            output_name,
            processing: DetectorProcessingContract::YOLOV8_BGRA_640,
            min_confidence: 0.45,
            iou_threshold: 0.45,
            class_names,
        })
    }

    pub fn detect(
        &mut self,
        bgra: &[u8],
        width: u32,
        height: u32,
        run_options: &ort::RunOptions,
    ) -> Result<Vec<DetBox>> {
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
        let inputs = ort::inputs![self.input_name.as_str() => tensor.view()]?;
        let outputs = self.session.run_with_options(inputs, run_options)?;
        let prediction = outputs
            .get(self.output_name.as_str())
            .context("model output missing")?
            .try_extract_tensor::<f32>()?;
        let raw = prediction.view().into_dyn();
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
            self.min_confidence,
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
                    width: rect.width,
                    height: rect.height,
                    class_id: detection.class_id,
                    class_name: self
                        .class_names
                        .get(detection.class_id as usize)
                        .filter(|name| !name.is_empty())
                        .cloned()
                        .unwrap_or_else(|| format!("class_{}", detection.class_id)),
                    confidence: detection.score,
                }
            })
            .collect())
    }

    /// Registration/load truth only. This does not prove that any graph node
    /// executed on a physical GPU, and both statuses consume CPU-uploaded NCHW.
    pub fn backend_status(&self) -> &'static str {
        self.backend.status()
    }

    pub fn set_thresholds(&mut self, score: f32, iou: f32) {
        self.min_confidence = score.clamp(0.05, 0.95);
        self.iou_threshold = iou.clamp(0.10, 0.90);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn backend_status_never_claims_texture_binding_or_gpu_execution() {
        assert_eq!(
            DetectorBackend::DirectMlRegisteredCpuInput.status(),
            "directml_registered_cpu_uploaded_tensor"
        );
        assert_eq!(
            DetectorBackend::CpuAfterDirectMlLoadFailure.status(),
            "cpu_after_directml_load_failure"
        );
        for status in [
            DetectorBackend::DirectMlRegisteredCpuInput.status(),
            DetectorBackend::CpuAfterDirectMlLoadFailure.status(),
        ] {
            assert!(!status.contains("texture"));
            assert!(!status.contains("executed"));
            assert!(!status.contains("zero_copy"));
        }
    }
}
