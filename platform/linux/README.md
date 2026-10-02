# LiveBlock — Linux port

Rust + Tauri 2 + ashpd (xdg-desktop-portal) + x11rb + wgpu (Vulkan) port of
the macOS LiveBlock app. The hot path (capture → detection → inpainting →
overlay) is native Rust; the adaptive desktop UI is shared with the Windows port.

## Status

**Wired, pending on-hardware verification.** The full canonical pipeline is
now implemented and routed through the shared core:

```
capture → checksum-bound detector → tracked explicit removal classes
→ protect-overlap filter → inpaint / opaque cover → render + emit
```

- **Capture:** the mapped PipeWire frame loop (`capture/wayland.rs`) and
  the X11 XShm path (`capture/x11.rs`) are implemented for real (format
  negotiation, stride-honoring copy, YUY2/NV12→BGRA conversion; SysV
  `shmget`/`shmat` + `XShmAttach`/`XShmGetImage`).
- **Runtime loop:** `runtime.rs` spawns the capture→detect→track→mask→paint
  task on `start_capture` and emits `capture-state-changed` / `patches-updated`
  / `regions-updated`.
- **Protected pixels:** black captured regions receive an opaque cover. This
  does not recover protected video or prevent a service's capture restrictions.
- **Cover only:** the control panel can draw opaque manual regions without
  opening PipeWire, the screenshot portal or XShm capture. It disables automatic
  detection and screenshots. The same compositor placement limits still apply.
- **Overlay:** the actual patch renderer is a Tauri window. X11 requires an
  active alpha compositor and aligns the renderer with the virtual desktop.
  Blocking stays unavailable when the compositor is missing. If the compositor
  stops during use, LiveBlock stops blocking and hides both overlays. Restart
  the compositor and LiveBlock before resuming. All Wayland
  compositors currently use a floating fallback;
  native layer-shell helpers are not wired to the patch renderer.
- **Hotkeys/tray:** ashpd GlobalShortcuts + XGrabKey are installed and the
  receiver loop is wired; an app-indicator tray is registered.

Linux compilation has been checked on Ubuntu 26.04 ARM64. Five PipeWire
buffer-decoding regressions cover offsets, wrapped chunks, padding,
truncated/corrupt data and separate NV12 planes. The actual XShm capture path
also passes RGB stripe checks on isolated 24-bit and 16-bit Xvfb displays;
four decoder regressions cover padding, byte order, RGB565, 10-bit channels
and malformed buffers. An installed release also passes an isolated X11
Openbox/xcompmgr test for editor transparency, region placement, click-through
after remapping, live region changes and stop. Blocking stays off without a
compositor, including after compositor loss during an active cover. The loss,
retry, restart and panic checks are recorded in
[`linux-compositor-loss-ui.json`](../../validation/linux-compositor-loss-ui.json);
The final selected-mode shortcut, panic and compositor-loss checks are in
[`linux-mode-preference-ui.json`](../../validation/linux-mode-preference-ui.json);
broader editor checks are in `../../validation/linux-native-ui.json`. These virtual-display
checks do not validate Wayland portals, physical multi-monitor placement,
capture self-exclusion or hardware GPU behavior. CPU mirror-blend
is the active inpaint path; the GPU path still needs hardware validation.

The locally built ARM64 Debian package uses glibc 2.43 from Ubuntu 26.04.
It is not a portable binary for older distributions; build from source on the
target distribution for that compatibility baseline.

### Classifier safety (important)

The installed model is the public AdSERP search-ad starter described in
[MODEL_CARD.md](../../MODEL_CARD.md). Automatic detection starts disabled.
Only explicit removable class names authorize removal; unknown or mismatched
metadata fails closed. Known protect labels suppress overlapping removals.
Sports candidates are evaluated separately before installation. User-drawn
regions remain available without a trained model.

## Distribution-server matrix

| Compositor | Capture | Overlay (fidelity) | Notes |
|---|---|---|---|
| Wayland | Portal + PipeWire | Tauri floating fallback (`FloatingDegraded`) | Selected-monitor alignment and compositor behavior require runtime validation |
| X11 | XShm, falling back to GetImage | Tauri click-through renderer aligned to root desktop | Placement and self-exclusion require runtime validation |

Picked at runtime from `XDG_SESSION_TYPE` and `XDG_CURRENT_DESKTOP`.

## Prerequisites

### Ubuntu 24.04 / Debian Trixie+

```bash
sudo apt install build-essential libwebkit2gtk-4.1-dev libgtk-3-dev \
    libsoup-3.0-dev libjavascriptcoregtk-4.1-dev libwayland-dev \
    libxkbcommon-dev pkg-config wayland-protocols libxcb-shm0-dev \
    libxcb-composite0-dev libxcb-shape0-dev libxcb-xfixes0-dev \
    libpipewire-0.3-dev libvulkan-dev libayatana-appindicator3-dev librsvg2-dev
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
cargo install tauri-cli --version "^2.0"
```

Or run the helper: `bash scripts/setup-ubuntu.sh`.

### Fedora

`bash scripts/setup-fedora.sh`

### Arch / Manjaro

`bash scripts/setup-arch.sh`

### Node

Node 20+. `nvm` is the easiest way to manage versions.

## Build & run

The frontend lives in `platform/_shared-frontend/` and is shared with
the Windows port. Tauri picks it up via `frontendDist` in
`tauri.conf.json`.

```bash
# One-time: install frontend deps
cd platform/_shared-frontend && npm install

# Build + run the Linux app
cd ../linux
cargo tauri dev
```

Release bundle:

```bash
cargo tauri build         # bundles are under platform/linux/target/release/bundle/
```

## Hotkeys

| Combo                  | Action                                      |
|------------------------|---------------------------------------------|
| Ctrl + Shift + L       | Toggle capture                              |
| Ctrl + Shift + B       | Toggle region editor                        |
| Ctrl + Shift + S       | Capture screenshot for labeling             |
| Ctrl + Shift + Alt + . | Panic disable (close editor, show panel)   |

System-wide hotkeys on Wayland require granting access via the GlobalShortcuts
portal (one-time prompt). On X11 they use XGrabKey on the root.

## Data layout

`$XDG_DATA_HOME/LiveBlock/` (typically `~/.local/share/LiveBlock/`):

```
LiveBlock/
  regions.json
  training/
    screenshots/    full-res PNGs from "capture for labeling"
    labels/         one JSON sidecar per labeled screenshot
    exports/        YOLO datasets from tools/export_labels.py
    trash/          discarded screenshots
```

Schema is byte-compatible with macOS + Windows so labels round-trip.

## Detection backends (cargo features)

| Feature      | EP           | Hardware needed              |
|--------------|--------------|------------------------------|
| `cpu` (default) | CPU       | Anything                     |
| `cuda`       | CUDA         | NVIDIA + CUDA 12             |
| `rocm`       | ROCm         | AMD Instinct / RX 7000       |
| `openvino`   | OpenVINO     | Intel CPU/GPU/NPU            |
| `tensorrt`   | TensorRT     | NVIDIA, faster than CUDA EP  |

```bash
cargo tauri build --features cuda
```

Before packaging, install the matching CPU runtime from the repository root:

```bash
python3 tools/setup_onnx_runtime.py --platform linux --arch x64
```

Use `--arch aarch64` for ARM64. The installer verifies the official archive,
preserves license notices and bundles ONNX Runtime 1.24.4. The app does not
download missing runtime libraries; automatic detection becomes unavailable.
Compatible GPU libraries may be supplied explicitly through `ORT_DYLIB_PATH`.

## Known limitations

- **Wayland uses a floating fallback.** Exact fullscreen placement is not
  guaranteed. Capture refuses to start if click-through setup reports failure.
- **DRM-protected windows on Wayland** may return black frames depending on
  the compositor's portal implementation.
- **Anti-cheat tooling** in some games will treat any overlay as suspicious.
- **The bundled model covers search-ad layouts.** It is not proof of reliable
  moving-sponsor detection. See the model card for measured deployment results.
- **Flatpak distribution requires portal-only operation.** The X11 path
  won't work inside a Flatpak sandbox; document this in the Flathub listing.

## Flatpak distribution

The recommended distribution path because the Flatpak sandbox uses portals
naturally (no extra permission code in our app).

```bash
cd flatpak
flatpak-builder --install --user --force-clean build-dir com.adamnolle.LiveBlock.json
flatpak run com.adamnolle.LiveBlock
```

## Top 3 things to verify on real Linux

1. **PipeWire negotiation and colorimetry** — mapped RGB and YUY2/NV12 paths
   exist; negotiated YUV range/matrix and unmappable DMA-BUF behavior need a
   live compositor check.
2. **Overlay alignment and capture self-exclusion** — confirm click-through,
   selected-monitor alignment and absence of feedback from the app's renderer.
3. **ort EP shared-library discovery in Flatpak** — `libonnxruntime_providers_*.so`
   must be reachable inside the sandbox. The default `cpu` feature avoids
   this; CUDA/ROCm/OpenVINO need explicit `--filesystem` permissions.

See [`../../ARCHITECTURE.md`](../../ARCHITECTURE.md) for cross-platform context.
