# LiveBlock — Linux port

Rust + Tauri 2 + ashpd (xdg-desktop-portal) + x11rb + wgpu (Vulkan) port of
the macOS LiveBlock app. The hot path (capture → detection → inpainting →
overlay) is native Rust; the UI is a Tauri webview that matches the macOS
Liquid Glass aesthetic.

## Status

**Wired, pending on-hardware verification.** The full canonical pipeline is
now implemented and routed through the shared core:

```
capture → detector (ort) → Tracker::update → decide_verdict (SAFE empty-allowlist
stub) → build_remove_mask_from_tracks → DRM split → inpaint / paint_over → emit
```

- **Capture:** the PipeWire DMA-BUF/shm frame loop (`capture/wayland.rs`) and
  the X11 XShm path (`capture/x11.rs`) are implemented for real (format
  negotiation, stride-honoring copy, YUY2/NV12→BGRA conversion; SysV
  `shmget`/`shmat` + `XShmAttach`/`XShmGetImage`).
- **Runtime loop:** `runtime.rs` spawns the capture→detect→track→mask→paint
  task on `start_capture` and emits `capture-state-changed` / `patches-updated`
  / `regions-updated`.
- **DRM:** `region_is_protected_black` routes HDCP black-outs to the capture-free
  `paint_over_regions` opaque cover instead of smearing them through the blend.
- **Overlay:** wlr-layer-shell (`LayerExact`), X11 override-redirect
  (`LayerExact`), and GNOME (`FloatingDegraded`) are implemented.
- **Hotkeys/tray:** ashpd GlobalShortcuts + XGrabKey are installed and the
  receiver loop is wired; an app-indicator tray is registered.

Code paths that touch live Wayland/X11/PipeWire are marked
`VERIFY-ON-LINUX(linux-port)` — they are coded to the documented crate APIs but
cannot be exercised on the Windows dev box. The CPU mirror-blend inpaint is the
only correctness-complete inpaint path; the wgpu/WGSL compute path is written
but gated behind on-hardware validation (we no longer falsely claim a live GPU
inpaint — see `inpainting.rs`).

### Classifier safety (important)

The bundled detector is generic **COCO** YOLOv8n. To prevent the
"erases people/cars" failure, the classifier gate in `runtime.rs` uses an
**EMPTY allowlist**: no COCO class is ever auto-removed. Every track stays
`Unsure`/`Keep`, so nothing is auto-erased until a real sponsor/team/number
model is wired. **User-drawn regions still inpaint/paint as before.**

## Distribution-server matrix

| Compositor | Capture | Overlay (fidelity) | Notes |
|---|---|---|---|
| Sway / Hyprland / river / wlroots | Portal + PipeWire | wlr-layer-shell (`LayerExact`) | Full feature parity |
| KDE Plasma (Wayland) | Portal + PipeWire | wlr-layer-shell (`LayerExact`) | KWin supports layer-shell since 5.27 |
| GNOME / Mutter (Wayland) | Portal + PipeWire | **GNOME mode** (`FloatingDegraded`) | Movable always-on-top window — Mutter refuses wlr-layer-shell |
| X11 (any) | XComposite + XShm | override-redirect + xfixes (`LayerExact`) | Best feature compatibility, going away |

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

The first run downloads `libonnxruntime.so` automatically (ort `load-dynamic`).

## Known limitations

- **GNOME mode is not click-through.** Mutter does not implement
  `wlr-layer-shell`, so the overlay is a normal always-on-top window. Move
  it out of the way when not editing.
- **DRM-protected windows on Wayland** may return black frames depending on
  the compositor's portal implementation.
- **Anti-cheat tooling** in some games will treat any overlay as suspicious.
- **The bundled detection model is generic YOLOv8n on COCO** — it doesn't
  detect ads. Train your own with `tools/auto.sh` (root of repo).
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

1. **PipeWire SPA pod negotiation** — BGRA8888 is preferred, but some
   compositors only offer YUY2 / NV12. `src-tauri/src/capture/wayland.rs`
   needs format-specific conversion paths.
2. **Wayland layer-shell click-through with wgpu surface** — the empty
   input region must be re-applied after every `wl_surface::commit` and
   resize. KWin in particular re-asserts the input region on commit.
3. **ort EP shared-library discovery in Flatpak** — `libonnxruntime_providers_*.so`
   must be reachable inside the sandbox. The default `cpu` feature avoids
   this; CUDA/ROCm/OpenVINO need explicit `--filesystem` permissions.

See [`../../ARCHITECTURE.md`](../../ARCHITECTURE.md) for cross-platform context.
