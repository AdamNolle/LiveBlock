# LiveBlock — Windows port

Rust + Tauri 2 + windows-rs port of the macOS LiveBlock app. The hot
path (capture → detection → inpainting → overlay) is native Rust; the UI
is a Tauri webview. The frontend HTML/CSS/TS lives in
[`../_shared-frontend/`](../_shared-frontend/) and is shared with the
Linux port.

## Status

**Experimental, not release-ready.** The Windows.Graphics.Capture path acquires
BGRA monitor frames, handles size changes and item closure, retains the newest
labeling frame, and copies only the newest frame-pool surface into an
application-owned texture. Staging/Map, inference, and inpainting run on the
capacity-one worker so GPU readback does not block the WGC callback. The control panel exposes
explicit physical-monitor selection; render/editor windows follow negative
origins and PerMonitorV2 pixel geometry. Global hotkeys and tray actions dispatch
natively, including panic teardown. The render HWND is click-through, top-most,
and `WDA_EXCLUDEFROMCAPTURE`. Deterministic tests cover queue, monitor ordering,
and conservative unavailable/protected-black-frame policy. DirectML is attempted
with required sequential/no-memory-pattern options; registration/model-load failure
constructs a separate CPU session. Inputs remain CPU-uploaded tensors and registration
is not physical-GPU execution evidence. The version-pinned D3D11/D3D12/ORT boundary is
in [`docs/WINDOWS_DIRECTML.md`](../../docs/WINDOWS_DIRECTML.md); texture transport remains
open. Mirror-blend patches now dispatch through a bounded,
isolated D3D11 compute device when available, then read back for PNG/webview
composition; setup, dispatch, or timeout failure permanently falls back to CPU.
Native power/session/display
observation now preserves user intent and retries runtime recovery after
0.5/1/2/4 seconds. Sustained near-total opaque-black input clears patches and
hides the render surface until visible frames return. Real sleep/lock/device-loss,
protected-content, game/anti-cheat, and GPU/DPI/multi-monitor execution remains
uncertified. Credential-free MSI/NSIS mechanics exist, but current packages are
explicitly BuildOnly and undistributable.

## Prerequisites

- Windows 11 x64 is the release target. Windows 10 build 18362+ source
  compatibility is retained where low-cost but is not release-certified.
- A WGC/D3D11-compatible display adapter. ONNX CPU fallback works; DirectML
  registration and D3D11 compute availability are runtime-dependent and not
  physical-GPU execution evidence.
- [Rust](https://rustup.rs/) — latest stable.
- [Visual Studio Build Tools 2022](https://visualstudio.microsoft.com/downloads/)
  with the "Desktop development with C++" workload.
- [Node.js 20+](https://nodejs.org/).
- Tauri CLI: `cargo install tauri-cli --version "^2.0"`.

## Build & run

```pwsh
# One-time: install shared frontend deps
cd platform\_shared-frontend
npm ci

# Build + run the Windows app
cd ..\windows\src-tauri
..\..\_shared-frontend\node_modules\.bin\tauri.cmd dev
# Production packaging uses tools\release_windows.ps1 and its fail-closed inputs.
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

- DRM-protected or otherwise unavailable capture can return black via
  Windows.Graphics.Capture. After sustained near-total opaque-black frames,
  LiveBlock clears stale patches, hides the render surface, and shows **possible
  protected or unavailable content**. Visible frames must return before the
  surface is restored. This conservative heuristic cannot distinguish DRM from
  a genuinely black fullscreen surface.
- Anti-cheat tooling may flag an always-on-top overlay. No process injection,
  game hooks, or anti-cheat bypass is attempted; game/anti-cheat real-device
  behavior remains uncertified.
- Per-monitor geometry uses the manifest's `PerMonitorV2` declaration and
  effective monitor DPI. Display handles are refreshed by device name during
  bounded recovery, but mixed-scale/hot-plug behavior still needs real Windows
  certification.
- Capture is not reported active and the renderer is not shown until the first
  valid packed frame. A three-second first-frame timeout participates in bounded
  recovery. Power/session observation failure disables capture rather than
  silently running through lock.
- Production packages require an authenticated promoted ONNX model and a
  nonempty embedded public-key ring. Source/CI builds use an explicit empty-ring
  override and are not distributable; release packages never train or download
  replacement weights.

See [`../../ARCHITECTURE.md`](../../ARCHITECTURE.md) for cross-platform context.
