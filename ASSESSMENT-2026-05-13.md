# LiveBlock — Scheduled Audit (2026-05-13)

**Scope:** Fresh deep read of the codebase one day after
[`ASSESSMENT-2026-05-12.md`](ASSESSMENT-2026-05-12.md). Prompt asked for:
(1) "perfect" macOS that handles any logo/ad at 60 fps minimum, (2)
missing features and nice-to-haves, (3) concrete Windows + Linux porting
plan. This run validates yesterday's findings against the code on disk,
adds new findings the previous audit missed, and turns the gaps into a
day-level execution plan.

**Author:** Automated scheduled run. Read-only — no source files
modified.

**TL;DR delta vs. 2026-05-12:** No commits have landed in the last 24 h
(`git log` still shows the single `1b95a18 Inital commit`). The code
state is identical to yesterday's audit. This report therefore focuses
on what wasn't covered: **(a)** turning the standing macOS perf + detector
gaps into a scoped Phase α / β plan with named files, **(b)** a
day-resolution Windows v1 plan, **(c)** a day-resolution Linux "first
real frame" plan, and **(d)** nine new findings the previous run did not
surface.

---

## 1. New Findings (Not in 2026-05-12 Audit)

### F1 — `_shared-frontend/dist/` has Finder duplicates (bundler footgun)

`ls platform/_shared-frontend/dist/` shows `control-panel 2.html`,
`index 2.html`, `labeling 2.html`, `region-editor 2.html`,
`render-layer 2.html`, `training 2.html` alongside the originals. These
are macOS Finder "copy" duplicates. Same class of footgun as
`LiveBlock 2.xcodeproj` (which `run.sh` already guards against). Vite /
Tauri will happily ship both copies into the bundle; the second-loaded
version may shadow the first depending on the manifest. Add a guard to
the Windows + Linux build scripts that refuses to build while any
` 2.html`, ` 2.ts`, ` 2.css` exists in `_shared-frontend/`.

### F2 — Raw YOLO decode is duplicated across Windows + Linux

`liveblock-detection` exposes `filter_by_score`, `iou`,
`non_max_suppression` — the post-processing — but the raw decoder (logits
tensor → `Vec<Detection>`, including anchor walk, sigmoid, letterbox
inverse) is reimplemented inside `platform/windows/src-tauri/src/detection.rs`
lines 119–225 and `platform/linux/src-tauri/src/detection.rs` lines
119–225, byte-for-byte the same. This is the single biggest piece of
code that should be moved into `liveblock-detection`. Probably 3–4 days
of work plus a smoke test on Windows.

### F3 — No PowerShell training entry point for Windows

`platform/windows/src-tauri/src/training.rs:36` declares a Tauri command
`start_training` but the executor is a TODO. There is no `tools/auto.ps1`
or `tools/auto.bat`. macOS users can train; Windows users currently
cannot. Port `tools/auto.sh` to PowerShell, or — better — replace the
shell wrapper with a small Rust binary in `tools/runner/` that all three
platforms invoke. Estimate: 1 day if PowerShell, 2–3 days if Rust.

### F4 — `liveblock-labels` schema is unused by the Python trainer

`liveblock-labels` defines `LabelDocument` with a sorted-keys ISO-8601
JSON schema. `TrainingController.swift` writes labels through the Rust
labels crate. But `tools/export_labels.py` reconstructs JSON parsing
inline rather than using the Rust crate as ground truth. If the schema
ever drifts (a developer adds a field on the Rust side without updating
the Python parser), training silently skips boxes. Two-line fix: have
`export_labels.py` shell out to a small `liveblock-labels` CLI that emits
canonical YOLO `.txt` directly, eliminating Python's JSON-parsing
responsibility.

### F5 — Multi-display fullscreen detection only checks `NSScreen.main`

`AppController.swift:205` reads `NSScreen.main` for the fullscreen check.
A fullscreen app on a secondary monitor doesn't pause capture if your
control panel happens to be on the primary. The check should iterate
`NSScreen.screens` and pause if **any** screen has a fullscreen frontmost
window — or per-screen if Phase α multi-display lands.

### F6 — No `didChangeScreenParametersNotification` observer

Plugging/unplugging a monitor while LiveBlock runs is undefined behaviour
today. `SCStream` is configured against a stale `displayID`; the overlay
window keeps its original frame. Add a single observer in `AppDelegate`
that rebuilds the render layer set + restarts capture on screen-config
change. Fewer than 30 lines.

### F7 — `Sources/Resources/` membership in `project.yml` not enumerated

`Info.plist:5` sets `ATSApplicationFontsPath: "."` so the bundle's root
should contain `.ttf`/`.otf` files. The `Sources/Resources/Fonts/`
folder exists but is **not declared as a build phase resource** in
`project.yml:11–16`. Result: `Font.custom("Geist", …)` falls through
silently to SF Pro. Add a `Sources/Resources/**` glob to the resources
phase in `project.yml`, then run `xcodegen generate`.

### F8 — `region-editor.html` is an empty shell in the shared frontend

`platform/_shared-frontend/src/region-editor.html` exists but has no
JavaScript counterpart in `ipc.ts` or a sibling `region-editor.ts`.
Tauri opens the window; the user gets an empty page. The Windows port
README claims a working region editor; in reality the UI doesn't ship.
Decision needed: build the editor in HTML+TS in the shared frontend, or
adopt the macOS Swift editor's interaction model (drag-to-create, 8-handle
resize, per-region toggle) port-for-port. Either is a 3–5 day task; the
HTML/TS path keeps Linux + Windows in lockstep.

### F9 — Tray "Open Region Editor" wires to a window that does nothing

`platform/windows/src-tauri/src/tray.rs:13` exposes "Open Region Editor"
which calls `window_show("region-editor")` → `region-editor.html` (see
F8). Same on Linux. Both ports advertise a feature that opens an empty
window. Either disable the menu item until F8 lands, or have it open the
control panel instead and surface "Region editor not yet available on
this platform" in the status bar.

---

## 2. Phase α — "Perfect macOS at 60 fps" (re-scoped to executable plan)

The 2026-05-12 audit identified the same three blockers (SwiftUI render,
per-frame `createCGImage`, single display). Concrete day-by-day:

**Day 1 — Multi-display protection (smallest, highest user-value win).**
- In `AppDelegate` (LiveBlockApp.swift:170–200), replace the single
  `renderLayerWindow` with a `[CGDirectDisplayID: RenderLayerWindow]`
  dictionary built from `NSScreen.screens`.
- Add `NSApplication.didChangeScreenParametersNotification` observer
  (F6); rebuild the dictionary and restart the capture stream on
  change.
- Capture currently targets a single `SCDisplay`. The cheapest path is
  one `SCStream` per display, with the existing detection cadence
  preserved. Multi-screen single-stream would need
  `SCContentFilter(.allDisplays)` and is more invasive.
- Move the `isFrontmostAppFullscreen` check to iterate all screens (F5).

**Day 2–4 — `CAMetalLayer` render layer.**
- Replace `RenderLayerView` (Sources/RenderLayerView.swift:1–34) with an
  `NSViewRepresentable` wrapping `MTKView`.
- In the Metal render pass, draw N textured quads from a single
  vertex buffer; patches arrive as `MTLTexture`s (one per region,
  re-rendered in place each frame, not re-allocated).
- The "patch" type becomes `(normalizedRect, MTLTexture)` instead of
  `(normalizedRect, CGImage)`. `InpaintingEngine.inpaintPatches` swaps
  `context.createCGImage` for `IOSurface`-backed `MTLTexture` writes.
- Read patches via an atomic pointer (`OSAllocatedUnfairLock`) rather
  than `@ObservedObject` — drop the SwiftUI binding on the hot path
  entirely.

**Day 5 — Inpaint on Metal Performance Shaders (or hand-rolled MPS-style
kernel).**
- The mirror-blend recipe is already implemented in WGSL on the Linux
  port (`platform/linux/src-tauri/src/inpainting.wgsl`). Translate
  1-to-1 to a Metal compute shader.
- Two upsides over `CIFilter.blendWithMask`: (1) no intermediate CIImage
  graph compilation per patch, (2) no CPU bitmap render-back via
  `context.createCGImage`.

**Day 6 — Frame-budget regression test.**
- Add `Tests/LiveBlockTests/FrameBudgetTests.swift`. Capture a known
  4K BGRA frame from a fixture PNG, run 4 regions through the engine,
  assert wall time < 8 ms per pass on M-series.
- This is the single most valuable test the Swift target is missing.

**Day 7 — Cosmetic fixups.**
- F1 (dist duplicates), F7 (font resource phase), S9 from prior audit
  (Info.plist / project.yml overlap).
- `MetricKit` opt-in crash reports — five-line `MXMetricManager.shared.add(self)`.

After Phase α the budget on 4K with 16 regions should be < 10 ms per
frame (capture is already 1–2 ms; detection is async). 60 fps becomes
deterministic, not best-effort.

---

## 3. Phase β — "Works on Any Logo or Advertisement"

The current bundled detector is generic YOLOv8n on COCO. Five days on
from yesterday's call, the dataset is still the gating item, not the
training code.

**Concrete path:**

1. **Day 1–2:** Pull OpenLogo (352 brands, ~27 k images), normalize to
   YOLO format. The training script supports it; what's missing is the
   data on disk and a `tools/datasets/openlogo.yml` entry.
2. **Day 3–4:** Synthesize ~1k IAB-slot crops (300×250, 728×90,
   160×600, 970×250) by rendering text-over-image fixtures with the
   word "Sponsored" / "Ad" / "Promoted" overlaid. Helper script:
   `tools/datasets/synthetic_iab.py`. Python's `Pillow` is enough.
3. **Day 5:** Fine-tune `yolov11n.pt` (newer, faster than v8n) for
   50–100 epochs at 640×640. M2 Pro: ~90 min.
4. **Day 6:** Build `tools/eval/`:
   - Pinned 500-image held-out test set (never in train/val).
   - `pytest`-driven assertion: `mAP@0.5 ≥ 0.55` (initial bar; raise
     once a baseline ships).
   - Comparison helper that prints `(this run) vs (best previously
     shipped)` to catch regressions.
5. **Day 7:** Swap `Sources/yolov8n.mlpackage` for the new model
   (rename to `Sources/liveblock-detector.mlpackage` for clarity). Update
   `MLDetectorView.swift:15` ("80 COCO classes" → "112 ad / logo
   classes" or whatever the model knows).

Phases α and β can run in parallel — they touch different files.

---

## 4. Phase γ — Windows v1 (Concrete Plan)

Status from yesterday's audit holds: hot path is real (WGC capture,
DML detection, CPU mirror-blend inpaint, layered overlay, 4 hotkeys,
tray, 18 IPC commands). Per F2, F3, F8, F9 plus the prior audit, here
is the day-resolution gap:

| Day | Task | Files |
|---|---|---|
| 1 | Multi-monitor: replace `pick_monitor()` with picker UI; add per-monitor capture loop instances. | `capture.rs:229–249`, `main.rs`, frontend `control-panel.html` |
| 2 | DRM-protected window detection: check `GetWindowDisplayAffinity()` on enumerated windows; if `WDA_MONITOR` or `WDA_EXCLUDEFROMCAPTURE`, mark region as "protected — won't capture" rather than rendering black. | new `protection.rs` |
| 3 | Per-app pause: `GetForegroundWindow` + `GetWindowThreadProcessId` + `QueryFullProcessImageNameW`. Hook into the existing detection-tick path; mirror macOS `PauseReason`. | `state.rs`, `main.rs` |
| 4 | Region editor UI (F8): port the macOS interaction model (drag-create, 8 handles, drag-to-move, per-region toggle, delete) to TS + Canvas2D in `region-editor.html`. Persist via existing `add_region` / `replace_region` IPC. | `_shared-frontend/src/region-editor.{html,ts}` |
| 5 | Settings UI: hotkey rebind (call `UnregisterHotKey` + `RegisterHotKey`), detection threshold slider, per-app rules, "exclude protected windows". | new `settings.html`, `hotkeys.rs:54–80` |
| 6 | Onboarding flow: first-launch wizard explaining Screen Recording isn't needed (Windows is different) but Run-as-admin may help for protected processes; tray walkthrough. | new `onboarding.html` |
| 7 | Training: PowerShell port of `auto.sh` → `tools/auto.ps1`, plus `start_training` Tauri command implementation. | `tools/auto.ps1`, `training.rs:36` |
| 8 | WinSparkle integration for auto-update; Authenticode signing pipeline in `tauri.conf.json`; MSI build smoke-test on a real Windows 11 host. | `tauri.conf.json`, new `.github/workflows/windows-build.yml` |
| 9 | F2 cleanup: kill the duplicated YOLO decoder; route through `liveblock-detection::decode_yolov8_head` once it exists in the shared crate. | `core/crates/liveblock-detection/src/lib.rs`, `platform/windows/src-tauri/src/detection.rs` |
| 10 | Tests: ONNX golden-pixel test (known 640×640 PNG → expected detection list), inpaint pixel-diff against fixture. | new `tests/` |

**v1 ships at end of day 10** on a single engineer if all goes well;
realistic: 14–18 working days with the Windows-host iteration tax.

---

## 5. Phase δ — Linux "First Real Frame" (Concrete Plan)

Status holds: dispatch / strategy picker / IPC / ONNX detection / CPU
inpaint are real. Capture and overlay are TODO; no frames flow.

| Day | Task | Files |
|---|---|---|
| 1–3 | Wayland PipeWire pump: `pipewire-rs` `Stream::add_listener()` on portal `node_id`. Negotiate `BGRA8888` via Pod metadata; map DMA-BUF for zero-copy, fall back to shm. Replace the 16 ms sleep at `capture/wayland.rs:78–91`. | `capture/wayland.rs` |
| 4–5 | X11 XShm pump: `libc::shmget` + `xcb_shm_attach` + `xcb_shm_get_image` from the composite-redirected root. Replace `capture/x11.rs:41–77`. | `capture/x11.rs` |
| 6–7 | `zwlr_layer_shell_v1` overlay: bind protocol, create OVERLAY layer surface, set empty input region for click-through. | `overlay/wayland_layer_shell.rs:10–24` |
| 8 | X11 override-redirect overlay: extract Tauri webview's `GdkSurface` → X11 `Window` ID, `XFixesSetWindowShapeRegion(window, ShapeInput, empty_region)`. | `overlay/x11.rs:13–22` |
| 9 | Hotkeys: `ashpd::GlobalShortcuts::create_session()` + `bind_shortcuts()` for Wayland; `x11rb::grab_key()` + event pump thread for X11. | `hotkeys.rs:28–42` |
| 10 | Wire capture → detection → inpaint → overlay loop. Each `next_frame` → `detect_bgra` → `inpaint` → `render`. All four already exist as separate units; the loop is the integration. | `main.rs` |
| 11 | wgpu compute inpaint dispatch: bind `inpainting.wgsl` (already written and correct, just never used) — create pipeline, upload `RegionUniform`, dispatch per region, read back. | `inpainting.rs:391–398` |
| 12 | Flatpak CI build job; documented GNOME-degraded mode (the fallback overlay is implemented at `overlay/wayland_gnome.rs`, just needs UX copy). | `.github/workflows/linux-flatpak.yml` |

**"Linux actually captures + renders a frame": end of day 8.**
**v1 parity with Windows: end of day 12.**

---

## 6. Cross-Platform Consolidation (Phase ε, ongoing)

Two refactors that pay for themselves on the third platform:

1. **Move YOLO raw decode into `liveblock-detection`** (F2). Add
   `liveblock-detection::decode_yolov8_head(raw: &[f32], shape: [usize;
   3], scale: f32, pad: (f32, f32)) -> Vec<Detection>`. Then delete the
   duplicates in Windows + Linux detection.rs. 3–4 days incl. a
   golden-tensor fixture test in the core crate.
2. **Create `liveblock-inpainting`** with the WGSL shader already
   written for Linux + a CPU reference fallback. Behind an `inpaint-gpu`
   feature flag to keep `liveblock-core` slim. Windows and Linux call
   the shared crate; macOS continues with the Metal port from Phase α
   §2. ~1 week.

After ε the platform-specific code is purely the bits that can't be
shared by physics: capture (OS APIs), overlay (window managers),
hotkeys (input system), tray (system UI). Detection logic, inpaint
logic, region store, label store, and the JSON schemas are one copy.

---

## 7. Missing Features and Nice-to-Haves (Standing List, Updated)

Carried over from 2026-05-12 §5; this run validated all of them.

**Must-have for v1 across all platforms:**

- Multi-display protection (macOS Day 1; Windows Day 1; Linux Day 10)
- Crash reporting — `MetricKit` on macOS (free), Sentry or simpler
  local log on Windows + Linux
- Auto-update — Sparkle (macOS), WinSparkle (Windows), Flatpak
  (Linux gets it for free)
- Code signing — Developer ID + notarization (macOS), Authenticode
  (Windows), Flathub publishing (Linux)
- Detection-accuracy harness in `tools/eval/` — "works on any logo" is
  unverifiable without it

**Nice-to-haves (carry-over from 2026-05-12, still unimplemented):**

- iCloud / portal-sync regions across machines (NSUbiquitousKeyValueStore
  on macOS; freedesktop secret service on Linux; Windows: skip).
- "Dim ad" mode as a render option (one extra fragment in the Metal
  / D3D11 / WGSL shader).
- Encrypted local "blocked 412 ads today" log.
- Picture-in-picture debug preview of original vs. inpainted.
- Shareable region-rule presets (JSON, signed).

**Net new nice-to-haves surfaced by this run:**

- A "demo mode" that shows the model's class label over each detection
  (already partially wired via `lastDetectionLabels` in
  `ScreenCaptureManager.swift:294`) — flip the UI affordance from
  "Demo detector — COCO classes" warning text to an actual debug
  overlay that can be toggled.
- `tools/eval/regression.py` that ingests a folder of test images +
  expected boxes and reports per-class precision/recall — same harness
  used in CI and locally before publishing a new `.mlpackage`.
- A `liveblock-runner` Rust binary in `tools/` that replaces
  `auto.sh` / `auto.ps1`. Single source of truth across platforms,
  removes the F3 PowerShell gap, removes the F4 schema drift risk.

---

## 8. What This Report Did Not Do

Read-only. Did not:
- Modify any source files (other than writing this assessment).
- Run `xcodegen generate`, `cargo build`, or any Tauri build.
- Verify Windows or Linux ports compile on their respective hosts.
- Run frame-rate, mAP, or memory benchmarks.
- Train, export, or evaluate a new detector.
- Update the README's optimistic "shipping / parity" table.

**Recommended next session:** Pick Phase α Day 1 (multi-display) and
ship it on a feature branch. It's the smallest scoped item with the
biggest user-visible impact, and unblocks the rest of α.

**Recommended parallel track:** Start Phase β Day 1 (download OpenLogo,
write the dataset normalizer). Training infrastructure is ready; the
data is the bottleneck.
