# LiveBlock — Linux port

Rust + Tauri 2 + ashpd (xdg-desktop-portal) + x11rb + wgpu (Vulkan) port of
the macOS LiveBlock app. The hot path (capture → detection → inpainting →
overlay) is native Rust; the UI is a Tauri webview that matches the macOS
Liquid Glass aesthetic.

## Status

**Experimental, not release-ready.** Wayland uses the ScreenCast portal and a
dedicated PipeWire loop with capacity-one newest-frame delivery and BGRA/BGRx,
NV12, and YUY2 conversion. X11 captures the virtual root through XComposite and
fd-backed MIT-SHM, converting padded 24/32-bit server pixels to packed BGRA.
Both paths feed local ONNX detection, CPU inpainting, patch rendering,
telemetry, and labeling screenshots. Native CI proves compilation and format
unit tests; real compositor/server, multi-output, GPU, overlay, hotkey, and
lifecycle certification remain open.

## Distribution-server matrix

| Compositor | Capture | Overlay | Notes |
|---|---|---|---|
| Sway / Hyprland / river / wlroots | Portal + PipeWire (experimental) | GTK layer-shell click-through (experimental) | Target full support; not certified |
| KDE Plasma (Wayland) | Portal + PipeWire (experimental) | GTK layer-shell click-through (experimental) | Target full support; not certified |
| GNOME / Mutter (Wayland) | Portal + PipeWire (experimental) | **limited decorated preview window** | No global click-through overlay |
| X11 (any) | XComposite + XShm (experimental) | override-redirect + XFixes click-through (experimental) | Target full support; not certified |

Picked at runtime from `XDG_SESSION_TYPE` and `XDG_CURRENT_DESKTOP`.

## Prerequisites

### Ubuntu 24.04 / Debian Trixie+

```bash
sudo apt install build-essential libwebkit2gtk-4.1-dev libgtk-3-dev \
    libsoup-3.0-dev libjavascriptcoregtk-4.1-dev libwayland-dev \
    libxkbcommon-dev pkg-config wayland-protocols libxcb-shm0-dev \
    libxcb-composite0-dev libxcb-shape0-dev libxcb-xfixes0-dev \
    libpipewire-0.3-dev libonnxruntime-dev libvulkan-dev
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
cd ../linux/src-tauri
cargo tauri dev
```

Release bundle:

```bash
cargo tauri build         # produces .deb, .rpm, .AppImage in src-tauri/target/release/bundle/
```

## Hotkeys

Global hotkeys are implemented through the permissioned Wayland
GlobalShortcuts portal and passive X11 `XGrabKey` registrations:

| Combo | Action |
|---|---|
| Ctrl+Shift+L | Toggle capture |
| Ctrl+Shift+B | Toggle region editor |
| Ctrl+Shift+S | Save a labeling frame |
| Ctrl+Shift+Alt+. | Panic disable |

Availability is runtime state, not assumed: the capability response and control
panel report unavailable until every binding succeeds. Portal denial, shortcut
conflicts, or an unsupported environment fail closed. Always retain control-panel
access as the fallback until the target compositor is certified.

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

Production packages must bundle or declare a trusted local ONNX Runtime CPU
library. LiveBlock does not download runtimes, code, or replacement weights.

## Known limitations

- **GNOME mode is not click-through.** Mutter does not implement
  `wlr-layer-shell`, so rendering is confined to a bounded decorated preview
  window. Move or close it when necessary.
- **DRM-protected windows on Wayland** may return black frames depending on
  the compositor's portal implementation.
- **Anti-cheat tooling** in some games will treat any overlay as suspicious.
- Production packages require the exact authenticated promoted ONNX model and
  a nonempty embedded public-key ring. The current source/CI empty-ring build is
  intentionally not distributable.
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

1. **PipeWire SPA pod negotiation** — BGRA/BGRx, YUY2, and NV12 conversion is
   implemented and unit-tested, but DMA-BUF-only portals, plane metadata, color
   range, resize, revocation, and compositor-specific negotiation need devices.
2. **Wayland layer-shell click-through with wgpu surface** — the empty
   input region must be re-applied after every `wl_surface::commit` and
   resize. KWin in particular re-asserts the input region on commit.
3. **ort EP shared-library discovery in Flatpak** — `libonnxruntime_providers_*.so`
   must be reachable inside the sandbox. The default `cpu` feature avoids
   this; CUDA/ROCm/OpenVINO need explicit `--filesystem` permissions.

See [`../../ARCHITECTURE.md`](../../ARCHITECTURE.md) for cross-platform context.
