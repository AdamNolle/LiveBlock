//! YOLO-style post-processing utilities.
//!
//! Pure compute, no platform deps. Operates on plain `Detection` structs in
//! normalized [0..1] coordinates with origin top-left. Helpers convert to/from
//! pixel coordinates and between top-left and bottom-left origins (the macOS
//! Vision / CoreVideo distinction).

use serde::{Deserialize, Serialize};
use thiserror::Error;

#[derive(Debug, Error, PartialEq, Eq)]
pub enum DetectionContractError {
    #[error("invalid packed BGRA frame")]
    InvalidFrame,
    #[error("invalid detector input dimensions")]
    InvalidInputDimensions,
    #[error("invalid YOLOv8 head shape")]
    InvalidHeadShape,
}

/// Immutable preprocessing/postprocessing contract shared by CoreML/ONNX
/// adapters. Runtime classes are defined by `liveblock-config`; this structure
/// fixes the pixel and tensor semantics that must remain equivalent.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DetectorProcessingContract {
    pub version: u32,
    pub input_width: u32,
    pub input_height: u32,
    pub input_channels: u32,
    pub padding_value: u8,
    pub class_aware_nms: bool,
}

impl DetectorProcessingContract {
    pub const YOLOV8_BGRA_640: Self = Self {
        version: 1,
        input_width: 640,
        input_height: 640,
        input_channels: 3,
        padding_value: 114,
        class_aware_nms: true,
    };

    pub fn validate(&self) -> Result<(), DetectionContractError> {
        if self.version != 1
            || self.input_width == 0
            || self.input_height == 0
            || self.input_channels != 3
            || !self.class_aware_nms
        {
            return Err(DetectionContractError::InvalidInputDimensions);
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LetterboxTransform {
    pub source_width: u32,
    pub source_height: u32,
    pub input_width: u32,
    pub input_height: u32,
    pub resized_width: u32,
    pub resized_height: u32,
    pub scale: f32,
    pub pad_x: u32,
    pub pad_y: u32,
}

#[derive(Debug, Clone, PartialEq)]
pub struct PreprocessedInput {
    /// Contiguous NCHW RGB float data for a single image, normalized to [0,1].
    pub chw_rgb: Vec<f32>,
    pub transform: LetterboxTransform,
}

/// Nearest-neighbor letterbox from packed BGRA8 to one normalized NCHW RGB
/// tensor. Padding is 114/255, matching Ultralytics export defaults.
pub fn preprocess_bgra_letterbox(
    bgra: &[u8],
    source_width: u32,
    source_height: u32,
    contract: DetectorProcessingContract,
) -> Result<PreprocessedInput, DetectionContractError> {
    contract.validate()?;
    let expected = source_width
        .checked_mul(source_height)
        .and_then(|pixels| pixels.checked_mul(4))
        .ok_or(DetectionContractError::InvalidFrame)? as usize;
    if source_width == 0 || source_height == 0 || bgra.len() < expected {
        return Err(DetectionContractError::InvalidFrame);
    }

    let scale = (contract.input_width as f32 / source_width as f32)
        .min(contract.input_height as f32 / source_height as f32);
    let resized_width = (source_width as f32 * scale).round() as u32;
    let resized_height = (source_height as f32 * scale).round() as u32;
    let pad_x = (contract.input_width - resized_width) / 2;
    let pad_y = (contract.input_height - resized_height) / 2;
    let plane = (contract.input_width * contract.input_height) as usize;
    let mut chw_rgb = vec![contract.padding_value as f32 / 255.0; plane * 3];

    for target_y in 0..resized_height {
        let source_y = ((target_y as f32 / scale) as u32).min(source_height - 1);
        for target_x in 0..resized_width {
            let source_x = ((target_x as f32 / scale) as u32).min(source_width - 1);
            let source_index = ((source_y * source_width + source_x) * 4) as usize;
            let target_index =
                ((target_y + pad_y) * contract.input_width + target_x + pad_x) as usize;
            chw_rgb[target_index] = bgra[source_index + 2] as f32 / 255.0;
            chw_rgb[plane + target_index] = bgra[source_index + 1] as f32 / 255.0;
            chw_rgb[plane * 2 + target_index] = bgra[source_index] as f32 / 255.0;
        }
    }

    Ok(PreprocessedInput {
        chw_rgb,
        transform: LetterboxTransform {
            source_width,
            source_height,
            input_width: contract.input_width,
            input_height: contract.input_height,
            resized_width,
            resized_height,
            scale,
            pad_x,
            pad_y,
        },
    })
}

/// Decode contiguous YOLOv8 output `[1, channels, anchors]`, reverse the
/// letterbox, normalize to top-left [0,1], filter, and class-aware NMS.
pub fn decode_yolov8_head(
    raw: &[f32],
    channels: usize,
    anchors: usize,
    transform: LetterboxTransform,
    score_threshold: f32,
    iou_threshold: f32,
) -> Result<Vec<Detection>, DetectionContractError> {
    if channels < 5
        || anchors == 0
        || raw.len() != channels.saturating_mul(anchors)
        || !transform.scale.is_finite()
        || transform.scale <= 0.0
        || transform.source_width == 0
        || transform.source_height == 0
    {
        return Err(DetectionContractError::InvalidHeadShape);
    }
    let class_count = channels - 4;
    let source_width = transform.source_width as f32;
    let source_height = transform.source_height as f32;
    let mut decoded = Vec::with_capacity(anchors);

    for anchor in 0..anchors {
        let at = |channel: usize| raw[channel * anchors + anchor];
        let cx = at(0);
        let cy = at(1);
        let width = at(2);
        let height = at(3);
        if ![cx, cy, width, height]
            .iter()
            .all(|value| value.is_finite())
        {
            continue;
        }
        let mut best_score = f32::NEG_INFINITY;
        let mut best_class = 0usize;
        for class_id in 0..class_count {
            let score = at(4 + class_id);
            if score.is_finite() && score > best_score {
                best_score = score;
                best_class = class_id;
            }
        }
        if best_score < score_threshold {
            continue;
        }

        let x1 = ((cx - width * 0.5 - transform.pad_x as f32) / transform.scale)
            .clamp(0.0, source_width);
        let y1 = ((cy - height * 0.5 - transform.pad_y as f32) / transform.scale)
            .clamp(0.0, source_height);
        let x2 = ((cx + width * 0.5 - transform.pad_x as f32) / transform.scale)
            .clamp(0.0, source_width);
        let y2 = ((cy + height * 0.5 - transform.pad_y as f32) / transform.scale)
            .clamp(0.0, source_height);
        if x2 - x1 <= 1.0 || y2 - y1 <= 1.0 {
            continue;
        }
        decoded.push(Detection::new(
            best_class as u32,
            best_score,
            x1 / source_width,
            y1 / source_height,
            (x2 - x1) / source_width,
            (y2 - y1) / source_height,
        ));
    }

    Ok(non_max_suppression(
        &filter_by_score(&decoded, score_threshold),
        iou_threshold,
    ))
}

/// A single detection in normalized [0..1] coordinates with origin top-left.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Detection {
    pub class_id: u32,
    pub score: f32,
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}

impl Detection {
    pub fn new(class_id: u32, score: f32, x: f32, y: f32, width: f32, height: f32) -> Self {
        Self {
            class_id,
            score,
            x,
            y,
            width,
            height,
        }
    }

    fn x2(&self) -> f32 {
        self.x + self.width
    }

    fn y2(&self) -> f32 {
        self.y + self.height
    }
}

/// Drop detections below `min_score`. Score is assumed in [0..1].
/// NaN scores are dropped — they would otherwise survive partial_cmp-based
/// sorting and IoU comparisons in NMS, polluting downstream masks.
pub fn filter_by_score(detections: &[Detection], min_score: f32) -> Vec<Detection> {
    detections
        .iter()
        .filter(|d| d.score.is_finite() && d.score >= min_score)
        .cloned()
        .collect()
}

/// IoU on two detections (axis-aligned).
pub fn iou(a: &Detection, b: &Detection) -> f32 {
    let inter_x = (a.x2().min(b.x2()) - a.x.max(b.x)).max(0.0);
    let inter_y = (a.y2().min(b.y2()) - a.y.max(b.y)).max(0.0);
    let inter = inter_x * inter_y;
    let union = a.width * a.height + b.width * b.height - inter;
    if union <= 0.0 {
        0.0
    } else {
        inter / union
    }
}

/// Class-aware non-maximum suppression. Returns survivors sorted by score
/// descending. `iou_threshold` is the cutoff above which a lower-scoring
/// detection of the same class is suppressed by a higher-scoring one.
pub fn non_max_suppression(detections: &[Detection], iou_threshold: f32) -> Vec<Detection> {
    let mut sorted: Vec<Detection> = detections.to_vec();
    sorted.sort_by(|a, b| {
        b.score
            .partial_cmp(&a.score)
            .unwrap_or(std::cmp::Ordering::Equal)
    });

    let mut keep: Vec<Detection> = Vec::with_capacity(sorted.len());
    let mut suppressed = vec![false; sorted.len()];

    for i in 0..sorted.len() {
        if suppressed[i] {
            continue;
        }
        keep.push(sorted[i].clone());
        for j in (i + 1)..sorted.len() {
            if suppressed[j] {
                continue;
            }
            if sorted[i].class_id != sorted[j].class_id {
                continue;
            }
            if iou(&sorted[i], &sorted[j]) > iou_threshold {
                suppressed[j] = true;
            }
        }
    }
    keep
}

/// Pixel rectangle, top-left origin. `(x, y, width, height)` in pixels.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PixelRect {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}

/// Convert a normalized detection to a pixel rect with top-left origin.
pub fn to_pixel_rect_top_left(d: &Detection, image_width: f32, image_height: f32) -> PixelRect {
    PixelRect {
        x: d.x * image_width,
        y: d.y * image_height,
        width: d.width * image_width,
        height: d.height * image_height,
    }
}

/// Convert a normalized detection to a pixel rect with bottom-left origin
/// (CoreVideo / Vision pixel-buffer coordinate space).
pub fn to_pixel_rect_bottom_left(d: &Detection, image_width: f32, image_height: f32) -> PixelRect {
    let r = to_pixel_rect_top_left(d, image_width, image_height);
    PixelRect {
        x: r.x,
        y: image_height - (r.y + r.height),
        width: r.width,
        height: r.height,
    }
}

/// Flip a pixel rect's y-axis between top-left and bottom-left origins. The
/// transform is its own inverse.
pub fn flip_y(rect: PixelRect, image_height: f32) -> PixelRect {
    PixelRect {
        x: rect.x,
        y: image_height - (rect.y + rect.height),
        width: rect.width,
        height: rect.height,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn det(class_id: u32, score: f32, x: f32, y: f32, w: f32, h: f32) -> Detection {
        Detection::new(class_id, score, x, y, w, h)
    }

    #[test]
    fn shared_letterbox_contract_produces_nchw_rgb() {
        // Two pixels: red then blue, packed BGRA.
        let bgra = [0, 0, 255, 255, 255, 0, 0, 255];
        let contract = DetectorProcessingContract {
            input_width: 4,
            input_height: 4,
            ..DetectorProcessingContract::YOLOV8_BGRA_640
        };
        let output = preprocess_bgra_letterbox(&bgra, 2, 1, contract).unwrap();
        assert_eq!(output.transform.resized_width, 4);
        assert_eq!(output.transform.resized_height, 2);
        assert_eq!(output.transform.pad_y, 1);
        assert_eq!(output.chw_rgb.len(), 48);
        let red_plane = 4; // first content pixel: y=1, x=0
        let green_plane = 16 + red_plane;
        let blue_plane = 32 + red_plane;
        assert_eq!(output.chw_rgb[red_plane], 1.0);
        assert_eq!(output.chw_rgb[green_plane], 0.0);
        assert_eq!(output.chw_rgb[blue_plane], 0.0);
        assert_eq!(output.chw_rgb[0], 114.0 / 255.0);
    }

    #[test]
    fn letterbox_uses_deterministic_floor_padding_for_odd_remainder() {
        let bgra = vec![255; 3 * 2 * 4];
        let contract = DetectorProcessingContract {
            input_width: 5,
            input_height: 5,
            ..DetectorProcessingContract::YOLOV8_BGRA_640
        };
        let output = preprocess_bgra_letterbox(&bgra, 3, 2, contract).unwrap();
        assert_eq!(output.transform.resized_width, 5);
        assert_eq!(output.transform.resized_height, 3);
        assert_eq!(output.transform.pad_y, 1);
    }

    #[test]
    fn shared_yolov8_decoder_reverses_letterbox_and_runs_nms() {
        let transform = LetterboxTransform {
            source_width: 1280,
            source_height: 720,
            input_width: 640,
            input_height: 640,
            resized_width: 640,
            resized_height: 360,
            scale: 0.5,
            pad_x: 0,
            pad_y: 140,
        };
        // [channels=6, anchors=2]: cx,cy,w,h,class0,class1.
        let raw = [
            320.0, 322.0, 320.0, 322.0, 200.0, 200.0, 100.0, 100.0, 0.9, 0.8, 0.1, 0.2,
        ];
        let decoded = decode_yolov8_head(&raw, 6, 2, transform, 0.25, 0.45).unwrap();
        assert_eq!(decoded.len(), 1);
        assert_eq!(decoded[0].class_id, 0);
        assert!((decoded[0].x - 0.34375).abs() < 1e-5);
        assert!((decoded[0].width - 0.3125).abs() < 1e-5);
    }

    #[test]
    fn shared_decoder_clips_boxes_to_source_boundaries() {
        let transform = LetterboxTransform {
            source_width: 100,
            source_height: 100,
            input_width: 100,
            input_height: 100,
            resized_width: 100,
            resized_height: 100,
            scale: 1.0,
            pad_x: 0,
            pad_y: 0,
        };
        let raw = [5.0, 5.0, 30.0, 30.0, 0.9];
        let decoded = decode_yolov8_head(&raw, 5, 1, transform, 0.25, 0.45).unwrap();
        assert_eq!(decoded.len(), 1);
        assert_eq!(decoded[0].x, 0.0);
        assert_eq!(decoded[0].y, 0.0);
        assert!((decoded[0].width - 0.2).abs() < 1e-6);
        assert!((decoded[0].height - 0.2).abs() < 1e-6);
    }

    #[test]
    fn shared_pipeline_rejects_invalid_frame_and_head_shapes() {
        assert_eq!(
            preprocess_bgra_letterbox(&[], 0, 1, DetectorProcessingContract::YOLOV8_BGRA_640),
            Err(DetectionContractError::InvalidFrame)
        );
        let transform = LetterboxTransform {
            source_width: 1,
            source_height: 1,
            input_width: 640,
            input_height: 640,
            resized_width: 640,
            resized_height: 640,
            scale: 640.0,
            pad_x: 0,
            pad_y: 0,
        };
        assert_eq!(
            decode_yolov8_head(&[0.0; 4], 4, 1, transform, 0.25, 0.45),
            Err(DetectionContractError::InvalidHeadShape)
        );
    }

    #[test]
    fn score_filter_drops_low() {
        let xs = vec![
            det(0, 0.9, 0.0, 0.0, 0.1, 0.1),
            det(0, 0.1, 0.0, 0.0, 0.1, 0.1),
        ];
        let out = filter_by_score(&xs, 0.5);
        assert_eq!(out.len(), 1);
        assert!((out[0].score - 0.9).abs() < 1e-6);
    }

    #[test]
    fn iou_basic() {
        let a = det(0, 1.0, 0.0, 0.0, 1.0, 1.0);
        let b = det(0, 1.0, 0.5, 0.5, 0.5, 0.5);
        // inter = 0.25, union = 1.0 + 0.25 - 0.25 = 1.0
        let v = iou(&a, &b);
        assert!((v - 0.25).abs() < 1e-6, "got {v}");
    }

    #[test]
    fn nms_suppresses_overlapping_same_class() {
        // two heavily overlapping class-0 detections; only highest survives
        let xs = vec![
            det(0, 0.9, 0.10, 0.10, 0.20, 0.20),
            det(0, 0.8, 0.11, 0.11, 0.20, 0.20),
        ];
        let out = non_max_suppression(&xs, 0.5);
        assert_eq!(out.len(), 1);
        assert!((out[0].score - 0.9).abs() < 1e-6);
    }

    #[test]
    fn nms_keeps_overlapping_different_classes() {
        let xs = vec![
            det(0, 0.9, 0.10, 0.10, 0.20, 0.20),
            det(1, 0.8, 0.10, 0.10, 0.20, 0.20),
        ];
        let out = non_max_suppression(&xs, 0.5);
        assert_eq!(out.len(), 2);
    }

    #[test]
    fn nms_returns_sorted_by_score_desc() {
        let xs = vec![
            det(0, 0.5, 0.0, 0.0, 0.05, 0.05),
            det(0, 0.95, 0.5, 0.5, 0.05, 0.05),
            det(0, 0.7, 0.9, 0.9, 0.05, 0.05),
        ];
        let out = non_max_suppression(&xs, 0.5);
        assert_eq!(out.len(), 3);
        assert!(out[0].score >= out[1].score);
        assert!(out[1].score >= out[2].score);
    }

    #[test]
    fn pixel_rect_top_left_conversion() {
        let d = det(0, 1.0, 0.25, 0.5, 0.5, 0.25);
        let r = to_pixel_rect_top_left(&d, 100.0, 200.0);
        assert_eq!(
            r,
            PixelRect {
                x: 25.0,
                y: 100.0,
                width: 50.0,
                height: 50.0
            }
        );
    }

    #[test]
    fn pixel_rect_bottom_left_flips_y() {
        let d = det(0, 1.0, 0.0, 0.0, 0.5, 0.25);
        // top-left would be y=0, h=50; bottom-left y = 200 - 50 = 150
        let r = to_pixel_rect_bottom_left(&d, 100.0, 200.0);
        assert_eq!(r.y, 150.0);
        assert_eq!(r.height, 50.0);
    }

    #[test]
    fn flip_y_is_self_inverse() {
        let r = PixelRect {
            x: 10.0,
            y: 20.0,
            width: 30.0,
            height: 40.0,
        };
        let flipped = flip_y(r, 200.0);
        let back = flip_y(flipped, 200.0);
        assert_eq!(back, r);
    }
}
