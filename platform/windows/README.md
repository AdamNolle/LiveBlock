# LiveBlocker — Windows port

Rust + Tauri 2 + windows-rs port of the macOS LiveBlocker app. The hot
path (capture → detection → inpainting → overlay) is native Rust; the UI
is a Tauri webview. The frontend HTML/CSS/TS lives in
[`../_shared-frontend/`](../_shared-frontend/) and is shared with the
Linux port.

## Status

**Experimental, not release-ready.** The Windows.Graphics.Capture path acquires
BGRA monitor frames, handles size changes and item closure, retains the newest
labeling frame, and feeds a capacity-one worker so slow CPU inference/inpainting
drops stale work instead of blocking frame delivery. The control panel exposes
explicit physical-monitor selection; render/editor windows follow negative
origins and PerMonitorV2 pixel geometry. Global hotkeys and tray actions dispatch
natively, including panic teardown. The render HWND is click-through, top-most,
and `WDA_EXCLUDEFROMCAPTURE`. Deterministic tests cover queue, monitor ordering,
and conservative unavailable/protected-black-frame policy. DirectML/CPU ORT
providers are configured for CPU-uploaded tensors; zero-copy D3D texture
inference, D3D11 compute inpainting, bounded automatic lifecycle recovery,
packaging, and real-device GPU/DPI/multi-monitor certification remain open.

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

- DRM-protected windows can return black via Windows.Graphics.Capture. After
  sustained near-total opaque-black frames, LiveBlock pauses overlays and shows
  **possible protected or unavailable content**; this conservative heuristic
  cannot distinguish DRM from genuinely black content.
- Anti-cheat tooling may flag an always-on-top overlay. No process injection,
  game hooks, or anti-cheat bypass is attempted; game/anti-cheat real-device
  behavior remains uncertified.
- Per-monitor geometry uses the manifest's `PerMonitorV2` declaration and
  effective monitor DPI, but mixed-scale/hot-plug behavior still needs real
  Windows certification.
- Production packages require an authenticated promoted ONNX model and a
  nonempty embedded public-key ring. Source/CI builds use an explicit empty-ring
  override and are not distributable; release packages never train or download
  replacement weights.

See [`../../ARCHITECTURE.md`](../../ARCHITECTURE.md) for cross-platform context.
