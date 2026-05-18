# LiveBlock — Architecture (current + cross-platform target)

## Today (macOS only)

```
LiveBlock.app
├── ScreenCaptureManager     SCStream → CVPixelBuffer (60Hz)
├── VisionProcessor          CoreML / Vision  (every Nth frame)
├── InpaintingEngine         CoreImage (mirror-blend, edge-color fallback)
├── RegionStore              normalized [0..1] rects, JSON-persisted
├── LabelingController       screenshots queue, in-app YOLO suggestions
├── TrainingController       export → train → install pipeline
├── AppController            wires everything, holds @Published state
└── 4 windows                ControlPanel · RegionEditor · RenderLayer · Labeling · TrainingDashboard
```

Single-target Swift app, AppKit + SwiftUI, with a Python `tools/` directory
for ultralytics training and CoreML export.

## Target (cross-platform)

The diagram below is what the Windows/Linux ports require. macOS keeps
working unchanged through this transition.

```
                          ┌────────────────────────┐
                          │       core (Rust)      │   ← shared logic
                          ├────────────────────────┤
                          │  trait Capture         │
                          │  trait Detector        │
                          │  trait Inpainter       │
                          │  RegionStore           │
                          │  LabelingStore         │
                          │  Coordinator           │
                          │  Settings              │
                          └────────────────────────┘
                                   ▲       ▲       ▲
                                   │       │       │
              ┌────────────────────┘       │       └────────────────────┐
              │                            │                            │
   ┌──────────┴──────────┐    ┌────────────┴───────────┐    ┌───────────┴───────────┐
   │   platform/macos    │    │    platform/windows    │    │    platform/linux     │
   ├─────────────────────┤    ├────────────────────────┤    ├───────────────────────┤
   │ SCStream            │    │ Windows.Graphics       │    │ PipeWire (Wayland)    │
   │   capture           │    │   .Capture             │    │ XComposite (X11)      │
   │ CoreML / Vision     │    │ ONNX Runtime + DML EP  │    │ ONNX Runtime          │
   │ Core Image          │    │ Direct3D 11            │    │ Vulkan compute        │
   │ AppKit/SwiftUI      │    │ WinUI 3 / Tauri        │    │ GTK4 / Tauri          │
   │ NSStatusItem        │    │ Shell_NotifyIcon       │    │ App indicator         │
   │ NSEvent monitor     │    │ RegisterHotKey         │    │ libxkbcommon / portal │
   └─────────────────────┘    └────────────────────────┘    └───────────────────────┘
```

### Why a Rust core (not C++ or shared Swift)

- **Shared Swift via SPM** would work on macOS but doesn't reach Windows or
  Linux without significant effort. Swift on Linux is stable for servers but
  the AppKit-equivalent UI story is not.
- **C++ core** is fine but loses a lot of safety; the Rust ecosystem already
  has bindings for ONNX Runtime, Vulkan, and the Windows/Linux capture APIs.
- **Rust core** gives us:
  - One copy of the coordinator, region store, labeling format, settings
    serialization, YOLO post-processing
  - Native bindings via `swift-bridge` (macOS), C ABI (Windows), and
    direct integration (Linux)
  - Clean trait boundaries for the platform-specific bits

### Crate layout (planned)

```
core/
├── Cargo.toml
└── crates/
    ├── liveblock-core/        # traits + shared logic
    ├── liveblock-detection/   # YOLO post-processing, mask cache
    ├── liveblock-inpainting/  # CPU reference inpaint kernels
    └── liveblock-config/      # Settings, allowlist, hotkey serialization
```

Each platform shell links the core crate via:
- macOS: `swift-bridge` generates Swift bindings
- Windows: `cbindgen` generates a C header consumed by C++/WinUI
- Linux: same C header, consumed by Rust GTK or Tauri

### Migration plan (no breaking changes during the transition)

1. **Phase 1** — extract pure logic (no AppKit dependencies) into the core
   crate behind a Swift wrapper that mirrors today's API. Verify macOS app
   still builds + runs unchanged.
   - Easy candidates: `RegionStore`, `TrainingPaths`, label JSON schema,
     YOLO post-processing inside `VisionProcessor`.
2. **Phase 2** — define `Capture`, `Detector`, `Inpainter` traits in the
   core; macOS adopts them via Swift adapters.
3. **Phase 3** — start the Windows port: stub trait impls using WGC + DML +
   D3D11. Reach functional parity with macOS Phase-2.
4. **Phase 4** — start the Linux port: PipeWire (portal) + ONNX Runtime
   (CUDA/ROCm/CPU EP) + Vulkan compute for inpaint. Wayland + X11 capture
   paths picked at runtime via `XDG_SESSION_TYPE`.

### Known cross-platform risks

- **DRM-protected windows on Windows** — Graphics Capture returns black for
  these. Detect and surface a clear error.
- **GNOME on Linux** — refuses arbitrary always-on overlay windows (security
  feature). Either ship a GNOME Shell extension that owns the actor, or
  document GNOME as best-effort.
- **Per-monitor DPI on Windows** — manifest must declare `PerMonitorV2`.

## Today's status

- macOS app: **shipping** (manual + COCO-detection demo).
- Logo training pipeline: **shipping** (`tools/train_logos.py` + `tools/auto.sh`).
- In-app labeling + dashboard: **shipping** (this session).
- Cross-platform restructure: **planned**, not started. See
  [`platform/README.md`](./platform/README.md) and
  [`core/README.md`](./core/README.md).
