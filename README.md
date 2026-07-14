<div align="center">

<img src="models/liveblock-icon.svg" width="160" alt="LiveBlock"/>

# LiveBlock

Experimental on-device ad blocking for your screen. LiveBlock captures display
frames, runs a local detector, and replaces selected regions with local
content-aware fill. Frames are not uploaded. No production-ready package or
promoted detector is available yet.

[![Status](https://img.shields.io/badge/status-active-success)]()
[![Platform](https://img.shields.io/badge/macOS-26-blue)]()
[![Platform](https://img.shields.io/badge/Windows-11%20experimental-blue)]()
[![Platform](https://img.shields.io/badge/Linux-experimental-blue)]()

</div>

---

## Why

Browser ad-blockers stop at the page. LiveBlock experiments at the display
capture layer, so it can cover content outside browsers where the operating
system and compositor permit capture and overlay behavior.

## Features

- Screen-level regions. Works outside browsers where capture is available;
  protected content, games, and restricted compositors may be unavailable.
- On-device inference. CoreML on macOS and ONNX Runtime adapters on Windows
  and Linux. Frames stay local; hardware-provider certification is pending.
- Native macOS 26 chrome. SwiftUI Liquid Glass — `.glassEffect`,
  `.glassProminent` buttons, system Toggle.
- Manual regions. The macOS editor supports draw/drag/resize/delete and
  persistence; Windows/Linux editor workflows remain incomplete.
- Source training workflow. Debug/source builds can capture, label, and train
  candidates; production packages are inference-only and never auto-install them.
- Mirror-blend inpainter. Fill is computed from surrounding pixels without
  additional model weights; stale frame work is bounded rather than queued.
- Cross-platform contracts. The macOS app and experimental Rust/Tauri ports
  share data types and IPC vocabulary via `core/`; feature parity is not claimed.

## Quick start

### macOS

```bash
brew install xcodegen fswatch          # fswatch is optional, only for ./run.sh --watch
git clone <this-repo>
cd LiveBlock
tools/setup_codesign_identity.sh       # one-time: stable signing for TCC
./run.sh                               # build + launch
./run.sh --install                     # install to /Applications
./run.sh --watch                       # rebuild + relaunch on every save
./run.sh --reset-perms                 # print TCC reset commands when permissions get stuck
```

`run.sh` regenerates the Xcode project, builds the Rust bridge, signs the
app with a stable identity (so Screen Recording / Accessibility grants
persist across rebuilds), and launches. First run prompts for Screen
Recording (required) and Accessibility (for global hotkeys).

Use `./kill.sh` to force-quit if anything gets stuck.

Requires macOS 26 (Tahoe). Older releases are not supported.

#### First-run checklist

1. `tools/setup_codesign_identity.sh` (once) — stable codesigning identity so TCC
   keeps your permission grants across rebuilds.
2. `./run.sh` — first launch shows two macOS prompts:
   - **Screen Recording** — required for capture. Click "Open System Settings",
     enable LiveBlock, then return to the app.
   - **Accessibility** — required for global hotkeys (⌘⇧L, ⌘⇧B, ⌘⇧S).
3. Click **Start** in the Control Panel (or ⌘⇧L) in a source/Debug build.
4. Press **⌘⇧B** to open the Region Editor. The render layer hides automatically
   while the editor is open — drag a rectangle around something you want
   blocked. Press **Esc** when done.
5. Capture now applies the selected fill style to that manual region. Source
   builds include experimental detector candidate material for `Logo`,
   `Ad banner`, and `Sponsored`; it is not promoted or release evidence. Use
   manual regions and the source labeling workflow for misses.

#### When permissions get stuck

macOS TCC sometimes drops grants if the app's code-signing hash changes
(e.g. you ran `./run.sh` once before `setup_codesign_identity.sh`, then
again after) or if you have a stale `/Applications/LiveBlock.app` from an
earlier build with a different signature. Symptoms: the toggle in System
Settings → Privacy → Screen Recording is grey, or the prompts re-appear
on every launch.

```bash
./run.sh --reset-perms       # prints the exact tccutil commands to copy
```

Then run the printed commands and `./run.sh` again. Grants will stick from
that point onward as long as `tools/.codesign_identity` exists.

#### When changes don't take effect

`run.sh` always kills the running instance, regenerates the project, and
launches the freshly-built binary from DerivedData. If a change still
seems missing, you're probably looking at the wrong copy of the app:

- A stale `/Applications/LiveBlock.app` (from a previous `--install`). The
  Dock and Spotlight launch this one, not the DerivedData binary. `run.sh`
  warns when its hash diverges from the fresh build — re-run with
  `--install` or `rm -rf /Applications/LiveBlock.app`.
- A `LiveBlock 2.xcodeproj` Finder duplicate. `run.sh` refuses to build
  while one exists; `rm -rf "LiveBlock 2.xcodeproj"` to clean up.

### Windows

```pwsh
cd platform\windows
npm install
cargo tauri dev
```

Prerequisites: Rust, MSVC build tools, Node 20+, `cargo install tauri-cli`.
See [`platform/windows/README.md`](platform/windows/README.md).

### Linux

```bash
cd platform/linux
bash scripts/setup-ubuntu.sh   # or setup-fedora.sh / setup-arch.sh
npm install
cargo tauri dev
```

See [`platform/linux/README.md`](platform/linux/README.md).

## Hotkeys

| Combo                     | Action                                |
|---------------------------|---------------------------------------|
| ⌘⇧L                       | Toggle capture                        |
| ⌘⇧B                       | Toggle Region Editor                  |
| ⌘⇧S                       | Capture screenshot for labeling       |
| ⌘⇧⌥ .                     | Panic disable                         |
| Esc                       | Exit Region Editor                    |
| ⌘Q                        | Quit                                  |

Windows and Linux mirror the same shortcuts with `Ctrl` instead of `⌘`.

## How it works

```
┌────────────────────────────────────────────────────────────┐
│        Local display capture                               │
│  macOS: ScreenCaptureKit · Win: WGC · Linux: PipeWire/X11  │
└──────────────────────────┬─────────────────────────────────┘
                           │
            ┌──────────────┼──────────────┐
            ▼              ▼              ▼
   Periodic detection  Region Store   Bounded inpainting
   YOLOv8 → mask cache    │           Mirror-blend, content-aware
            └──────────────┼──────────────┘
                           ▼
              Render Layer · click-through overlay
```

- Capture produces local BGRA frame data. Windows currently copies D3D textures
  to CPU memory; Linux supports mapped PipeWire buffers and X11 shared memory,
  not DMA-BUF-only portals.
- Detection runs periodically and results are cached. CoreML chooses available
  macOS compute units; Windows/Linux hardware-provider execution is uncertified.
- Region store holds user-drawn rectangles in normalized [0..1] coords —
  portable across resolutions and platforms.
- Inpainting picks an axis from the region's aspect ratio, samples a band
  of pixels above and below (or left and right), reflects each band across
  the region's adjacent edge, and cross-fades the two with a linear
  gradient. No model weights, runs at frame rate.
- The render layer paints only generated patches. Supported macOS, Windows, and
  KDE/wlroots/X11 paths target click-through; GNOME Wayland uses a movable,
  non-click-through limited preview.

## Train your own detector (source workflow)

Production desktop packages are inference-only and never bootstrap Python or
pip. Training is available from an explicit source/Debug checkout; see
[`docs/TRAINING_RUNTIME.md`](docs/TRAINING_RUNTIME.md) for the distribution and
security policy.

The bundled open-vocabulary model targets logos, ad banners, and sponsored
content without a fixed brand list. Detection quality still varies by layout,
size, and contrast. To personalize it for the ads you actually see:

1. Click Start, grant Screen Recording on first run.
2. While browsing normally, press ⌘⇧S each time you see an ad.
3. Open the Label window and drag a rectangle around each ad. Yellow
   proposals from the bundled model help — accept or reject with one click.
4. In a source/Debug build, open the Training Dashboard and click Train Now.
5. A macOS notification fires when training completes. The export remains a
   candidate until a complete schema-5 promotion report passes; the app never
   auto-installs unverified weights.

End-to-end docs in [`tools/README.md`](tools/README.md). Realistic timing
on M-series Macs: 30 minutes to 3 hours, depending on dataset size.

## Repository layout

```
LiveBlock/
├── Sources/                macOS app (Swift + AppKit + SwiftUI + ScreenCaptureKit)
├── Tests/                  XCTest target
├── core/                   Cross-platform Rust crates (regions, labels, detection, bridge)
├── platform/
│   ├── windows/            Windows port (Rust + Tauri 2 + windows-rs + DirectML)
│   ├── linux/              Linux port (Rust + Tauri 2 + ashpd + x11rb + wgpu)
│   └── _shared-frontend/   HTML + TS for the Tauri ports
├── tools/                  Python training pipeline + setup scripts
├── models/                 YOLO weights (.pt) + brand assets
├── ARCHITECTURE.md         Cross-platform architecture
└── ASSESSMENT.md           Standing audit + roadmap
```

## Project status

| Component            | Status                                                |
|----------------------|-------------------------------------------------------|
| macOS app            | Capture, region editor, labeling, training dashboard, ML detector tuning. Native macOS 26 Liquid Glass UI. Linked against the Rust core via swift-bridge. |
| Rust core            | Shared versioned persistence, detector processing, behavior, and authenticated model-update contracts with cross-platform tests. |
| Windows port         | Experimental WGC frame loop, local ONNX/CPU processing, physical-display overlay geometry, runtime hotkeys, and authenticated updates. DirectML texture transport, D3D11 compute, bounded device recovery, packaging, and hardware certification remain open. |
| Linux port           | Experimental portal/PipeWire mapped-buffer and X11 frame loops, local ONNX/CPU processing, runtime hotkeys, supported overlay plumbing, and GNOME limited preview. GPU compositing, provider packaging, distribution, and compositor certification remain open. |
| Detector             | Runtime taxonomy is Logo / Ad banner / Sponsored, but human review is 0/21 and no candidate passes schema-5; nothing is promotable or distributable. |
| Code signing         | Stable self-signed development identity plus a fail-closed Developer ID/notarization runbook in [`docs/MACOS_RELEASE.md`](docs/MACOS_RELEASE.md); production execution still requires genuine credentials. |

## Roadmap

- Build and gate a representative detector evaluation corpus with per-class precision/recall targets.
- Complete attributable human corpus review and exact CoreML/ONNX parity.
- Add Windows DirectML texture transport, D3D11 compute, and bounded recovery.
- Add Linux wgpu/WGSL compositing and provider/package distribution.
- Execute signed installers, update delivery, and real-device release matrices.

## Documentation

- [`ARCHITECTURE.md`](ARCHITECTURE.md) — system design and migration phases.
- [`docs/DESKTOP_RELEASE_MATRIX.md`](docs/DESKTOP_RELEASE_MATRIX.md) — supported desktop tiers and measurable release gates.
- [`docs/SHARED_CONTRACTS.md`](docs/SHARED_CONTRACTS.md) — versioned persistence and authenticated model contracts.
- [`docs/MODEL_DISTRIBUTION_SECURITY.md`](docs/MODEL_DISTRIBUTION_SECURITY.md) — promotion-bound signing, rollback, and platform adoption status.
- [`docs/DESKTOP_VALIDATION_RUNBOOKS.md`](docs/DESKTOP_VALIDATION_RUNBOOKS.md) — clean-install, upgrade, permission, display, lifecycle, and crash evidence procedures.
- [`docs/PRIVACY_SECURITY_REVIEW.md`](docs/PRIVACY_SECURITY_REVIEW.md) — local frame/data flow and authenticated update boundary review.
- [`docs/RELEASE_NOTES_DRAFT.md`](docs/RELEASE_NOTES_DRAFT.md) — unreleased scope, implemented behavior, and known limitations.
- [`ASSESSMENT.md`](ASSESSMENT.md) — standing audit and prioritized roadmap.
- [`AI.md`](AI.md) — guide for AI agents working on this codebase.
- [`tools/README.md`](tools/README.md) — training-pipeline walkthrough.
- [`platform/windows/README.md`](platform/windows/README.md) — Windows build prerequisites.
- [`platform/linux/README.md`](platform/linux/README.md) — distro-specific setup.

## Contributing

Issues and pull requests welcome. Two house rules:

1. No frames leave the user's machine. Local-only is the product.
2. Do not claim something works until it has been verified. Build green
   plus tests green is not enough — UI/UX changes need an actual run.


---

Made with [Claude](https://claude.ai).
