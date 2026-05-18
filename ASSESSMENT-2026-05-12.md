# LiveBlock — Scheduled Audit (2026-05-12)

**Scope:** Fresh deep read of the codebase five days after the standing
[`ASSESSMENT.md`](ASSESSMENT.md) (2026-05-07). The earlier audit identified
12 macOS bugs (S1–S12), three porting paths, and a phased roadmap. This run
verifies what's been closed since then, what's still open, and re-scopes the
Windows + Linux ports against the code actually on disk today.

**Author:** Automated scheduled run. Read-only — no code changes shipped.

---

## TL;DR

**The skeleton is gone.** Detection, region persistence, inpainting, the
overlay window, hotkeys, training pipeline, and a multi-window SwiftUI shell
are all implemented and wired together. The Rust core has shipped Phase 1 of
the migration plan: `liveblock-bridge` is built into the macOS binary and
`RegionStore` round-trips through Rust today.

**Three blockers remain for "perfect macOS at 60 fps on 4K":**

1. **The render layer is still SwiftUI**, not `CAMetalLayer`/`MTKView`
   ([RenderLayerView.swift:14](Sources/RenderLayerView.swift#L14)). At 4K
   with many patches this saturates the main actor — the original S6 is
   only partially closed.
2. **No `CVPixelBufferPool`** anywhere; per-patch `createCGImage` allocations
   still happen on every frame ([InpaintingEngine.swift:44–46](Sources/InpaintingEngine.swift#L44)).
   Smaller than the old full-frame leak, but not free.
3. **Single display only.** `AppDelegate` builds one overlay window for the
   primary screen; users with multiple monitors get protection on one.

**Two product blockers remain for "works on any logo or advertisement":**

4. The bundled detector is still **generic YOLOv8n on COCO**. It cannot
   identify ads. Detection mode wires the pipeline, doesn't deliver the
   feature ([VisionProcessor.swift:33–36](Sources/VisionProcessor.swift#L33)).
5. The inpainter is **mirror-blend**, not generative. Great on solid pages,
   visible on textured backgrounds.

**Windows and Linux** are in different shapes than the README claims.
Windows is a working skeleton: WGC capture, ONNX/DirectML detection, CPU
mirror-blend inpaint, layered overlay, hotkeys, tray, DPI awareness, 18
Tauri commands — all real. Linux is **half-stubbed**: session detection,
ONNX, CPU inpaint, and the strategy-selection layer are real, but every
actual capture loop (PipeWire pump, X11 XShm copy) and every actual overlay
install (layer-shell, X11 override-redirect) is a TODO returning empty
frames or doing nothing.

---

## 1. Status vs. Prior Assessment (S1–S12)

| ID | Issue | Status | Evidence |
|----|---|---|---|
| **S1** | Detection unreachable from capture | **CLOSED** | [ScreenCaptureManager.swift:263–266](Sources/ScreenCaptureManager.swift#L263) — detection runs every 4th frame (~15 Hz at 60 capture) and feeds `detectionCache`. |
| **S2** | CoreML model never loaded | **CLOSED** | [VisionProcessor.swift:168–211](Sources/VisionProcessor.swift#L168) — `VNCoreMLModel` + `VNCoreMLRequest` lazily built and cached; runtime-dir override for trained models. |
| **S3** | Overlay window not display-aligned | **CLOSED** | [RenderLayerWindow.swift:15](Sources/RenderLayerWindow.swift#L15) sizes to `targetScreen.frame`; only inpainted patches render, not the full frame ([RenderLayerView.swift:11–20](Sources/RenderLayerView.swift#L11)). |
| **S4** | Region coords mis-mapped on resize / AppKit flip | **CLOSED** | Regions are now stored in normalized [0..1] coords with `cvRect(inPixelBufferSize:)` doing the explicit flip ([RegionStore.swift:30–36](Sources/RegionStore.swift#L30)). |
| **S5** | Per-frame `CVPixelBufferCreate` | **PARTIALLY CLOSED** | No more full-display buffer allocation. But there's no `CVPixelBufferPool` either; the engine still calls `context.createCGImage(...)` per patch per frame ([InpaintingEngine.swift:155](Sources/InpaintingEngine.swift#L155)). Smaller cost, still O(patches × fps). AI.md claims "A `CVPixelBufferPool` keeps allocations off the hot path" — **this is not true today**. |
| **S6** | SwiftUI `@MainActor` image binding starves main queue | **PARTIALLY CLOSED** | A 30 Hz emit-throttle is in place ([ScreenCaptureManager.swift:90,292](Sources/ScreenCaptureManager.swift#L292)). But `RenderLayerView` still uses `@ObservedObject` + SwiftUI `Image(decorative:)` — no `CAMetalLayer`. Acceptable for few patches, will buckle past ~8–10 simultaneous regions at 4K. |
| **S7** | `display.width`/`height` treated as pixels | **CLOSED** | [ScreenCaptureManager.swift:177–179](Sources/ScreenCaptureManager.swift#L177) multiplies by `backingScaleFactor`. |
| **S8** | `displays.first` may be wrong screen / multi-display | **PARTIALLY CLOSED** | Display picking by `displayID` ([ScreenCaptureManager.swift:345–351](Sources/ScreenCaptureManager.swift#L345)) is fixed. **But the app still creates one overlay window for `NSScreen.main` only** — multi-display protection is missing. |
| **S9** | Duplicate `Info.plist` / `project.yml` keys | **OPEN (cosmetic)** | Both files still declare `NSPrincipalClass`, `NSScreenCaptureUsageDescription`, `NSAccessibilityUsageDescription` ([Info.plist:25,23,27](Info.plist), [project.yml:19–22](project.yml#L19)). After `xcodegen`, the `.yml` wins; cosmetic but a footgun. |
| **S10** | No entitlements / Hardened Runtime mis-set | **PARTIALLY CLOSED** | Hardened Runtime is on in Release ([project.yml:46](project.yml#L46)), off in Debug for XCTest. **Still no `.entitlements` file**; App Sandbox is not declared (correct for Developer ID + ScreenCaptureKit, blocks Mac App Store). |
| **S11** | `.DS_Store` tracked | **CLOSED** | `.gitignore` exists; `.DS_Store` is staged for deletion. |
| **S12** | No tests | **PARTIALLY CLOSED** | Two XCTest files now exist (`RegionStoreTests`, `WindowRetentionTests`) and the Rust core ships **32 unit tests across 5 crates**. No detection-accuracy harness yet. |

**Net delta:** 6 closed, 5 partially closed, 1 cosmetic. The two highest-
severity issues from the prior audit (S1, S2) are resolved.

---

## 2. New Findings (macOS) Not in Prior Audit

These are observations from the May 12 read that the prior assessment
either didn't reach or that have appeared since.

### N1 — Rust bridge is shipping (Phase 1 of ARCHITECTURE.md is done)

`liveblock-bridge` is built by `tools/build_bridge.sh` as a universal
static lib, linked via `OTHER_LDFLAGS: "-lliveblock_bridge"`
([project.yml:37](project.yml#L37)), and the generated Swift wrappers
(`LiveBlockBridge.swift`, `SwiftBridgeCore.swift`) are compiled into the
app target ([project.yml:13–16](project.yml#L13)). `RegionStore.swift`
calls into Rust on every mutation ([RegionStore.swift:65,98,113](Sources/RegionStore.swift#L65))
and snapshots state by round-tripping JSON through the Rust store. The
JSON wire format is **byte-compatible with the Windows + Linux ports**.

This means the cross-platform migration is **not "planned, not started"**
(README:213) — Phase 1 is in production. The README is out of date.

### N2 — Pause-reason enum and pause-on-fullscreen are wired end-to-end

A `PauseReason` enum ([ScreenCaptureManager.swift:14–22](Sources/ScreenCaptureManager.swift#L14))
composes capture state, permissions, frontmost-app fullscreen detection
([AppController.swift:204–223](Sources/AppController.swift#L204)), and
per-app exclude rules ([PerAppRulesStore.swift](Sources/PerAppRulesStore.swift)).
The Control Panel and Mini HUD both read this. This is a v1 feature
listed as a nice-to-have in the prior assessment §5 — implemented.

### N3 — Sleep / display-lock auto-pause

Capture pauses on `willSleep` / `screensDidSleep` and resumes on wake if
it was running before ([ScreenCaptureManager.swift:119–153](Sources/ScreenCaptureManager.swift#L119)).
Not in the prior audit's checklist; this is one of the things you'd
expect to find broken at launch in a v1 ad-blocker, and it's right.

### N4 — Training pipeline is in-app

`TrainingController` + `TrainingDashboardView` + `tools/auto.sh` form a
self-service "capture → label → train → install → reload" loop. The
trained model writes to `~/Library/Application Support/LiveBlock/models/`
and `VisionProcessor.reloadModel()` picks it up without relaunch
([VisionProcessor.swift:160–165](Sources/VisionProcessor.swift#L160)).
This is the right structural answer to "works on any logo": let the user
extend the model from inside the app.

### N5 — On-disk regions corruption handling

`RegionStore.quarantineIfCorrupt` renames a malformed `regions.json` to
`.corrupt-<ts>` instead of silently losing data ([RegionStore.swift:72–88](Sources/RegionStore.swift#L72)).
Good production-quality detail.

### N6 — Tests are minimal

Only two XCTest files, both narrowly scoped (`RegionStoreTests`,
`WindowRetentionTests`). Missing entirely:
- Inpaint correctness fixtures (golden PNGs in / out)
- Capture-stream lifecycle (start → stop → start)
- Hot-path performance regression (frame budget assertion)
- Multi-display window placement

The Rust side is in better shape (32 tests), but the Swift hot path has
no test fence around regressions.

### N7 — `ATSApplicationFontsPath` requires Resources/ to exist

`Info.plist:5` sets `ATSApplicationFontsPath: "."` to auto-load bundled
`.ttf`/`.otf` from the bundle root. `Sources/Resources/` is referenced
but not enumerated in `project.yml:11–16`. If the resources aren't
in the build phase, `Font.custom("Geist", ...)` calls silently fall back
to SF Pro. Worth verifying that the fonts actually ship.

### N8 — Detection still publishes only COCO labels

`lastDetectionLabels` is shown to the user as the live "what got blocked"
list ([ScreenCaptureManager.swift:294,299](Sources/ScreenCaptureManager.swift#L294)).
Until the model is replaced (§3 in the prior audit), users see "person
(67%)" overlays on people's faces, not "ad". This is honest — but the UI
should label the mode explicitly as "Demo detector — COCO classes" until
a real ad detector ships.

### N9 — Region editor focus-trap

`RenderLayerWindow` sits at `CGWindowLevelForKey(.overlayWindow)` and the
Region Editor at `.floating + 1`. AppDelegate orders the render window
out when the editor opens (good), but if `closeEditor` is reached via a
crash or short-circuit, the render layer never comes back. Worth a
defer-style guarantee that `isEditorOpen` always flips back, or wire it
through a `didDismiss` notification.

---

## 3. The Detection Problem, Re-stated

The prior assessment §3 covered this thoroughly. Summary: **YOLOv8n on
COCO cannot detect ads.** Five days on, that's unchanged. The training
pipeline exists, but the *shipped* weights are still generic.

The pragmatic next step is unchanged from §3 Path A:

- Fine-tune YOLOv11n (newer, faster than v8n) on **OpenLogo** + a small
  synthetic IAB-slot dataset (~2k images is enough for a v0).
- Ship that as the default `yolov8n.mlpackage` replacement.
- Treat Path B (GroundingDINO + SigLIP zero-shot) and Path C (SAM 2 +
  ad-classification head) as v0.3 / v1.0.

The training infrastructure to do this is already in `tools/`
([train_logos.py](tools/train_logos.py), [export_to_coreml.py](tools/export_to_coreml.py)).
What's missing is **the dataset**, not the code.

### Suggested concrete v0.2 plan

1. Pull OpenLogo (~352 brands, ~27k images).
2. Add ~1k IAB-standard ad-creative crops (300×250, 728×90, 160×600,
   970×250) from public ad libraries.
3. Add a "sponsored content card" class trained on a synthetic dataset
   generated by rendering random text over random backgrounds with the
   word "Sponsored" / "Promoted" / "Ad" overlaid.
4. Fine-tune `yolov11n.pt` (better speed/accuracy than v8n) for 50–100
   epochs at 640×640.
5. Export to CoreML, ship as the default.

Eval should land in `tools/eval/` first: a fixed test set of (image,
expected ad bounding boxes) and a `pytest`-style harness that asserts
mAP@0.5 ≥ X. Without this, "works on any logo" cannot be claimed.

---

## 4. 60 FPS Audit (Updated)

The prior assessment's §4 is mostly still correct. The post-fix budget
estimate (≤16 ms on M1 Pro at 4K with detection at 15 Hz, inpaint at
60 Hz, ≤4 regions) is still realistic — **but the path is not yet taken**:

- **Detection cadence: ✅** Every 4th frame, ~15 Hz, cached
  ([ScreenCaptureManager.swift:83,263](Sources/ScreenCaptureManager.swift#L83)).
- **Emit throttle: ✅** 30 Hz cap on SwiftUI updates
  ([ScreenCaptureManager.swift:90,292](Sources/ScreenCaptureManager.swift#L292)).
- **Zero-copy `IOSurface` → `MTLTexture`: ❌** Not implemented. Detection
  uses `VNImageRequestHandler(cvPixelBuffer:)` — Vision does the texture
  upload, which works, but the inpaint path goes `CVPixelBuffer` →
  `CIImage` → `createCGImage` per patch per frame.
- **`CAMetalLayer` render: ❌** SwiftUI `Image` binding remains.
- **CVPixelBufferPool: ❌** Per-patch `createCGImage` allocations on every
  frame.

For the "60 fps minimum on any kind of logo or advertisement" target,
the missing pieces in priority order are:

1. **Move render to `CAMetalLayer`.** Host an `NSHostingView` with an
   `MTKView` inside `RenderLayerView`; draw patches as textures via
   `MPSImageCopyToTexture` or a simple textured-quad pipeline. Drop the
   `@ObservedObject` link from main-thread image swapping to a single
   atomic patch-array pointer the Metal render reads each vsync.
2. **Pool `CGImage` allocations.** Either keep an `IOSurface`-backed
   `MTLTexture` per active region (re-rendered in place each frame) or
   make patches `MTLTexture`-typed end-to-end. The CG step is the only
   reason a patch ever leaves GPU memory.
3. **Move inpaint to MPS.** A 5-line MPS pipeline (sample band → reflect
   → blend with linear ramp) is faster and prettier than `CIFilter`
   chained through `CIContext`. This unblocks scaling to ~16 regions
   simultaneously at 4K.

Targets:
- 4K display, 4 regions: should hit ~120 fps headroom after these changes.
- 4K display, 16 regions: should sustain 60 fps comfortably.

The prior §4 ASCII diagram is still the right architecture; the code now
needs to catch up to it.

---

## 5. Missing Features and Nice-to-haves (Updated)

The "must-have for v1" list from prior §5 was 8 items. Status today:

| Item | Status | Evidence |
|---|---|---|
| Menu-bar icon, Start/Stop/Quit | **Done** | [LiveBlockApp.swift](Sources/LiveBlockApp.swift), `MenuBarExtra` |
| Global hotkey for control mode | **Done — 4 hotkeys** | [HotKeyMonitor.swift](Sources/HotKeyMonitor.swift) + [LiveBlockApp.swift:297–310](Sources/LiveBlockApp.swift#L297) |
| Per-display selection | **Single display** | Display *selection* fixed (S8); multi-display *protection* missing |
| Screen-aligned full-display overlay | **Done** | [RenderLayerWindow.swift](Sources/RenderLayerWindow.swift) |
| Settings pane (sensitivity, allowlist, telemetry) | **Done** | [SettingsView.swift](Sources/SettingsView.swift) + [PerAppRulesView.swift](Sources/PerAppRulesView.swift). No telemetry opt-in — that's correct for local-only product. |
| Persistent user regions | **Done, via Rust** | [RegionStore.swift](Sources/RegionStore.swift) |
| Pause on fullscreen | **Done** | [AppController.swift:197,204–223](Sources/AppController.swift#L197) |
| App allowlist/blocklist | **Done** | [PerAppRulesStore.swift](Sources/PerAppRulesStore.swift) |
| Sparkle auto-update / Dev ID + notarization | **Not done** | `tools/setup_codesign_identity.sh` is local self-sign |
| Crash reporting | **Not done** | No Sentry / `MetricKit` hooks observed |

**Remaining must-haves:**

- **Multi-display overlay.** One `RenderLayerWindow` per `NSScreen`,
  re-emitted on `NSApplication.didChangeScreenParametersNotification`.
- **Sparkle + Developer ID notarization.** Necessary for any real
  distribution. `tools/setup_codesign_identity.sh` is local-only.
- **Crash reporting.** `MetricKit` is the on-device, privacy-safe choice
  (logs hang/crash diagnostics to the user's device; nothing leaves).
- **Detection-accuracy harness.** Without a test set, "works on any
  logo" is unverifiable.

**Newly-added v1 candidates (since prior audit):**

- **Onboarding flow ✅** — `OnboardingView.swift` exists. Verify it shows
  on first run and walks the user through Screen Recording / Accessibility
  grants (the two failure modes that silently disable the product).
- **Mini HUD ✅** — gives the user always-visible state. Verify it's
  default-off but discoverable.
- **Region library + per-region toggles ✅** — disabled-IDs are persisted
  in UserDefaults ([AppController.swift:323–336](Sources/AppController.swift#L323)).

**Still nice-to-have:**

- **iCloud sync of regions + allowlist** — bridge to `NSUbiquitousKeyValueStore`
  for small data; CloudKit private DB for regions.
- **Optional encrypted detection log** ("LiveBlock blocked 412 ads
  today") — keep the privacy story intact by writing locally only.
- **"Dim ad" mode** as a render option (some users prefer obvious
  blocking to seamless replacement; one extra fragment in the Metal
  render path).
- **Picture-in-picture debug preview** of original vs. inpainted.
- **Per-region rule presets** ("YouTube ad slot", "Twitter promoted")
  shareable as JSON.

---

## 6. Cross-Platform Reality (Updated)

Both ports exist on disk. The README's "shipping / parity" table is
optimistic — code state below.

### 6.1 Rust core (`/core/crates/`)

| Crate | Surface | Tests | Status |
|---|---|---|---|
| `liveblock-core` | `Capture` / `Detector` / `Inpainter` traits, `Coordinator<C,D,I>`, `MaskCache` | 3 | Traits + coordinator implemented; **no platform uses Coordinator yet** |
| `liveblock-detection` | YOLO NMS, IoU, coord-frame conversion | 8 | Production-ready |
| `liveblock-regions` | `NormalizedRegion` + JSON-persisted thread-safe store | 7 | Shipping (macOS uses it) |
| `liveblock-labels` | Sorted-key JSON label docs with ISO-8601 dates | 8 | Production-ready |
| `liveblock-bridge` | swift-bridge opaque handles | 6 | Linked + used on macOS today |

**Gap:** there is no `liveblock-inpainting` crate yet. Each platform
ships its own (CoreImage on macOS, CPU on Windows, CPU on Linux). The
WGSL shader at `platform/linux/src-tauri/src/inpainting.wgsl` could be
the start of a shared GPU path, but it's not wired and not built.

**Gap:** YOLO **decoder** (raw logits → boxes/scores before NMS) is
implemented separately in each port instead of in `liveblock-detection`.
Windows decodes inside `detection.rs`; Linux decodes inside `detection.rs`.
This is the single biggest piece of cross-platform code that should live
in the shared crate.

**Gap:** the `Coordinator` is implemented and tested, but neither
platform actually drives capture through it. Each port has its own
event loop. Phase 2 of the migration plan in ARCHITECTURE.md (macOS
adopts the traits) hasn't happened.

### 6.2 Windows port (`/platform/windows/`)

A real skeleton, more complete than the README suggests:

| Component | Status |
|---|---|
| Windows Graphics Capture (D3D11 + frame pool, 30 Hz throttle) | **Implemented** ([capture.rs:1–158](platform/windows/src-tauri/src/capture.rs)) |
| ONNX Runtime + DirectML EP, letterbox + YOLOv8 decode | **Implemented** ([detection.rs:33–182](platform/windows/src-tauri/src/detection.rs)) |
| CPU mirror-blend inpainter with 400 ms TTL cache | **Implemented** ([inpainting.rs:122–311](platform/windows/src-tauri/src/inpainting.rs)) |
| D3D11 compute-shader inpainter | **Stub** — TODO comment block ([inpainting.rs:313–326](platform/windows/src-tauri/src/inpainting.rs)) |
| Layered overlay (`WS_EX_LAYERED`/`TRANSPARENT`/`NOACTIVATE`/`TOPMOST` + `WDA_EXCLUDEFROMCAPTURE`) | **Implemented** ([overlay.rs:17–48](platform/windows/src-tauri/src/overlay.rs)) |
| Global hotkeys via `RegisterHotKey` (4 bindings on a dedicated thread) | **Implemented** ([hotkeys.rs:34–99](platform/windows/src-tauri/src/hotkeys.rs)) |
| Tray icon via Tauri tray | **Implemented** ([tray.rs:10–62](platform/windows/src-tauri/src/tray.rs)) |
| Per-monitor V2 DPI awareness | **Implemented** ([main.rs:34–46](platform/windows/src-tauri/src/main.rs)) |
| Shared core link (regions, labels, detection) | **Implemented** (Cargo.toml path deps) |
| 18 Tauri IPC commands | **Implemented** |
| Unit tests | **None** |
| Working binary on CI | **Unknown** — `target/x86_64-pc-windows-msvc/debug/` exists but is empty; cross-build to MSVC requires a Windows host |

**Windows-port gaps to v1 parity with macOS Phase-2:**
1. Verify end-to-end against a real Windows host. README claims it
   compiles on Windows; we cannot verify that from this macOS checkout.
2. DRM-protected window detection. Graphics Capture returns black for
   protected content (Netflix, Spotify) — surface an error rather than
   silently inpainting black.
3. Multi-monitor enumeration. `capture.rs` selects one monitor; needs
   a list/picker mirroring macOS.
4. App allowlist / per-app pause (frontmost via
   `GetForegroundWindow` + `GetWindowThreadProcessId`).
5. Auto-update (WinSparkle or MSIX).
6. Authenticode signing pipeline.
7. The shared YOLO decoder needs to move into `liveblock-detection`.

### 6.3 Linux port (`/platform/linux/`)

Far less complete than the Windows port, despite README claiming
"compiles on Linux." Most of the hot path is TODO:

| Component | Status |
|---|---|
| `XDG_SESSION_TYPE` → Wayland/X11 dispatch | **Implemented** ([session.rs](platform/linux/src-tauri/src/session.rs), [main.rs:30–44](platform/linux/src-tauri/src/main.rs)) |
| Wayland portal session (ashpd, `open_pipe_wire_remote`) | **Implemented** ([capture/wayland.rs:30–73](platform/linux/src-tauri/src/capture/wayland.rs)) |
| Wayland PipeWire stream pump + DMA-BUF / shm copy | **Stub** — `next_frame` sleeps 16 ms and returns empty ([capture/wayland.rs:79–91](platform/linux/src-tauri/src/capture/wayland.rs)) |
| X11 connection + `composite_redirect_subwindows` | **Implemented** ([capture/x11.rs:27–39](platform/linux/src-tauri/src/capture/x11.rs)) |
| X11 XShm segment + frame copy | **Stub** — `shm_seg = 0` ([capture/x11.rs:41–46](platform/linux/src-tauri/src/capture/x11.rs)) |
| ONNX Runtime + EP feature flags (CUDA/ROCm/OpenVINO/TRT/CPU) | **Implemented** ([detection.rs:33–225](platform/linux/src-tauri/src/detection.rs)) |
| CPU mirror-blend inpainter | **Implemented** ([inpainting.rs:62–155](platform/linux/src-tauri/src/inpainting.rs)) |
| wgpu/Vulkan compute inpainter (shader file present) | **Stub** — `inpainting.wgsl` exists, never dispatched |
| Overlay strategy selection (layer-shell / GNOME fallback / X11 override-redirect) | **Implemented** ([overlay/mod.rs:23–34](platform/linux/src-tauri/src/overlay/mod.rs)) |
| wlr-layer-shell click-through overlay | **Stub** ([overlay/wayland_layer_shell.rs:10–24](platform/linux/src-tauri/src/overlay/wayland_layer_shell.rs)) |
| X11 `_NET_WM_WINDOW_TYPE_DOCK` + `XShape` overlay | **Stub** ([overlay/x11.rs:13–22](platform/linux/src-tauri/src/overlay/x11.rs)) |
| GNOME fallback (acknowledged degraded) | **Implemented** ([overlay/wayland_gnome.rs](platform/linux/src-tauri/src/overlay/wayland_gnome.rs)) |
| Global hotkeys (portal `GlobalShortcuts` or X11 XGrabKey) | **Stub** ([hotkeys.rs:28–42](platform/linux/src-tauri/src/hotkeys.rs)) |
| Shared core link (regions, labels, detection) | **Implemented** |
| Tauri IPC commands | **Implemented** (~18 commands) |
| Flatpak manifest | **Implemented** ([flatpak/com.adamnolle.LiveBlock.json](platform/linux/flatpak/com.adamnolle.LiveBlock.json)) |
| Unit tests | **None** |

**Net Linux state:** the dispatch + strategy + IPC scaffolding is in
place, ONNX detection is real, CPU inpaint is real, but **no frames
actually flow through the pipeline yet** because both capture backends
return empty frames and no overlay backend creates a window.

**Linux-port work to reach Windows parity:**
1. Wire the PipeWire stream pump (DMA-BUF → BGRA8). This is the single
   biggest piece. Reference: `pipewire-rs` + `libspa`.
2. Wire XShm: `shmget` + `xcb_shm_attach` + `xcb_shm_get_image`.
3. Wire `zwlr_layer_shell_v1` surface creation for wlroots/KDE
   compositors. The compositor enumeration in `pick_strategy` is
   already done.
4. Wire X11 override-redirect overlay window with `XShapeCombineRectangles`
   for click-through.
5. Wire `xdg-desktop-portal.GlobalShortcuts` for Wayland hotkeys.
6. Wire `XGrabKey` for X11 hotkeys.
7. Dispatch wgpu compute pipeline (the WGSL shader is the easy part —
   binding the storage texture + uniforms isn't done).
8. Add an integration test that asserts at least one frame round-trips
   through the pipeline on each backend.

---

## 7. Updated Roadmap

The prior assessment's 8-phase plan is largely accurate; phases need
re-numbering against today's reality.

**Phase α — Close the macOS 60 fps blockers (4–6 days)**
- Move render to `CAMetalLayer` / `MTKView`. End the SwiftUI image
  binding on the hot path.
- Inpaint via MPS instead of `CIFilter`.
- Multi-display overlay (one `RenderLayerWindow` per `NSScreen`,
  rebuilt on screen-config change).
- Add a frame-budget benchmark to `Tests/LiveBlockTests/` that fails
  CI if 4 regions at 4K drop below 60 fps on M-series.
- Fix the cosmetic S9 (`Info.plist` / `project.yml` overlap).

**Phase β — Ship a real detector (2–4 weeks)**
- Fine-tune YOLOv11n on OpenLogo + synthetic IAB slots + a "sponsored
  card" generator. Ship as the default `yolov8n.mlpackage` replacement
  (or rename to `liveblock-detector.mlpackage`).
- Build `tools/eval/` with a pinned test set and a `pytest`-driven
  mAP target. Wire to CI.
- Replace AI.md / README references to "generic YOLOv8n on COCO" once
  the model swaps.

**Phase γ — Bring Windows to parity (2–3 weeks)**
- Wire DRM-black detection.
- Multi-monitor.
- Per-app pause.
- WinSparkle + Authenticode signing pipeline.
- Move the YOLO decoder into `liveblock-detection`; Windows port
  consumes it.
- Add Tauri smoke tests in CI on a Windows runner.

**Phase δ — Make Linux actually capture frames (3–5 weeks)**
- PipeWire pump (Wayland).
- XShm pump (X11).
- wlr-layer-shell overlay install (Wayland).
- X11 override-redirect overlay install.
- Portal `GlobalShortcuts` + `XGrabKey` for hotkeys.
- wgpu compute inpaint dispatch.
- Flatpak CI build job (the manifest is already there).
- Document GNOME-degraded mode (the fallback is implemented; needs UX).

**Phase ε — Cross-platform consolidation (2–3 weeks)**
- Migrate YOLO decoder into `liveblock-detection` (kill the duplicates).
- Add `liveblock-inpainting` crate with shared mirror-blend reference
  + wgpu shader. macOS continues to use CoreImage; Windows/Linux call
  shared wgpu.
- Adopt `liveblock-core::Coordinator` from at least one platform
  (probably Linux first since it has no legacy event loop).

**Phase ζ — Better detection (ongoing)**
- Path B from prior §3: GroundingDINO + SigLIP zero-shot ad classifier.
- Detection at 10–15 Hz, render at 60 Hz, the same architecture.

**Phase η — Better inpainting (ongoing)**
- LaMa or MAT via Metal Performance Shaders / DirectML / Vulkan.

**Total to "perfect macOS + Windows + Linux at v1 quality":** still
~4–6 months for one full-time engineer. The detection problem (Phase β)
remains the dominant risk; everything else is execution against a now-
much-more-complete codebase.

---

## 8. Things This Report Did Not Do

Read-only run. It did not:
- Modify any source files (other than writing this assessment).
- Run `xcodegen generate` or build either Tauri port.
- Verify the Windows port compiles on a Windows host (cannot from macOS).
- Verify the Linux port compiles on a Linux host (cannot from macOS).
- Run any frame-rate or detection-accuracy benchmarks.
- Train, export, or validate a new detector.

Recommended next session: pick Phase α and α-only — `CAMetalLayer` swap
+ multi-display overlay + frame-budget regression test — and execute
against a feature branch. The detection work (Phase β) is decoupled
and can run in parallel as a dataset+training effort.
