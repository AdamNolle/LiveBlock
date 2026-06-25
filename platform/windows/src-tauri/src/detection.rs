//! Object detection via ONNX Runtime + DirectML EP. Mirrors `VisionProcessor.swift`.
//!
//! Bundled model is the open-vocab `liveblock-detector.onnx`; class names come
//! from the shared `tools/vocab/liveblock-vocab.json` via `liveblock-config`.

use anyhow::{anyhow, Context, Result};
use ndarray::{Array, Array4, IxDyn};
use std::path::Path;

#[derive(Debug, Clone)]
pub struct DetBox {
    pub x: f32,      // pixels in source frame
    pub y: f32,
    pub width: f32,
    pub height: f32,
    pub class_id: u32,
    pub class_name: String,
    pub confidence: f32,
}

pub struct Detector {
    session: ort::Session,
    input_name: String,
    output_name: String,
    input_size: u32, // square; 640 for yolov8n
    min_confidence: f32,
    iou_threshold: f32,
    class_names: Vec<String>,
}

impl Detector {
    /// Load an ONNX model. Tries DirectML first, then CPU.
    pub fn load(model_path: &Path) -> Result<Self> {
        if !model_path.exists() {
            return Err(anyhow!(
                "Model not found at {}. Drop a liveblock-detector.onnx into src-tauri/resources/.",
                model_path.display()
            ));
        }

        let session = ort::Session::builder()?
            .with_execution_providers([
                ort::DirectMLExecutionProvider::default().build(),
                ort::CPUExecutionProvider::default().build(),
            ])?
            .with_optimization_level(ort::GraphOptimizationLevel::Level3)?
            .commit_from_file(model_path)
            .with_context(|| format!("load ONNX model {}", model_path.display()))?;

        let input_name = session
            .inputs
            .first()
            .map(|i| i.name.clone())
            .context("model has no inputs")?;
        let output_name = session
            .outputs
            .first()
            .map(|o| o.name.clone())
            .context("model has no outputs")?;

        // Open-vocab class names from the shared config, ordered so the Vec
        // index matches the model's class id.
        let class_names = vocab_class_names();

        Ok(Self {
            session,
            input_name,
            output_name,
            input_size: 640,
            min_confidence: 0.45,
            iou_threshold: 0.45,
            class_names,
        })
    }

    /// Run inference on a packed BGRA frame.
    pub fn detect(&mut self, bgra: &[u8], width: u32, height: u32) -> Result<Vec<DetBox>> {
        if width == 0 || height == 0 || bgra.len() < (width * height * 4) as usize {
            return Ok(Vec::new());
        }

        let s = self.input_size;
        // Letterbox: scale preserving aspect, pad to square 640.
        let scale = (s as f32 / width as f32).min(s as f32 / height as f32);
        let new_w = (width as f32 * scale).round() as u32;
        let new_h = (height as f32 * scale).round() as u32;
        let pad_x = ((s - new_w) / 2) as i32;
        let pad_y = ((s - new_h) / 2) as i32;

        let mut input: Array4<f32> = Array::zeros((1, 3, s as usize, s as usize));
        // Fill letterbox padding with mid-grey (114/255), Ultralytics convention.
        input.fill(114.0 / 255.0);

        // Nearest-neighbor resize + BGRA->RGB + normalize. Cheap, good enough for
        // detection input. `image` crate could do bilinear; skipping for hot-path speed.
        for ty in 0..new_h {
            let sy = (ty as f32 / scale) as u32;
            let sy = sy.min(height - 1);
            for tx in 0..new_w {
                let sx = (tx as f32 / scale) as u32;
                let sx = sx.min(width - 1);
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

        // Run.
        let inputs = ort::inputs![self.input_name.as_str() => input.view()]?;
        let outputs = self.session.run(inputs)?;
        let pred = outputs
            .get(self.output_name.as_str())
            .context("output missing")?
            .try_extract_tensor::<f32>()?;
        let view = pred.view().into_dyn();

        // YOLOv8 head: shape [1, 4 + nc, N] where nc=80, N=8400.
        let (nc, n_anchors) = match view.shape() {
            &[1, c, n] => (c - 4, n),
            other => {
                return Err(anyhow!("unexpected output shape {:?}", other));
            }
        };

        let mut boxes: Vec<DetBox> = Vec::new();
        for a in 0..n_anchors {
            // bbox in letterboxed pixels: cx, cy, w, h
            let cx = view[IxDyn(&[0, 0, a])];
            let cy = view[IxDyn(&[0, 1, a])];
            let w = view[IxDyn(&[0, 2, a])];
            let h = view[IxDyn(&[0, 3, a])];

            // Pick best class.
            let mut best_score: f32 = 0.0;
            let mut best_cls: usize = 0;
            for c in 0..nc {
                let s = view[IxDyn(&[0, 4 + c, a])];
                if s > best_score {
                    best_score = s;
                    best_cls = c;
                }
            }
            if best_score < self.min_confidence {
                continue;
            }

            // Un-letterbox.
            let lx = cx - w * 0.5;
            let ly = cy - h * 0.5;
            let px = (lx - pad_x as f32).max(0.0) / scale;
            let py = (ly - pad_y as f32).max(0.0) / scale;
            let pw = (w / scale).min(width as f32 - px);
            let ph = (h / scale).min(height as f32 - py);
            if pw <= 1.0 || ph <= 1.0 {
                continue;
            }

            let class_name = self
                .class_names
                .get(best_cls)
                .cloned()
                .unwrap_or_else(|| format!("class_{best_cls}"));
            boxes.push(DetBox {
                x: px,
                y: py,
                width: pw,
                height: ph,
                class_id: best_cls as u32,
                class_name,
                confidence: best_score,
            });
        }

        // NMS (per-class).
        Ok(non_max_suppression(boxes, self.iou_threshold))
    }
}

fn iou(a: &DetBox, b: &DetBox) -> f32 {
    let ax2 = a.x + a.width;
    let ay2 = a.y + a.height;
    let bx2 = b.x + b.width;
    let by2 = b.y + b.height;
    let ix1 = a.x.max(b.x);
    let iy1 = a.y.max(b.y);
    let ix2 = ax2.min(bx2);
    let iy2 = ay2.min(by2);
    let iw = (ix2 - ix1).max(0.0);
    let ih = (iy2 - iy1).max(0.0);
    let inter = iw * ih;
    let union = a.width * a.height + b.width * b.height - inter;
    if union <= 0.0 { 0.0 } else { inter / union }
}

fn non_max_suppression(mut boxes: Vec<DetBox>, iou_threshold: f32) -> Vec<DetBox> {
    boxes.sort_by(|a, b| b.confidence.partial_cmp(&a.confidence).unwrap_or(std::cmp::Ordering::Equal));
    let mut keep: Vec<DetBox> = Vec::new();
    while let Some(top) = boxes.first().cloned() {
        keep.push(top.clone());
        boxes.retain(|b| b.class_id != top.class_id || iou(&top, b) < iou_threshold);
        if boxes.first().map(|b| b.confidence).unwrap_or(0.0) >= top.confidence {
            // Reordered by retain; recheck order on next loop.
            boxes.sort_by(|a, b| b.confidence.partial_cmp(&a.confidence).unwrap_or(std::cmp::Ordering::Equal));
        }
        if boxes.first().map(|b| b.confidence < 0.0).unwrap_or(true) {
            break;
        }
        boxes.remove(0);
    }
    keep
}

/// Open-vocab class names from the shared `tools/vocab/liveblock-vocab.json`,
/// embedded at build time via `liveblock-config`. Classes are ordered by `id`
/// so the returned Vec's index equals the model's class id.
fn vocab_class_names() -> Vec<String> {
    const VOCAB_JSON: &str = include_str!("../../../../tools/vocab/liveblock-vocab.json");
    match liveblock_config::Vocabulary::from_json(VOCAB_JSON) {
        Ok(mut vocab) => {
            vocab.classes.sort_by_key(|c| c.id);
            vocab.classes.into_iter().map(|c| c.name).collect()
        }
        Err(e) => {
            tracing::error!("failed to parse bundled vocabulary: {e}");
            Vec::new()
        }
    }
}
