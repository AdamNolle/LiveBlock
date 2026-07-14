# LiveBlock desktop v1.0 — draft release notes

**Status: not released; not release-ready.** This document is a claim boundary
for the current release branch, not an announcement or downloadable release.
Signing credentials, production keyrings, a promoted detector, package evidence,
and required real-device matrices are still missing.

## Intended scope

- macOS 26 on supported Apple hardware.
- Windows 11 x64 primary target; the WGC API floor permits a best-effort Windows
  10 1903+ goal only while compatibility is inexpensive. Current CI and runtime
  evidence do not certify Windows 10.
- KDE and wlroots Wayland plus supported X11 as intended full Linux targets.
- GNOME/Mutter Wayland as an explicitly limited movable-preview mode without a
  global click-through overlay.

No current platform profile reports `releaseReady: true`.

## Implemented on the release branch

### Shared

- Versioned settings, regions, label, signed model-manifest, public-key-ring, and
  accepted-update-state contracts with fail-closed future-schema handling.
- Local three-class detector contract: `Logo`, `Ad banner`, `Sponsored`.
- Shared BGRA letterboxing, YOLOv8 decoding, clipping, finite filtering, and
  class-aware NMS.
- Inference-only production policy; source/Debug training emits candidates and
  never installs them automatically.
- Promotion-bound Ed25519 model manifests, embedded trust roots, monotonic
  sequence floors, atomic replacement/recovery, and production-load validation.
- Local-only capture processing and privacy-minimized diagnostic policy.

### macOS

- ScreenCaptureKit capture, one explicitly selected display, negative-origin and
  pixel geometry, single-flight inpainting, stale work dropping, static-frame
  reuse, manual regions, local labeling, and frontmost-app/fullscreen pause.
- Bounded capture retry policy across sleep/display sleep/session lock, panic
  cancellation, and fail-closed permission/display resolution.
- Offline authenticated precompiled CoreML update installation and startup
  recovery.
- Credential-free release preflight plus credential-gated Developer ID,
  notarization, stapling, Gatekeeper, and checksummed packaging automation.

### Windows

- Windows Graphics Capture monitor frames, capacity-one processing, latest local
  labeling frame, shared ONNX processing, CPU mirror inpainting, telemetry, and
  conservative possible-protected/unavailable black-frame handling.
- Explicit physical-monitor selection, PerMonitorV2 physical geometry including
  negative origins, active topology refresh, and fail-closed monitor removal.
- Runtime-attested global hotkeys, sequenced panic behavior, tray controls, and a
  click-through/top-most render HWND with `WDA_EXCLUDEFROMCAPTURE`.
- Authenticated monotonic ONNX replacement/recovery.

### Linux

- Wayland ScreenCast portal plus PipeWire capture for mapped BGRA/BGRx, NV12,
  and YUY2 buffers; X11 virtual-root XComposite/MIT-SHM capture.
- Local ONNX CPU processing, CPU inpainting, private screenshot paths, and
  fail-closed capture state.
- Permissioned Wayland GlobalShortcuts and X11 passive grabs with runtime
  availability.
- KDE/wlroots GTK layer-shell and X11 XFixes click-through plumbing; GNOME uses
  a bounded decorated preview.
- Authenticated monotonic ONNX replacement/recovery.

## Known limitations and blockers

### Detector/distribution

- Human review is **0 approved / 21 pending**. AI-assisted annotations do not
  count as human evidence.
- No candidate passes the immutable schema-5 quality/preservation/parity gates.
- No exact equivalent promoted `.mlmodelc` and ONNX pair exists.
- Committed public-key rings are intentionally empty. Production model private
  keys and custody infrastructure do not exist in the repository.

### macOS

- Developer ID/notarization execute mode has not run with genuine credentials.
- Real one/two/three-display, scaling, hot-plug, ten-cycle sleep/lock, Spaces,
  fullscreen, accessibility, panic-latency, and 30-minute soak evidence is open.
- Current source model content is development/candidate material and cannot be
  represented as a promoted packaged detector.

### Windows

- Captured D3D textures are copied to CPU memory before ORT preprocessing.
  Zero-copy DirectML texture inference is not implemented.
- D3D11 compute inpainting/compositing and WARP fallback are not implemented;
  current inpainting and patch encoding are CPU paths.
- Bounded automatic restart after sleep, display/device loss, or driver reset is
  incomplete.
- Region editor and labeling UI are not complete end-to-end workflows.
- DirectML/CPU provider execution, NVIDIA/AMD/Intel performance, mixed-DPI
  overlays, capture exclusion, games, DRM, and anti-cheat behavior lack
  real-device evidence.
- Signed MSI/MSIX/NSIS production automation and Authenticode execution are open.

### Linux

- PipeWire DMA-BUF-only buffers and compositor-specific resize/revocation need
  implementation/evidence; current conversion covers mapped buffers.
- wgpu/WGSL inpainting/compositing is not implemented; current path is CPU.
- CPU ORT is the only packaging baseline. CUDA/ROCm/OpenVINO/TensorRT provider
  packaging and selection are open.
- Flatpak and native packages are not production-complete or signed.
- Real portal approval/revocation, layer placement, pointer click-through,
  multi-output, suspend, and compositor matrices are unverified.
- GNOME Wayland cannot provide feature-equivalent global click-through and is
  intentionally limited.

### General

- CI proves contracts, tests, and native compilation—not real capture, GPU,
  installer, signing, accessibility, power-loss, or hardware behavior.
- Opaque-black handling means **possible protected or unavailable content**; it
  is not definitive DRM detection.
- LiveBlock does not inject into applications, hook games, or bypass anti-cheat.
- Same-user arbitrary application-data replay and a compromised local process
  are outside the rollback-state threat boundary.

## Privacy

Frames, patches, regions, and labels remain local unless the user explicitly
exports/copies them. No analytics or remote crash reporter is configured.
Labeling screenshots are sensitive full-display images stored in the user's
application-data directory. See `PRIVACY_SECURITY_REVIEW.md` before requesting
support artifacts.

## Evidence before release

The exact procedures and artifact contract are in
`DESKTOP_VALIDATION_RUNBOOKS.md`. A public release remains blocked until the
advertised support rows have signed-package, clean-install/upgrade, permission,
display, lifecycle, crash-recovery, privacy, accessibility, performance, and
real-device evidence—or are downgraded to match what was actually verified.
