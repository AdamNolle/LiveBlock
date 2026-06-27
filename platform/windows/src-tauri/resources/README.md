# LiveBlock — bundled resources

Drop the detector model here as `yolov8n.onnx` (COCO-pretrained Ultralytics
export), or your trained logo/sponsor model exported to ONNX.

At runtime the app resolves `yolov8n.onnx` from this resource directory
(`tauri::path::BaseDirectory::Resource`). If it is absent, capture/overlay still
work for **user-drawn regions**; only the model-driven detection path is
disabled until a model is present.

This placeholder file exists so the Tauri bundler's `resources/*` glob matches
even before a model is added. It is safe to leave in place.

## ONNX Runtime DLL

The `ort` crate is built with `load-dynamic`, so it loads `onnxruntime.dll` at
runtime (from `ORT_DYLIB_PATH`, or the executable directory / system path). Ship
`onnxruntime.dll` (matching ONNX Runtime 1.24, the API level `ort` 2.0.0-rc.12
targets) next to the app, or set `ORT_DYLIB_PATH`.
