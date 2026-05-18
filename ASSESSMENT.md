# LiveBlock — Scheduled Audit & Cross-Platform Roadmap

**Date:** 2026-05-07
**Scope:** Deep read of the current codebase, gap analysis against the product claim ("neural ad-blocker, works on any logo/ad, 60 FPS minimum"), and a porting plan for Windows + Linux.
**Author:** Automated assessment run (no user present — report only, no code changes shipped).

---

## 1. Reality vs. Claim

The README and AI.md describe a working neural ad-blocker. The code on `main` is a **458-line skeleton**: the capture/overlay plumbing is real, but the two pieces that make this product the product — detection and inpainting — are mocks, and the CoreML model that ships in the bundle is never loaded.

| Claim in README/AI.md | What's actually in the code |
|---|---|
| "Detects commercial advertisements using YOLO" | [VisionProcessor.swift:20-36](Sources/VisionProcessor.swift) returns a hard-coded box at the center of every frame. `MLModel` / `Vision` / `VNCoreMLRequest` is never imported. |
| "Generative inpainting via RETHINED" | [InpaintingEngine.swift:19-34](Sources/InpaintingEngine.swift) applies `CIPixelate` over the ad rect. No diffusion, no GAN, no model. |
| Model is loaded at startup | `Sources/yolov8n.mlpackage` is present on disk but no Swift file references the class name `yolov8n` or calls `MLModel(contentsOf:)`. The model is dead weight in the bundle. |
| `detectAds()` runs on every frame | `ScreenCaptureManager.stream(_:didOutputSampleBuffer:of:)` ([line 95](Sources/ScreenCaptureManager.swift#L95)) does **not** call `visionProcessor.detectAds()`. Only `regionsStorage.current` (user-drawn rectangles) feeds the pipeline. |
| 60 FPS, "minimal CPU overhead" | Capture is configured for 60 FPS, but each frame allocates a fresh `CVPixelBuffer` via `CVPixelBufferCreate` ([InpaintingEngine.swift:46](Sources/InpaintingEngine.swift#L46)), creates an `NSImage` on every frame, and round-trips to `@MainActor` for SwiftUI binding. No buffer pool, no Metal-direct path. |
| "Cozy glass overlay" rendering only modified pixels | OverlayView renders the **entire captured frame** with `.aspectRatio(.fill)` into an 800×600 window. The inpainted output is not screen-aligned — it's a scaled mini-preview of the full display. |

This is the gap to close before any of the other goals (works on any logo, 60 fps, Windows/Linux ports) make sense to pursue. **Today the app cannot block any real advertisement on its own.**

---

## 2. Bugs and Correctness Issues (macOS)

Ordered by severity. Each is a blocker for the "perfect macOS version" goal.

### S1 — Detection model never runs
- `VisionProcessor.detectAds()` is unreachable from the capture pipeline.
- Fix: in `stream(_:didOutputSampleBuffer:of:)`, call `let detected = visionProcessor.detectAds(in: pixelBuffer)` and merge with `regionsStorage.current` before deciding whether to inpaint.
- Wiring this up requires `VisionProcessor` to actually load the `.mlpackage` (see S2).

### S2 — CoreML model never loaded
- `VisionProcessor.init` only `print`s. There's no `MLModel` instance, no `VNCoreMLModel`, no `VNCoreMLRequest`.
- Fix: lazily build a `VNCoreMLRequest` from `try VNCoreMLModel(for: yolov8n(configuration:).model)`, run via `VNImageRequestHandler(cvPixelBuffer:)` on the `videoQueue`, and translate `VNRecognizedObjectObservation` results into `AdBoundingBox`. The exported model already includes NMS, so output is post-NMS.
- ⚠️ The shipped model is **generic YOLOv8n** (80 COCO classes — `person`, `car`, `dog`, …). It cannot distinguish ads from non-ads. See §3.

### S3 — Overlay window not aligned to the display
- The window is created at 800×600 in the screen center ([LiveBlockApp.swift:21-25](Sources/LiveBlockApp.swift#L21)).
- The capture is the full display.
- The inpainted frame is then `.aspectRatio(.fill)`'d into the 800×600 window, so the user sees a shrunk-down preview of the entire desktop floating in the middle of their screen, not a transparent overlay that hides ads in place.
- Fix: window must be borderless, full-screen, set to the display's frame; capture's `sourceRect` and the SwiftUI render must be in 1:1 pixel correspondence with the screen. Then only the inpainted **regions** should render — everything else should be `Color.clear`.

### S4 — `regionsStorage` coordinate mapping is wrong
- [ScreenCaptureManager.swift:106-108](Sources/ScreenCaptureManager.swift#L106) divides by hard-coded 800/600 to convert window coords to pixel-buffer coords. If the window is resized — and it's `.resizable` — every region is mis-mapped. Y-axis is also AppKit-flipped relative to CoreGraphics pixel buffers; this isn't accounted for.
- Fix: pass the live `window.frame` size and apply the AppKit→CG flip explicitly.

### S5 — Per-frame buffer allocation
- `InpaintingEngine.inpaint` calls `CVPixelBufferCreate` on every frame at 60 fps (one alloc + one free of a full-display BGRA buffer per frame ≈ ~33 MB/frame for 4K). Same in `VisionProcessor.segmentAds`.
- Fix: use a `CVPixelBufferPool` sized to the display dimensions, recreated only when display size changes.

### S6 — `Task { @MainActor in self.currentInpaintedFrame = nsImage }` per frame
- Each frame schedules a hop to the main actor and an `NSImage` rebuild. At 60 fps this saturates the main queue and starves SwiftUI.
- Fix: render via `MTKView` / `CAMetalLayer` driven directly from `videoQueue`. SwiftUI is the wrong layer for a 60 fps video stream.

### S7 — `display.width`/`display.height` are points, not pixels
- `SCDisplay.width`/`height` return logical points. On a Retina display this is half resolution, which silently halves the input to the model and breaks pixel-perfect overlay alignment.
- Fix: multiply by `display.scaleFactor` (or use `pixelWidth`/`pixelHeight` once available) and configure `SCStreamConfiguration.width`/`height` accordingly.

### S8 — `availableContent.displays.first` may be wrong display
- On multi-monitor setups, the order is undefined.
- Fix: select the display whose `frame` contains `overlayWindow.frame.center` (or expose a picker).

### S9 — `Info.plist` and `project.yml` define overlapping keys
- `NSPrincipalClass` is set both in `Info.plist` and as an `info.properties` entry in `project.yml`. After `xcodegen generate`, the project copy wins — fine, but this is a footgun for future edits. Consolidate to `project.yml` and remove the duplicate from the standalone plist.

### S10 — No App Sandbox / Hardened Runtime configuration
- `project.yml` doesn't declare entitlements. To ship via Developer ID notarization (let alone the Mac App Store), the app needs a `.entitlements` file with at least `com.apple.security.device.audio-input` (no, wait — it doesn't need that) and explicit screen-recording handling. App-Store-distributable builds are likely impossible because `ScreenCaptureKit` requires the user to approve in System Settings → Privacy & Security → Screen Recording, which is allowed outside the sandbox but tricky inside.

### S11 — `.DS_Store` is tracked
- `git status` shows `.DS_Store` modified. Add a `.gitignore` and `git rm --cached .DS_Store`.

### S12 — No tests
- No `XCTest` target, no fixtures, no detection-accuracy harness. Without a test corpus, "works on any logo or ad" is an unverifiable claim.

---

## 3. The Hard Problem: "Works on any logo or advertisement"

This is the central product requirement and it is **not solvable with the shipped YOLOv8n weights**. YOLOv8n is trained on COCO — 80 generic object classes, none of which are "advertisement" or "logo." Plugging it in will give you a person/car/dog detector, not an ad blocker.

There are three viable paths, in increasing order of cost and capability:

### Path A — Curated logo dataset, fine-tune a small detector (cheapest, narrowest)
- Use **OpenLogo** (~352 brand classes, ~27k images) or **LogoDet-3K** (~3k classes, ~158k images) to fine-tune YOLOv8n/YOLOv11n.
- Add synthetic data: scrape display ads (Google Display, Facebook ad library) + IAB-standard creative sizes (300×250, 728×90, 160×600, 970×250) to build an "ad layout" class.
- Limitation: only blocks logos/layouts seen during training. Novel ads slip through.

### Path B — Vision-language model classifier (medium cost, broad coverage)
- Run a generic object/region proposer (YOLOv8 + class-agnostic mode, or **GroundingDINO** with the prompt "advertisement, banner, sponsored content, logo, popup"), then filter regions with a CLIP/SigLIP zero-shot head asking "is this an ad?".
- This generalizes to brands/layouts the model has never seen before.
- Latency budget at 60 fps is tight: GroundingDINO + SigLIP will not fit. Run detection on a downsampled frame at 10–15 Hz and **interpolate masks across frames**, while inpainting runs at 60 Hz from cached masks.

### Path C — End-to-end "ad/no-ad" segmentation model (most capable, most work)
- Collect a dataset of `(screenshot, binary ad mask)` pairs across web pages, video players (YouTube ads, Twitch overlays), mobile-style apps. Several thousand annotated frames.
- Fine-tune **SAM 2** or **MobileSAMv2** with a learned ad/no-ad classification head on top of mask proposals.
- This is the only path that hits "works on any logo or ad," but it is a multi-month dataset + training effort.

**Recommendation:** ship Path A first as a v0.2 ("blocks well-known brand logos + standard IAB ad slots"), and treat Path B/C as the v0.3/v1.0 milestone. Be honest in the README about which categories the current build handles.

---

## 4. 60 FPS Performance Audit

Even with detection wired up, the current pipeline cannot sustain 60 fps on a Retina display because of the issues in §2 (S5, S6, S7). The right architecture for the macOS hot path:

```
SCStream  ──▶  videoQueue (userInteractive)
              │
              ├─▶ Detection (every Nth frame, e.g., N=4 → 15Hz)
              │     CoreML on ANE, async, ~5–10ms on M-series
              │
              ├─▶ Mask cache (lock-free atomic swap)
              │
              ├─▶ Inpainting (Metal Performance Shaders or MPSGraph)
              │     • Telea / Navier–Stokes for fast box-fill v0
              │     • LaMa / MAT MPS port for v1 quality
              │
              └─▶ CAMetalLayer-backed view, no SwiftUI image binding
```

Key changes:
- **Decouple detection rate from render rate.** Detection at 10–15 Hz is fine; eyes don't notice ad-mask jitter at that cadence if you spatially smooth between frames. Render and inpaint at 60 Hz.
- **Use `CVPixelBufferPool` and `IOSurface`-backed buffers.** Zero-copy from `SCStream` → `MTLTexture` via `CVMetalTextureCacheCreateTextureFromImage`.
- **Inpaint in Metal.** Even Telea inpainting beats `CIPixelate` for both quality and speed when written as an MPS kernel.
- **Replace `NSImage` binding with `CAMetalLayer`.** SwiftUI just hosts the layer; no per-frame allocations cross the actor boundary.
- **Add an FPS/latency HUD** behind a debug flag so regressions are visible.

Realistic post-fix budget on M1 Pro at 4K:
- Capture: 1–2 ms
- Detection (every 4th frame, 15 Hz effective): 6–8 ms when it runs, 0 ms otherwise
- Inpainting (Metal box fill, 4 regions): 1–2 ms
- Composite + present: 1 ms
- Total: comfortably under the 16.6 ms 60-fps frame budget.

---

## 5. Missing Features and Nice-to-haves (macOS)

Pulled from the gap between the README claims and what would constitute a shippable v1:

**Must-have for v1:**
- Menu-bar icon with Start/Stop, "Show Control Mode" hotkey, Quit. There is no menu-bar UI today.
- Global hotkey (e.g., ⌘⇧B) to toggle Control Mode. Currently the only way back into Control Mode is launching the app.
- Per-display selection and multi-display support.
- Screen-aligned full-display overlay (S3 above).
- Settings pane (sensitivity threshold, allowlist of apps to skip, opt-in for telemetry).
- Persistent user-drawn region store (`UserDefaults` or a JSON file in `~/Library/Application Support/LiveBlock/`).
- Pause-on-fullscreen-app toggle (don't run during Keynote presentations, games, etc.).
- App allowlist/blocklist — many users will want it off in Slack/Zoom/their IDE.
- Sparkle (or equivalent) auto-update + Developer ID signing + notarization pipeline.
- Crash reporting (e.g., Sentry) — even Apple's frame callbacks throw; today crashes vanish.

**Nice-to-haves:**
- Optional region-based caching: if the user is on a static page (no scroll, no video), reuse the previous mask.
- "Dim ad" mode as an alternative to inpaint — some users will prefer obvious blocking to seamless replacement.
- Per-app overlay enable/disable using `NSWorkspace.frontmostApplication`.
- iCloud-synced allowlist between Macs.
- Optional encrypted local log of detections for the user to audit ("LiveBlock blocked 412 ads today").
- Picture-in-picture preview window showing original vs. inpainted (debug).

---

## 6. Porting to Windows

The architectural shape is the same — capture, detect, inpaint, present — but every layer changes implementation:

| Layer | macOS today | Windows port |
|---|---|---|
| Capture | `ScreenCaptureKit` (`SCStream`) | **Windows Graphics Capture API** (`Windows.Graphics.Capture.GraphicsCaptureItem` + `Direct3D11CaptureFramePool`). Min: Windows 10 1903. Falls back to DXGI Desktop Duplication for older. |
| Frame format | `CVPixelBuffer` BGRA + `IOSurface` | `ID3D11Texture2D` BGRA. Zero-copy into ML inference via DirectML. |
| Detection | CoreML / Vision | **ONNX Runtime** with the **DirectML** execution provider (works on AMD/Intel/NVIDIA), or **TensorRT** EP for NVIDIA. Use the same YOLO `.onnx` export from `ultralytics`. |
| Inpainting | Core Image / Metal | **Direct3D 11 compute shaders** or **DirectML** for a learned model. Same Telea/LaMa choice. |
| Overlay window | `NSWindow` borderless + `sharingType=.none` | **Layered/click-through window** with `WS_EX_LAYERED \| WS_EX_TRANSPARENT \| WS_EX_NOACTIVATE \| WS_EX_TOOLMOST`. Excluding self from capture: `SetWindowDisplayAffinity(hwnd, WDA_EXCLUDEFROMCAPTURE)` (Windows 10 2004+). |
| UI | SwiftUI | WinUI 3 + Win2D, or pragmatically: **Tauri 2** (Rust + WebView2) for the settings UI and a thin C++ core for the hot path. |
| Lang | Swift | Recommend **C++ for hot path + Rust for control plane**, exposed to a small WinUI/Tauri shell. Or full Rust with `windows-rs`. |
| Hotkey | `NSEvent.addGlobalMonitorForEvents` | `RegisterHotKey` |
| Tray / menu | `NSStatusItem` | `Shell_NotifyIcon` |
| Permissions | TCC prompt | None — Graphics Capture API does not require system permission, but capturing protected content (DRM video) is still blocked. Document this. |
| Auto-update | Sparkle | **WinSparkle** or MSIX with the Microsoft Store, or a custom updater. |
| Signing | Developer ID + notarization | **Authenticode** code-signing certificate (DigiCert/Sectigo/etc.). EV cert recommended to skip SmartScreen warm-up. |

### Suggested repo layout for cross-platform

```
LiveBlock/
├── core/                  # Shared, platform-agnostic Rust (or C++) crate
│   ├── detection/         # ONNX/CoreML wrapper traits + YOLO post-processing
│   ├── inpainting/        # CPU reference + GPU kernel descriptions (WGSL)
│   ├── coordinator/       # Frame scheduler, mask cache, region store
│   └── config/            # Settings schema, allowlist, hotkey serialization
├── platform/
│   ├── macos/             # Current Sources/ moves here, slimmed to platform glue
│   ├── windows/           # WGC capture + D3D11 + WinUI/Tauri shell
│   └── linux/             # PipeWire + Vulkan + GTK4 (see §7)
├── models/                # .mlpackage / .onnx / .gguf — git-LFS
├── assets/
└── tools/
    ├── export_models.py
    └── eval/              # detection-accuracy harness
```

A Rust core with `cxx`/`swift-bridge` interop on macOS, plain C ABI on Windows/Linux, gives the cleanest cross-platform story without three independent inpainting implementations. (Alternative: write the core in C++ and use `swift-cxx-interop` on macOS — equally valid.)

### Windows-specific risks

- **DRM-protected windows** (Netflix, Disney+, etc.) come back as black frames in Graphics Capture. Detect this and surface a clear error instead of silently inpainting black.
- **Per-monitor DPI scaling** is more chaotic than macOS's `scaleFactor`. Mark the app `PerMonitorV2` DPI-aware in the manifest.
- **Anti-cheat software** (Riot Vanguard, Easy Anti-Cheat) treats overlay/capture combos as suspicious. Add a "pause when game is foreground" mode by default.

---

## 7. Porting to Linux

Linux is the hardest of the three because there's no single capture/overlay surface — X11 and Wayland have completely different APIs.

### Capture
- **Wayland** (the future): use **`xdg-desktop-portal`** + **PipeWire**. The portal opens a permission dialog; PipeWire delivers DMA-BUF frames. This is the path that works under GNOME, KDE, and Sway. There is no equivalent of "always-on screen capture" — the portal token can be persisted (`org.freedesktop.portal.ScreenCast` with `persist_mode`) so the user is prompted only once.
- **X11** (still common): use **`XShm`** + **XComposite** for redirected capture. Faster, no permission prompt, but going away.
- Keep both behind a `Capture` trait; pick at runtime via `XDG_SESSION_TYPE`.

### Detection / Inpainting
- **ONNX Runtime** with the **CUDA** EP (NVIDIA), **ROCm** EP (AMD), or **OpenVINO** EP (Intel). Vulkan EP is improving but not yet broadly stable.
- For platform-portability, the safest baseline is ONNX Runtime CPU + a Vulkan compute fallback for inpainting. Then add CUDA/ROCm acceleration where available.

### Overlay
- **Wayland**: Wayland intentionally **does not** support arbitrary "always-on-top click-through windows from any app" — that's a security feature. Workable approaches:
  1. `wlr-layer-shell-unstable-v1` protocol — supported by wlroots compositors (Sway, Hyprland, river) and KDE. Lets you put a layer above all clients with click-through. **Not supported by GNOME** as of 2026.
  2. For GNOME: ship a small GNOME Shell extension that owns the overlay actor. Higher friction, but the only way.
  3. Last resort: a fullscreen click-through window with `wlr-layer-shell` only — document GNOME as best-effort.
- **X11**: trivial — `_NET_WM_WINDOW_TYPE_DOCK` + `XShapeCombineRectangles` for click-through + `_NET_WM_STATE_ABOVE`.

### UI / packaging
- GTK4 + libadwaita for native feel, or Tauri (same as Windows recommendation above for code share).
- Distribute as **Flatpak** (sandboxed, uses portals naturally) and **AppImage** (sandbox-less, broader compatibility). `.deb`/`.rpm` are nice-to-have.
- Auto-update is whatever the packaging system provides — don't roll your own.

### Linux-specific risks

- GNOME's lack of layer-shell is the biggest open question. May need to ship "GNOME mode = pause when fullscreen, otherwise show in a movable window" as a degraded experience.
- NVIDIA + Wayland still has glitches in some compositors; test early.
- Permission UX through portals is good but slower than macOS — expect first-launch friction.

---

## 8. Recommended Phasing

A pragmatic order of operations. Each phase produces something shippable.

**Phase 0 — Hygiene (1–2 days)**
- `.gitignore`, remove `.DS_Store`, fix `Info.plist`/`project.yml` duplication (S9, S11).
- Add an XCTest target with one smoke test so CI is meaningful.
- Wire up GitHub Actions: `xcodegen generate && xcodebuild build test`.

**Phase 1 — Make the macOS overlay actually work (1 week)**
- Fix S3 (full-display screen-aligned overlay), S4 (region mapping), S7 (Retina), S8 (multi-display).
- Replace SwiftUI image binding with `CAMetalLayer` (S6).
- Add `CVPixelBufferPool` (S5).
- Add menu-bar item + global hotkey + persistent user regions.
- Result: a working "manual ad blocker" — user drags a box, it disappears with a clean inpaint, at 60 fps. No ML needed yet. **This is the demo.**

**Phase 2 — Wire detection (1–2 weeks)**
- Load `.mlpackage` via Vision (S1, S2).
- Fine-tune YOLOv8n on OpenLogo + IAB ad-slot synthetic data (Path A in §3).
- Add a detection-accuracy eval harness in `tools/eval/`.
- Ship "blocks known-brand logos + standard ad slots." Be explicit about the limitation in the README.

**Phase 3 — Refactor for cross-platform (2–3 weeks)**
- Extract Rust (or C++) core: detection trait, inpainting trait, frame coordinator, config.
- Move current Swift code to `platform/macos/`, talking to the core.
- This is the largest single chunk of work and pays for itself across the next two phases.

**Phase 4 — Windows port (3–4 weeks)**
- WGC capture + D3D11 + ONNX/DML detection + Telea inpaint + WinUI shell.
- Goal: parity with macOS Phase 2.

**Phase 5 — Linux port (4–6 weeks)**
- Wayland (layer-shell compositors) + X11. GNOME tracked as a known-degraded path.
- PipeWire portal capture + ONNX Runtime + GTK4/Tauri shell.

**Phase 6 — Better detection (ongoing)**
- Path B from §3: GroundingDINO + CLIP zero-shot ad classifier.
- Detection at 10–15 Hz, render at 60 Hz.

**Phase 7 — Better inpainting (ongoing)**
- LaMa or MAT in MPS / DirectML / Vulkan.

Total realistic timeline to "perfect macOS + Windows + Linux at v1 quality" assuming a single full-time engineer: **4–6 months**. The detection problem (§3) is the dominant risk; everything else is execution.

---

## 9. Things I Would Want to Know Before Starting

- Is there a brand list / specific ad sources we want to prioritize? "Any logo or ad" is the ambition; the tractable v1 is "the logos and ad sources that matter most to our users."
- What's the distribution channel? Mac App Store has implications for entitlements (App Sandbox + ScreenCaptureKit is fragile). Direct download via Developer ID is much simpler.
- What's the privacy stance? An ad-blocker that ships frames to a server for inference is a non-starter for many users. Today everything is on-device; the design above keeps it that way, but the GroundingDINO + CLIP pipeline (Path B) needs ~2 GB of model weights — confirm that's acceptable.
- Is monetization in scope? Some choices (telemetry, allowlist sync) depend on this.

---

## 10. What This Report Did Not Do

This run is read-only. It did not:
- Modify any source files.
- Run `xcodegen generate` or attempt a build.
- Run `export_models.py` (would require a Python venv with `ultralytics` + `coremltools`).
- Implement any of the fixes in §2.

Recommended next session: pick Phase 0 + Phase 1 above and execute against a feature branch. The full set of issues is captured here so a future session can be opened against this document directly.
