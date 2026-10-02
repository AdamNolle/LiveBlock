# LiveBlocker — Windows port

Rust + Tauri 2 + windows-rs port of the macOS LiveBlocker app. The hot
path (capture → detection → inpainting → overlay) is native Rust; the UI
is a Tauri webview. The frontend HTML/CSS/TS lives in
[`../_shared-frontend/`](../_shared-frontend/) and is shared with the
Linux port.

## Status

**Release cross-build verified; native runtime pending.** The WGC frame worker,
native DirectComposition overlay, CPU inpainting and trained detector are wired.
The x64 executable links on macOS with MinGW. Native Windows capture, placement,
DirectML and hotkey behavior still need on-device verification; the GPU compute
inpainter remains a scaffold.

The control panel offers **Screen blend** and **Cover only · no capture**.
Cover only draws opaque manual regions without starting WGC. It disables
automatic detection and screenshots, and Stop or the hotkey hides the covers.
Services may still restrict overlays; this is not a guarantee of undetectability.

## Prerequisites

- Windows 10 build 18362 (1903) or newer.
- DirectX 12-capable GPU for DirectML acceleration (CPU fallback works).
- [Rust](https://rustup.rs/) — latest stable.
- [Visual Studio Build Tools 2022](https://visualstudio.microsoft.com/downloads/)
  with the "Desktop development with C++" workload.
- [Node.js 20+](https://nodejs.org/).
- Tauri CLI: `cargo install tauri-cli --version "^2.0"`.
- Microsoft Edge WebView2 Runtime and Microsoft Visual C++ x64 v14 runtime.

## Build & run

```pwsh
# One-time: install shared frontend deps
cd platform\_shared-frontend
npm install

# Build + run the Windows app
cd ..\windows
python ..\..\tools\setup_onnx_runtime.py --platform windows --arch x86_64
cargo tauri dev      # dev mode with hot reload
cargo tauri build    # release MSI/NSIS under target\release\bundle\
```

For a direct Cargo release build, first build the shared frontend, then use
`cargo build --release --features custom-protocol`. This feature embeds the UI;
without it the executable uses the development server. The portable packager
rejects executables missing the current frontend assets.

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
- The bundled detector is the public AdSERP search-ad starter described in
  [MODEL_CARD.md](../../MODEL_CARD.md). Automatic detection starts disabled;
  only checksum-bound explicit removal classes authorize automatic covers.
  Search-ad results do not prove reliable moving-sponsor detection.

See [`../../ARCHITECTURE.md`](../../ARCHITECTURE.md) for cross-platform context.
