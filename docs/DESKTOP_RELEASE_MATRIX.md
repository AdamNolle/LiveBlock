# LiveBlock desktop release matrix

This document defines what “supported” means for a desktop release. A platform
is not promoted from a lower tier because it compiles; it must satisfy the
measurable gates below on the named environment.

## Support tiers

- **Tier 1 — supported:** release-blocking CI plus a recorded real-device run.
  Regressions block a release.
- **Tier 2 — compatible:** build and focused tests pass, but hardware/compositor
  coverage is incomplete. Known limitations are documented and do not block a
  Tier 1 release.
- **Tier 3 — best effort:** architecture or OS policy prevents full behavior.
  The app must fail clearly and safely rather than claim feature parity.
- **Unsupported:** no release artifact or compatibility claim.

## Platform matrix

| Platform | Environment | Capture | Inference | Overlay | Target tier |
|---|---|---|---|---|---|
| macOS | macOS 26 on Apple silicon supported by Apple | ScreenCaptureKit | CoreML (ANE/GPU/CPU selected by CoreML) | Screen-aligned click-through AppKit layer | Tier 1 |
| macOS | macOS 26 on an Apple-supported Intel Mac | ScreenCaptureKit | CoreML CPU/GPU | Screen-aligned click-through AppKit layer | Tier 2 until real hardware evidence exists |
| Windows | Windows 11 23H2+ x64, NVIDIA GPU | Windows Graphics Capture | ONNX Runtime DirectML, CPU fallback | D3D11 click-through and capture-excluded | Tier 1 |
| Windows | Windows 11 23H2+ x64, AMD GPU | Windows Graphics Capture | ONNX Runtime DirectML, CPU fallback | D3D11 click-through and capture-excluded | Tier 1 |
| Windows | Windows 11 23H2+ x64, Intel GPU | Windows Graphics Capture | ONNX Runtime DirectML, CPU fallback | D3D11 click-through and capture-excluded | Tier 1 |
| Windows | Windows 11 23H2+ x64, software-only | Windows Graphics Capture | ONNX Runtime CPU | D3D11/WARP fallback | Tier 2; reduced performance is disclosed |
| Windows | Windows 10 1903+ x64 | Windows Graphics Capture where available | DirectML/CPU | D3D11 overlay | Tier 2 while low-cost compatibility remains; not a release gate |
| Linux | KDE Plasma Wayland on a currently supported Ubuntu/Fedora release | xdg-desktop-portal + PipeWire | ONNX Runtime CPU; optional provider | layer-shell click-through | Tier 1 |
| Linux | wlroots compositor (Sway; Hyprland compatibility tested separately) | xdg-desktop-portal + PipeWire | ONNX Runtime CPU; optional provider | wlr-layer-shell click-through | Tier 1 |
| Linux | Supported X11 session | XComposite + XShm | ONNX Runtime CPU; optional provider | X11 click-through overlay | Tier 1 while distributions ship X11 |
| Linux | GNOME/Mutter Wayland | xdg-desktop-portal + PipeWire | ONNX Runtime CPU; optional provider | visible movable limited-mode window | Tier 3; Mutter does not permit feature-equivalent global click-through overlays |

Optional Linux CUDA, ROCm, OpenVINO, and TensorRT providers are Tier 2 until each
provider is exercised on real matching hardware. CPU inference is the portable
fallback and packaging baseline.

## Universal release gates

Every Tier 1 row must provide evidence for all applicable gates.

### Build and installation

1. A clean hosted or documented clean-machine build succeeds from the tagged
   commit with a locked dependency graph.
2. The native installer/package installs, launches, upgrades from the previous
   release, and uninstalls without deleting user data unless explicitly chosen.
3. Release artifacts and update/model manifests are cryptographically signed.
   Dry-run signing automation is acceptable during development; a public
   release is blocked until real platform credentials verify successfully.
4. No secret, private dataset, local credential, or unlicensed model is present
   in the repository or release archive.

### Functional behavior

1. First-run capture/accessibility/portal onboarding either succeeds or presents
   a specific recoverable error.
2. Start, stop, panic-disable, region editing, settings persistence, and local
   labeling work after restart.
3. The overlay is aligned at 100%, 125%, 150%, 175%, and 200% scaling where the
   platform exposes those modes.
4. A primary-display change and display hot-plug are recovered without a crash,
   stale overlay, or capture recursion.
5. Sleep/wake or lock/unlock is exercised for 10 cycles; capture resumes or
   remains explicitly paused according to settings.
6. DRM/blocked capture produces an explicit safe state rather than painting
   black or stale content over the desktop.

### Quality and preservation

The only installable detector is one backed by a complete passing schema-5
promotion report. Immutable minimum policy:

- aggregate precision >= 0.50;
- aggregate recall >= 0.50;
- recall >= 0.50 for every required placement;
- aggregate false positives <= 10;
- exactly zero false positives on every required preservation and contextual
  hard-negative slice;
- runtime classes remain exactly `Logo`, `Ad banner`, and `Sponsored`;
- human evidence is attributable and leakage-safe; AI-assisted labels never
  count as human review.

CoreML and ONNX exports must pass the same fixed-fixture preprocessing,
postprocessing, class-order, threshold, and box-coordinate parity tests.

### Performance and reliability

1. Render/inpaint p95 remains below the 16.67 ms 60 Hz frame budget under the
   documented reference workload; stale work is dropped rather than queued.
2. Detector p95 is <= 10 ms at the promotion benchmark size on the macOS
   reference device. Windows/Linux budgets are recorded per backend before
   Tier 1 promotion and must sustain the configured detection cadence.
3. A 30-minute capture soak has no unbounded memory growth, deadlock, persistent
   stale overlay, or unrecovered stream failure.
4. Panic disable removes overlays and stops capture within 500 ms.
5. Crash/restart never installs a partial model, settings file, or update.

### Privacy and security

1. Frame pixels and labels remain local unless the user explicitly exports
   them. A network-denied functional test must still pass.
2. Model and application updates reject altered or stale signed manifests.
3. Paths supplied by UI/IPC are constrained to the application data roots.
4. Logs and diagnostic exports contain no frame pixels by default.

### Accessibility and documentation

1. Primary controls are keyboard reachable and expose meaningful accessibility
   labels on each platform.
2. Reduced-motion/high-contrast behavior and 200% UI scaling are checked.
3. Known DRM, anti-cheat, compositor, GPU, and permission limitations are
   visible in both release notes and platform documentation.

## Evidence states

Matrix evidence must use one of these labels:

- `verified-real-device`
- `verified-hosted-runner`
- `build-only`
- `blocked-credentials`
- `blocked-hardware`
- `unsupported-by-platform`

An empty cell never implies success. GitHub milestone issues own the evidence for
each Tier 1 row, and the draft release PR remains open until all release gates
are either verified or explicitly moved out of the advertised support tier.
