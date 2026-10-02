//! Model-specific class policy. Never infer semantics from COCO numeric IDs.
use crate::{non_max_suppression, Detection};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct ModelClasses {
    pub schema_version: u32,
    pub names: Vec<String>,
    #[serde(default)]
    pub model_sha256: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ModelClassPolicy { Remove, Keep, Unknown }

/// The exporter produces static batch-one RGB square inputs at the training size.
pub fn yolo_input_size(shape: &[i64]) -> Result<u32, &'static str> {
    match shape {
        [1, 3, height, width] if height == width && (32..=4096).contains(height) => Ok(*height as u32),
        _ => Err("expected static [1, 3, size, size] YOLO input"),
    }
}

/// RGB CHW input using the training pipeline's bilinear letterbox resize.
/// Half-pixel sampling avoids text-edge aliasing from nearest-neighbor resize.
pub fn letterbox_bgra(bgra: &[u8], width: u32, height: u32, size: u32) -> Result<Vec<f32>, &'static str> {
    let required = (width as usize).checked_mul(height as usize).and_then(|n| n.checked_mul(4));
    if width == 0 || height == 0 || size == 0 || required.is_none_or(|n| bgra.len() < n) {
        return Err("invalid BGRA frame");
    }
    let scale = (size as f32 / width as f32).min(size as f32 / height as f32);
    let new_w = ((width as f32 * scale).round() as u32).max(1).min(size);
    let new_h = ((height as f32 * scale).round() as u32).max(1).min(size);
    let pad_x = (size-new_w)/2;
    let pad_y = (size-new_h)/2;
    let plane = (size as usize).checked_mul(size as usize).ok_or("input size overflow")?;
    let length = plane.checked_mul(3).ok_or("input size overflow")?;
    let mut input = vec![114.0 / 255.0; length];
    for ty in 0..new_h {
        let sy = ((ty as f32 + 0.5) * height as f32 / new_h as f32 - 0.5).clamp(0.0, (height-1) as f32);
        let y0 = sy.floor() as u32;
        let y1 = (y0+1).min(height-1);
        let fy = sy-y0 as f32;
        for tx in 0..new_w {
            let sx = ((tx as f32 + 0.5) * width as f32 / new_w as f32 - 0.5).clamp(0.0, (width-1) as f32);
            let x0 = sx.floor() as u32;
            let x1 = (x0+1).min(width-1);
            let fx = sx-x0 as f32;
            let destination = (ty+pad_y) as usize * size as usize + (tx+pad_x) as usize;
            for channel in 0..3 {
                let offset = 2-channel;
                let pixel = |x: u32, y: u32| bgra[(y as usize * width as usize + x as usize)*4+offset] as f32;
                let top = pixel(x0,y0)*(1.0-fx) + pixel(x1,y0)*fx;
                let bottom = pixel(x0,y1)*(1.0-fx) + pixel(x1,y1)*fx;
                input[channel*plane+destination] = (top*(1.0-fy)+bottom*fy).round()/255.0;
            }
        }
    }
    Ok(input)
}

impl ModelClasses {
    /// Missing/invalid metadata fails closed: generic detections are display-only.
    pub fn load(model: &std::path::Path) -> Self {
        std::fs::read(model.with_extension("classes.json"))
            .ok().and_then(|bytes| serde_json::from_slice(&bytes).ok())
            .filter(|classes: &Self| {
                classes.schema_version == 1 && !classes.names.is_empty()
                    && std::fs::read(model).ok().is_some_and(|bytes|
                        format!("{:x}", Sha256::digest(bytes)) == classes.model_sha256)
            })
            .unwrap_or_default()
    }

    pub fn policy(&self, id: u32) -> ModelClassPolicy {
        if self.schema_version != 1 { return ModelClassPolicy::Unknown; }
        match self.names.get(id as usize).map(|n| n.to_ascii_lowercase()).as_deref() {
            Some("ad" | "sponsor_remove") => ModelClassPolicy::Remove,
            Some("team_keep" | "number_keep") => ModelClassPolicy::Keep,
            _ => ModelClassPolicy::Unknown,
        }
    }
}

/// Decode a channels-first YOLOv8 head with exact shape/buffer validation.
/// Clip both corners AFTER reversing letterboxing; clipping the origin alone
/// incorrectly enlarged boxes that crossed the top/left edge.
#[allow(clippy::too_many_arguments)]
pub fn decode_yolov8(data: &[f32], shape: &[i64], width: u32, height: u32,
    input_size: u32, score_threshold: f32, iou_threshold: f32) -> Result<Vec<Detection>, &'static str> {
    let [1, channels, anchors] = shape else { return Err("expected [1, 4 + classes, anchors]"); };
    if *channels < 5 || *anchors <= 0 || width == 0 || height == 0 || input_size == 0 {
        return Err("invalid YOLO dimensions");
    }
    let (c, n) = (*channels as usize, *anchors as usize);
    if c.checked_mul(n) != Some(data.len()) { return Err("YOLO tensor length mismatch"); }
    let scale = (input_size as f32 / width as f32).min(input_size as f32 / height as f32);
    let pad_x = ((input_size as f32 - (width as f32 * scale).round()) / 2.0).floor();
    let pad_y = ((input_size as f32 - (height as f32 * scale).round()) / 2.0).floor();
    let at = |ch: usize, a: usize| data[ch * n + a];
    let mut detections = Vec::new();
    for a in 0..n {
        let (cx, cy, bw, bh) = (at(0,a), at(1,a), at(2,a), at(3,a));
        if ![cx,cy,bw,bh].iter().all(|v| v.is_finite()) || bw <= 0.0 || bh <= 0.0 { continue; }
        let mut best = (0, 0.0_f32);
        for class in 0..c-4 {
            let score = at(4+class,a);
            if score.is_finite() && score <= 1.0 && score > best.1 { best = (class, score); }
        }
        if best.1 < score_threshold { continue; }
        let x0 = ((cx-bw/2.0-pad_x)/scale).clamp(0.0,width as f32);
        let y0 = ((cy-bh/2.0-pad_y)/scale).clamp(0.0,height as f32);
        let x1 = ((cx+bw/2.0-pad_x)/scale).clamp(0.0,width as f32);
        let y1 = ((cy+bh/2.0-pad_y)/scale).clamp(0.0,height as f32);
        if x1-x0 <= 1.0 || y1-y0 <= 1.0 { continue; }
        detections.push(Detection::new(best.0 as u32,best.1,x0/width as f32,y0/height as f32,
            (x1-x0)/width as f32,(y1-y0)/height as f32));
    }
    Ok(non_max_suppression(&detections,iou_threshold))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_exported_input_size_instead_of_assuming_640() {
        assert_eq!(yolo_input_size(&[1,3,640,640]),Ok(640));
        assert_eq!(yolo_input_size(&[1,3,1280,1280]),Ok(1280));
        for shape in [[1,3,-1,-1], [1,3,640,320], [4,3,640,640], [1,1,640,640]] {
            assert!(yolo_input_size(&shape).is_err());
        }
    }

    #[test]
    fn bilinear_letterbox_preserves_rgb_and_padding() {
        let bgra = [0,0,255,255, 255,0,0,255];
        let input = letterbox_bgra(&bgra,2,1,4).unwrap();
        assert_eq!(input.len(),48);
        assert_eq!(input[0],114.0/255.0);
        assert_eq!(input[4],1.0);
        assert_eq!(input[4+32],0.0);
        assert_eq!(input[7],0.0);
        assert_eq!(input[7+32],1.0);
        assert!(input[5] > 0.5 && input[5] < 1.0);
        assert!(letterbox_bgra(&[],2,1,4).is_err());
    }

    #[test]
    fn metadata_must_match_the_installed_model() {
        let unique = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos();
        let dir = std::env::temp_dir().join(format!("liveblock-model-{}-{unique}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let model = dir.join("model.onnx");
        std::fs::write(&model, b"model-a").unwrap();
        let classes = ModelClasses { schema_version: 1, names: vec!["ad".into()],
            model_sha256: format!("{:x}", Sha256::digest(b"model-a")) };
        std::fs::write(model.with_extension("classes.json"), serde_json::to_vec(&classes).unwrap()).unwrap();
        assert_eq!(ModelClasses::load(&model).policy(0), ModelClassPolicy::Remove);
        std::fs::write(&model, b"model-b").unwrap();
        assert_eq!(ModelClasses::load(&model).policy(0), ModelClassPolicy::Unknown);
        std::fs::remove_file(model.with_extension("classes.json")).unwrap();
        assert_eq!(ModelClasses::load(&model).policy(0), ModelClassPolicy::Unknown);
        std::fs::remove_dir_all(dir).unwrap();
    }
    #[test]
    fn semantics_are_bound_to_names_not_numeric_ids() {
        let coco = ModelClasses { schema_version: 1, names: vec!["person".into(),"car".into()], ..Default::default() };
        assert_eq!(coco.policy(0),ModelClassPolicy::Unknown);
        let ad = ModelClasses { schema_version: 1, names: vec!["number_keep".into(),"ad".into()], ..Default::default() };
        assert_eq!(ad.policy(0),ModelClassPolicy::Keep);
        assert_eq!(ad.policy(1),ModelClassPolicy::Remove);
        assert_eq!(ad.policy(80),ModelClassPolicy::Unknown);
        assert_eq!(ModelClasses::default().policy(0),ModelClassPolicy::Unknown);
    }
    #[test]
    fn malformed_or_nonfinite_heads_never_create_masks() {
        assert!(decode_yolov8(&[], &[1,3,1],100,100,100,0.5,0.45).is_err());
        assert!(decode_yolov8(&[], &[1,5,1],100,100,100,0.5,0.45).is_err());
        assert!(decode_yolov8(&[f32::NAN,50.0,20.0,20.0,0.9], &[1,5,1],100,100,100,0.5,0.45).unwrap().is_empty());
    }
    #[test]
    fn clips_both_corners_at_source_edge() {
        let d = decode_yolov8(&[5.0,50.0,30.0,20.0,0.9], &[1,5,1],100,100,100,0.5,0.45).unwrap();
        assert_eq!(d.len(),1);
        assert!((d[0].x-0.0).abs()<1e-6);
        assert!((d[0].width-0.2).abs()<1e-6);
    }
    #[test]
    fn reverses_wide_frame_letterbox() {
        let d = decode_yolov8(&[320.0,320.0,320.0,160.0,0.9], &[1,5,1],1280,640,640,0.5,0.45).unwrap();
        assert!((d[0].x-0.25).abs()<1e-6);
        assert!((d[0].y-0.25).abs()<1e-6);
        assert!((d[0].height-0.5).abs()<1e-6);
    }
}
