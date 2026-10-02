//! Exercise each port's actual preprocessing, inference and shared decoder.
#![allow(unexpected_cfgs, dead_code)]
#[path = "../../../platform/windows/src-tauri/src/detection.rs"]
mod windows_detector;
#[path = "../../../platform/linux/src-tauri/src/detection.rs"]
mod linux_detector;
use anyhow::{bail, Context, Result};
use serde_json::json;
use std::collections::BTreeMap;
use std::path::PathBuf;

fn main() -> Result<()> {
    let args: Vec<_> = std::env::args_os().skip(1).collect();
    if args.len() != 5 { bail!("Usage: validate-onnx windows|linux runtime-library model.onnx images-dir output.json"); }
    ort::init_from(PathBuf::from(&args[1]))?.commit();
    let model = PathBuf::from(&args[2]);
    let classes = liveblock_detection::ModelClasses::load(&model);
    if classes.names.is_empty() { bail!("Missing or mismatched model class metadata"); }
    let mut win = if args[0] == "windows" { Some(windows_detector::Detector::load(&model)?) } else { None };
    let mut linux = if args[0] == "linux" { Some(linux_detector::Detector::load(&model)?) } else { None };
    if win.is_none() && linux.is_none() { bail!("Choose windows or linux"); }
    let mut files: Vec<_> = std::fs::read_dir(PathBuf::from(&args[3]))?
        .filter_map(|f| f.ok().map(|f| f.path()))
        .filter(|p| matches!(p.extension().and_then(|s| s.to_str()), Some("png" | "jpg" | "jpeg")))
        .collect();
    files.sort();
    let mut predictions = BTreeMap::new();
    for file in files {
        let mut image = image::open(&file).with_context(|| file.display().to_string())?.to_rgba8();
        let (width, height) = image.dimensions();
        for pixel in image.pixels_mut() { pixel.0.swap(0, 2); }
        let boxes: Vec<_> = if let Some(detector) = win.as_mut() {
            detector.detect(image.as_raw(), width, height)?.into_iter().map(|b| {
                json!({"class": b.class_name, "confidence": b.det.score,
                    "x": b.det.x, "y": b.det.y, "width": b.det.width, "height": b.det.height})
            }).collect()
        } else {
            linux.as_mut().unwrap().detect_bgra(image.as_raw(), width, height)?.into_iter().map(|b| {
                json!({"class": classes.names[b.class_id as usize], "confidence": b.score,
                    "x": b.x / width as f32, "y": b.y / height as f32,
                    "width": b.w / width as f32, "height": b.h / height as f32})
            }).collect()
        };
        predictions.insert(file.file_stem().unwrap().to_string_lossy().to_string(), boxes);
    }
    std::fs::write(PathBuf::from(&args[4]), serde_json::to_vec(&predictions)?)?;
    println!("Validated {} images through the {} detector", predictions.len(), args[0].to_string_lossy());
    Ok(())
}
