//! YOLO-style post-processing utilities.
//!
//! Pure compute, no platform deps. Operates on plain `Detection` structs in
//! normalized [0..1] coordinates with origin top-left. Helpers convert to/from
//! pixel coordinates and between top-left and bottom-left origins (the macOS
//! Vision / CoreVideo distinction).

use serde::{Deserialize, Serialize};

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
