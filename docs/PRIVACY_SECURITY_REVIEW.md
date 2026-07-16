# Desktop privacy and security review

**Review date:** 2026-07-14  
**Scope:** current `feat/desktop-release-completion` source tree  
**Evidence level:** source inspection plus automated contract tests; not a
network appliance audit, penetration test, signed-package review, or real-device
certification.

## Conclusions

- Desktop frame inference and inpainting are implemented in-process and no frame
  upload client exists in the macOS, Windows, Linux, or shared-frontend runtime
  source.
- Labeling screenshots are written only after explicit user action to fixed
  application-data directories. Labels and regions remain local unless the user
  separately exports/copies them.
- Desktop production packages are inference-only and do not bootstrap Python,
  pip, training code, runtimes, or replacement weights.
- Model activation is authenticated, promotion-bound, monotonic, atomic, and
  production-load-validated on each platform. The committed public-key rings are
  empty, so the repository cannot currently produce an honest distributable.
- Tauri screenshot/label IPC path values are constrained to immediate regular,
  non-symlink files in fixed, non-symlink screenshot/label roots. Arbitrary
  paths, traversal, nested paths, wrong extensions, special files, and candidate
  symlinks fail validation. The review found and removed the prior renderer-
  supplied arbitrary-file read/write/move surface.
- Unused Tauri filesystem, shell, dialog, and OS plugins are not registered or
  linked. Production webviews use a restrictive content-security policy rather
  than `csp: null`.

## Frame data flow

### macOS

`Sources/ScreenCaptureManager.swift` receives local `SCStream` BGRA buffers,
stores only request-driven detached labeling snapshots, invokes
`VisionProcessor` and `InpaintingEngine` in process, and sends generated patches
to local AppKit render windows. The screen callback has no URL/network client.
`Sources/DiagnosticsReport.swift` declares and tests that diagnostics contain no
frames, process lists, window titles, or user paths.

### Windows

`platform/windows/src-tauri/src/capture.rs` copies WGC textures into packed BGRA
memory and feeds a capacity-one local worker. `main.rs` calls the local ORT
adapter and CPU inpainter and emits only patch PNG data to the app's own Tauri
webview. The latest full frame is retained only by the active capture session
for an explicit labeling request and is cleared on stop/panic/failure.

### Linux

Wayland portal/PipeWire and X11 XShm sources produce packed local BGRA
`FrameView` values. `platform/linux/src-tauri/src/main.rs` retains the latest
frame only while active, runs local ONNX/CPU processing, and emits local patches.
Capture, latest-frame, and patch state clear on stop, panic, and source end.

A repository search of runtime Swift, Rust, TypeScript, and HTML found no
`URLSession`, `reqwest`, `TcpStream`, `WebSocket`, browser `fetch`, or XHR use.
This is source evidence, not proof against compromised dependencies or binaries.
CI/package review must repeat the search and inspect the final dependency and
bundle inventories.

## Local persistence and IPC boundaries

- macOS uses `TrainingPaths` beneath Application Support and native controllers
  derive screenshot/label paths rather than accepting renderer-provided paths.
  Snapshot requests and delivery are bound to one capture generation; stop/panic
  cancels pending continuations. The training root plus screenshot/label/export/
  trash directories are tightened to owner-only `0700` on every use. PNGs are
  encoded before create-new `O_EXCL|O_NOFOLLOW` `0600` writes, synchronized, and
  partial-cleaned. Discard moves the optional sidecar and screenshot without
  replacing an existing trash entry; screenshot failure rolls the sidecar back.
- Windows uses `%APPDATA%\LiveBlock`; Windows user-profile ACLs are the outer
  boundary. Screenshot creation is create-new, flushed, and synchronized.
- Linux uses `$XDG_DATA_HOME/LiveBlock`; directories are forced to `0700`, and
  new screenshot files are create-new `0600`, flushed, synchronized, and removed
  after partial-write failures.
- Shared `liveblock-config::validate_managed_file_path` canonicalizes the fixed
  parent and returns `root/file_name` only. Both Tauri adapters use it before
  listing, loading, saving labels, or moving screenshots to trash. Label
  sidecars must name an existing matching managed screenshot.
- Trash moves use atomic create-new semantics: Unix reserves a same-volume hard
  link before removing the source, while Windows rename fails when the
  destination exists. An existing destination is never replaced.
- macOS installs terminal barriers in the region store, labeling controller,
  and per-app exclusion store before hiding windows. Retained AppKit/editor
  callbacks cannot add/replace/delete regions, save/migrate/discard labels, or
  change exclusions afterward. Diagnostics rechecks the policy after its modal
  save panel, display selection rejects terminal actions, and delayed topology
  work rechecks after suspension so it cannot restart capture once native
  termination begins.
- Windows and Linux serialize region, labeling-screenshot, label-save, and
  discard mutations through one native gate. Quit sets its terminal flag before
  waiting on that gate: an already-admitted mutation completes before exit, and
  a queued renderer command fails after acquiring the gate rather than changing
  persisted data during terminal teardown. Runtime detector toggles also reject
  after the terminal flag.
- Model update source paths are explicit user-selected package inputs, but the
  destination is fixed by native code. Content is accepted only after embedded
  keyring signature, exact artifact hash, sequence, schema, and production-load
  checks. Update packages cannot add trust roots.

Validation and later file access are separate pathname operations. A malicious
same-user process can race by replacing a validated file; this is part of the
explicit same-user compromise exclusion, not claimed symlink-race resistance.
The application does not claim protection from a malicious process already
running as the same user. Such a process can read application data, replay files,
or modify the process just as it can disable/replace the app.

## Webview and command surface

Windows and Linux load packaged local HTML/TypeScript. Their production CSP is:

```text
default-src 'self'; connect-src ipc: http://ipc.localhost;
img-src 'self' data:; style-src 'self' 'unsafe-inline'; script-src 'self'
```

Inline styles are currently required by the local UI; inline scripts and remote
content are not allowed. Unused Tauri shell/filesystem/dialog/OS plugins were
removed. Custom commands remain the exposed native surface and therefore require
path and state validation even though the UI is local. Renderer window commands
are allowlisted to editor, labeling, and training; render/control surfaces stay
native-owned. Window opens and capture transitions carry monotonic native action
sequences so requests allocated before a later stop/panic cannot arrive late.

The CSP and removed plugins reduce impact; they do not prove the absence of
WebView2/WebKit vulnerabilities. Final packages need dependency/advisory scans
and bundle inventory review.

## Diagnostics, logs, and telemetry

- Runtime capability profiles set `telemetryEnabled: false`.
- No analytics or remote crash reporter is configured.
- macOS diagnostic JSON is deliberately privacy-minimized and covered by tests.
- Windows/Linux expose counters and conservative error strings to local UI only;
  they do not write frame pixels to logs.
- Native library/runtime logs may contain generic filesystem or model-load
  errors. Release validation must inspect logs and diagnostics with paths and
  usernames designed to detect accidental leakage.

No support workflow should request raw full-screen labeling PNGs by default.
Operators must obtain explicit consent before a user exports any screenshot,
label, or private dataset.

## Training and model distribution

`docs/TRAINING_RUNTIME.md` defines release packages as inference-only. Source
training outputs candidates and cannot install them. `tools/verify_promotion.py`
owns immutable schema-5 quality/preservation/latency gates; signers and installers
rerun that gate rather than trusting `passed: true` text.

The complete activation/rollback threat model is in
`MODEL_DISTRIBUTION_SECURITY.md`. Important boundaries:

- embedded nonempty public-key ring; no downloaded/user-imported roots;
- signed manifest binds exact artifact, promotion report, runtime contract, and
  increasing release sequence;
- fixed application-data destinations and advisory update locks;
- production runtime load before commit and startup reauthentication/recovery;
- packaged authenticated release floor;
- same-user arbitrary replay and compromised process are out of scope;
- no hardware-backed non-replayable counter exists.

Authentication does not substitute for detector quality. No model may be signed,
installed, or packaged until human-only leakage-safe schema-5 evidence and exact
CoreML/ONNX parity artifacts exist.

## Automated evidence

From repository root:

```bash
cargo test --manifest-path core/Cargo.toml --workspace --lib
python3 -m unittest tools.test_desktop_contracts tools.test_release_docs
PYTHONPATH=tools:tools/eval python3 -m unittest tools.test_model_distribution
```

Native CI additionally compiles Windows/Linux Debug and Release paths, proves
empty production rings fail without the explicit CI override, and runs platform
library tests. macOS tests cover diagnostic redaction and authenticated CoreML
transactions. See `docs/CI.md` for what green CI does not prove.

## Residual risks and required release evidence

1. No production signing keys, promoted detector, signed installers, or
   production package inventories exist yet; existing inventories are BuildOnly.
2. Network-denied real-device functional tests and outbound traffic observation
   have not been run.
3. Windows application-data privacy relies on inherited user-profile ACLs; a
   clean-install ACL inspection is required.
4. Windows/Linux drawing/save/navigation is implemented, but real-device
   keyboard, accessibility, display-change, and panic-race evidence is open.
5. Dependency/SBOM/package gates run in CI, but five prepared MPL obligations
   still require accountable production-channel approval.
6. Portal/compositor, GPU provider, anti-cheat, DRM, multi-display, lifecycle,
   panic-latency, and power-loss claims remain hardware-blocked.
7. CSP still allows local inline styles; remove them when UI refactoring makes
   that practical.

Until those items have retained evidence, capability profiles and release notes
must remain experimental/not release-ready.
