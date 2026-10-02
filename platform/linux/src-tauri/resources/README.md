# LiveBlock linux resources

`tools/export_onnx.py --install` places the trained `yolov8n.onnx` and its
checksum-bound `yolov8n.classes.json` here. Unknown or mismatched class metadata
cannot authorize automatic removal. User-drawn regions work without a model.

Before bundling, run `python3 tools/setup_onnx_runtime.py --platform linux --arch x64`.
Use `--arch aarch64` for an ARM64 build. This installs verified ONNX Runtime 1.24.4 CPU binaries and
preserves their license notices. Use the architecture of each release.
A compatible GPU runtime is optional via `ORT_DYLIB_PATH`.
Missing runtime libraries disable automatic detection without crashing manual capture.
