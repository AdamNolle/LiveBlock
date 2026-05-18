# LiveBlocker — Windows port

Rust + Tauri 2 + windows-rs port of the macOS LiveBlocker app. The hot
path (capture → detection → inpainting → overlay) is native Rust; the UI
is a Tauri webview. The frontend HTML/CSS/TS lives in
[`../_shared-frontend/`](../_shared-frontend/) and is shared with the
Linux port.

## Status

**Skeleton.** Compiles a runnable bundle with real Windows API calls; a
few hot-path bits (D3D11 compute inpaint, full WGC frame loop) are
flagged with `// TODO(windows-port):`.

## Prerequisites

- Windows 10 build 18362 (1903) or newer.
- DirectX 12-capable GPU for DirectML acceleration (CPU fallback works).
- [Rust](https://rustup.rs/) — latest stable.
- [Visual Studio Build Tools 2022](https://visualstudio.microsoft.com/downloads/)
  with the "Desktop development with C++" workload.
- [Node.js 20+](https://nodejs.org/).
- Tauri CLI: `cargo install tauri-cli --version "^2.0"`.

## Build & run

```pwsh
# One-time: install shared frontend deps
cd platform\_shared-frontend
npm install

# Build + run the Windows app
cd ..\windows\src-tauri
cargo tauri dev      # dev mode with hot reload
cargo tauri build    # release MSI under src-tauri\target\release\bundle\
```

## Hotkeys

| Combo                | Action                          |
|----------------------|---------------------------------|
| Ctrl + Shift + L     | Toggle capture                  |
| Ctrl + Shift + B     | Toggle Region Editor            |
| Ctrl + Shift + S     | Capture screenshot for labeling |
| Ctrl + Shift + Alt + . | Panic disable                 |

## Data layout

`%APPDATA%\LiveBlock\`:
```
LiveBlock\
  regions.json
  training\
    screenshots\    full-res PNGs from "capture for labeling"
    labels\         one JSON sidecar per labeled screenshot
    exports\        YOLO-format datasets from tools/export_labels.py
    trash\          discarded screenshots
```

JSON format is byte-compatible with macOS + Linux so labeled training
data round-trips across platforms.

## Known limitations

- DRM-protected windows return black via Windows.Graphics.Capture.
- Anti-cheat tooling may flag the always-on-top overlay.
- Per-monitor DPI requires the manifest's `PerMonitorV2` declaration.
- Bundled detector model is generic YOLOv8n on COCO classes — train your
  own with `tools/auto.sh` for real ad detection.

See [`../../ARCHITECTURE.md`](../../ARCHITECTURE.md) for cross-platform context.
