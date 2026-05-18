//! ONNX Runtime YOLOv8 detection. EP picked at compile time via cargo features:
//! `cuda` / `rocm` / `openvino` / `tensorrt` / `cpu` (default). Uses the shared
//! `liveblock-detection` crate for filtering and NMS so the algorithm matches
//! the Windows port byte-for-byte.

use anyhow::{Context, Result};
use liveblock_detection::{filter_by_score, non_max_suppression, Detection};
use ndarray::{Array, Array4, IxDyn};
use ort::session::{Session, SessionBuilder};
use ort::value::Value;
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
}

impl Detector {
    pub fn load(model_path: &Path) -> Result<Self> {
        let mut builder = Session::builder().context("ort SessionBuilder")?;
        builder = configure_execution_providers(builder)?;

        let session = builder
            .commit_from_file(model_path)
            .with_context(|| format!("load model {}", model_path.display()))?;

        let input_name = session.inputs[0].name.clone();
        let output_name = session.outputs[0].name.clone();

        Ok(Self {
            session,
            input_name,
            output_name,
            input_size: 640,
            score_threshold: 0.35,
            iou_threshold: 0.45,
        })
    }

    /// Run a forward pass on a BGRA frame; returns post-processed boxes.
    pub fn detect_bgra(&mut self, bgra: &[u8], width: u32, height: u32) -> Result<Vec<DetBox>> {
        if width == 0 || height == 0 || bgra.len() < (width * height * 4) as usize {
            return Ok(Vec::new());
        }
        let (tensor, scale, pad_x, pad_y) =
            letterbox_to_tensor(bgra, width, height, self.input_size);
        let input = Value::from_array(tensor)?;
        let outputs = self
            .session
            .run(ort::inputs![self.input_name.as_str() => input]?)?;
        let raw_owned = outputs[self.output_name.as_str()].try_extract_tensor::<f32>()?;
        let raw = raw_owned.view().into_dyn();
        Ok(decode_yolov8_head(
            raw,
            width as f32,
            height as f32,
            scale,
            pad_x,
            pad_y,
            self.score_threshold,
            self.iou_threshold,
        ))
    }

    pub fn set_thresholds(&mut self, score: f32, iou: f32) {
        self.score_threshold = score.clamp(0.05, 0.95);
        self.iou_threshold = iou.clamp(0.10, 0.90);
    }
}

#[cfg(feature = "cuda")]
fn configure_execution_providers(b: SessionBuilder) -> Result<SessionBuilder> {
    use ort::execution_providers::CUDAExecutionProvider;
    Ok(b.with_execution_providers([CUDAExecutionProvider::default().build()])?)
}

#[cfg(feature = "rocm")]
fn configure_execution_providers(b: SessionBuilder) -> Result<SessionBuilder> {
    use ort::execution_providers::ROCmExecutionProvider;
    Ok(b.with_execution_providers([ROCmExecutionProvider::default().build()])?)
}

#[cfg(feature = "openvino")]
fn configure_execution_providers(b: SessionBuilder) -> Result<SessionBuilder> {
    use ort::execution_providers::OpenVINOExecutionProvider;
    Ok(b.with_execution_providers([OpenVINOExecutionProvider::default().build()])?)
}

#[cfg(feature = "tensorrt")]
fn configure_execution_providers(b: SessionBuilder) -> Result<SessionBuilder> {
    use ort::execution_providers::TensorRTExecutionProvider;
    Ok(b.with_execution_providers([TensorRTExecutionProvider::default().build()])?)
}

#[cfg(not(any(feature = "cuda", feature = "rocm", feature = "openvino", feature = "tensorrt")))]
fn configure_execution_providers(b: SessionBuilder) -> Result<SessionBuilder> {
    Ok(b)
}

/// Letterbox-resize a BGRA frame to the model's square input tensor (CHW
/// float, mid-grey padded).
///
/// Returns `(tensor, scale, pad_x, pad_y)` so the head decoder can reverse
/// the letterbox into source-image pixel coordinates.
fn letterbox_to_tensor(
    bgra: &[u8],
    width: u32,
    height: u32,
    size: u32,
) -> (Array4<f32>, f32, i32, i32) {
    let s = size;
    let scale = (s as f32 / width as f32).min(s as f32 / height as f32);
    let new_w = (width as f32 * scale).round() as u32;
    let new_h = (height as f32 * scale).round() as u32;
    let pad_x = ((s - new_w) / 2) as i32;
    let pad_y = ((s - new_h) / 2) as i32;

    // Mid-grey 114/255 padding — Ultralytics convention.
    let mut input: Array4<f32> = Array::from_elem((1, 3, s as usize, s as usize), 114.0 / 255.0);

    // Nearest-neighbor resize + BGRA→RGB normalize. Bilinear would be cleaner
    // but the perf cost on a 60+ Hz hot path doesn't justify it here.
    for ty in 0..new_h {
        let sy = ((ty as f32 / scale) as u32).min(height - 1);
        for tx in 0..new_w {
            let sx = ((tx as f32 / scale) as u32).min(width - 1);
            let i = ((sy * width + sx) * 4) as usize;
            let b = bgra[i] as f32 / 255.0;
            let g = bgra[i + 1] as f32 / 255.0;
            let r = bgra[i + 2] as f32 / 255.0;
            let dx = (tx as i32 + pad_x) as usize;
            let dy = (ty as i32 + pad_y) as usize;
            input[[0, 0, dy, dx]] = r;
            input[[0, 1, dy, dx]] = g;
            input[[0, 2, dy, dx]] = b;
        }
    }

    (input, scale, pad_x, pad_y)
}

/// Decode YOLOv8 head output `[1, 4 + nc, N]` into pixel-space `DetBox`.
/// Filters by score and runs class-aware NMS via the shared
/// `liveblock-detection` crate (so the algorithm matches Windows / future macOS).
#[allow(clippy::too_many_arguments)]
fn decode_yolov8_head(
    raw: ndarray::ArrayViewD<'_, f32>,
    src_w: f32,
    src_h: f32,
    scale: f32,
    pad_x: i32,
    pad_y: i32,
    score_threshold: f32,
    iou_threshold: f32,
) -> Vec<DetBox> {
    let (nc, n_anchors) = match raw.shape() {
        &[1, c, n] => (c - 4, n),
        _ => return Vec::new(),
    };

    let mut normalized: Vec<Detection> = Vec::with_capacity(n_anchors);
    for a in 0..n_anchors {
        let cx = raw[IxDyn(&[0, 0, a])];
        let cy = raw[IxDyn(&[0, 1, a])];
        let w = raw[IxDyn(&[0, 2, a])];
        let h = raw[IxDyn(&[0, 3, a])];

        let mut best_score: f32 = 0.0;
        let mut best_cls: usize = 0;
        for c in 0..nc {
            let s = raw[IxDyn(&[0, 4 + c, a])];
            if s > best_score {
                best_score = s;
                best_cls = c;
            }
        }

        // Un-letterbox into source-image pixel coords, then normalize.
        let lx = cx - w * 0.5;
        let ly = cy - h * 0.5;
        let px = (lx - pad_x as f32).max(0.0) / scale;
        let py = (ly - pad_y as f32).max(0.0) / scale;
        let pw = (w / scale).min(src_w - px);
        let ph = (h / scale).min(src_h - py);
        if pw <= 1.0 || ph <= 1.0 {
            continue;
        }
        normalized.push(Detection::new(
            best_cls as u32,
            best_score,
            px / src_w,
            py / src_h,
            pw / src_w,
            ph / src_h,
        ));
    }

    let filtered = filter_by_score(&normalized, score_threshold);
    let kept = non_max_suppression(&filtered, iou_threshold);

    kept.into_iter()
        .map(|d| DetBox {
            x: d.x * src_w,
            y: d.y * src_h,
            w: d.width * src_w,
            h: d.height * src_h,
            class_id: d.class_id,
            score: d.score,
        })
        .collect()
}
