# Desktop release validation runbooks

These runbooks define the manual and automated evidence required before a
platform may move to a supported release tier. A successful source build is not
installation, permission, GPU, compositor, or lifecycle evidence.

## Evidence contract

Run from a clean checkout of the exact candidate commit. Create one directory
per run outside the signed bundle (the repository path is gitignored):

```text
tools/runs/desktop-validation/<commit>/<platform>/<UTC timestamp>/
  environment.json
  commands.log
  scenario-results.json
  screenshots/             # UI state only; never captured private content
  diagnostics.json         # LiveBlock privacy-minimized export, where available
  artifacts.sha256
```

`environment.json` records the exact commit, dirty status, evidence state, OS
build, hardware/GPU, displays and scaling, and package hash. The helper rejects
a dirty tree unless its hidden test-only override is explicitly used, and dirty
evidence can only be labeled `build-only`. `scenario-results.json` records
`scenario`, `startedAt`, `finishedAt`, result (`pass`, `fail`, or `blocked`),
evidence filenames, and operator identity. Do not store frame
captures, labels, credentials, private model keys, or user/window names in this
directory. Hash every retained package and log with the platform SHA-256 tool.

Allowed evidence labels remain those in `DESKTOP_RELEASE_MATRIX.md`:
`verified-real-device`, `verified-hosted-runner`, `build-only`,
`blocked-credentials`, `blocked-hardware`, and `unsupported-by-platform`.
Initialize, record, and seal a run with the checked-in helper:

```bash
RUN="tools/runs/desktop-validation/$(git rev-parse HEAD)/macos/$(date -u +%Y%m%dT%H%M%SZ)"
python3 tools/desktop_validation_evidence.py init \
  --platform macos --evidence-state build-only --output "$RUN" \
  --os-build "$(sw_vers -buildVersion)" --hardware "$(uname -m)" \
  --gpu "not-recorded" --displays "not-recorded"
STARTED="$(date -u +%Y-%m-%dT%H:%M:%SZ)"
if cargo test --manifest-path core/Cargo.toml --workspace --lib \
    >"$RUN/commands.log" 2>&1; then
  RESULT=pass
else
  RESULT=fail
fi
FINISHED="$(date -u +%Y-%m-%dT%H:%M:%SZ)"
python3 tools/desktop_validation_evidence.py record --run-dir "$RUN" \
  --scenario common-source-preflight --result "$RESULT" --operator "$USER" \
  --started-at "$STARTED" --finished-at "$FINISHED" \
  --notes "build-only; no hardware claim" --evidence commands.log
python3 tools/desktop_validation_evidence.py hashes --run-dir "$RUN"
test "$RESULT" = pass
```

Use `windows` or `linux` and the actual evidence state for other targets. A
`pass` requires at least one nonempty confined evidence file. A `blocked` result
requires an explanatory note, and `blocked-credentials`, `blocked-hardware`, or
`unsupported-by-platform` runs cannot record `pass` or `fail`. The helper
strictly reparses the environment and every result before sealing, rejects empty
or duplicate evidence paths plus links/special files anywhere in the run,
updates results atomically, and creates `artifacts.sha256` without overwriting an
earlier seal. Once sealed, additional scenario records are rejected.

## Common source preflight

From the repository root:

```bash
git status --short --branch
git rev-parse HEAD
python3 -m unittest tools.test_desktop_contracts tools.test_release_docs
cargo test --manifest-path core/Cargo.toml --workspace --lib
(cd platform/_shared-frontend && npm ci && npm run build)
```

Production package validation additionally requires a passing schema-5 detector,
exact promoted CoreML/ONNX artifacts, nonempty protected public-key rings, and
platform signing credentials. The committed empty rings and the explicit CI
override are never acceptable in a distributable.

## macOS 26

### Build and clean install

1. Run `tools/release_macos.sh --dry-run`; retain the log as preflight only.
2. In protected release infrastructure, provide the variables documented in
   `MACOS_RELEASE.md` and run `tools/release_macos.sh --execute`.
3. Verify the emitted SHA-256 before copying the notarized zip to a clean test
   Mac. Expand it, drag `LiveBlock.app` to `/Applications`, and run:

   ```bash
   codesign --verify --deep --strict --verbose=2 /Applications/LiveBlock.app
   spctl --assess --type execute --verbose=4 /Applications/LiveBlock.app
   xcrun stapler validate /Applications/LiveBlock.app
   ```

4. Launch from Finder. Record Gatekeeper result and application version. Never
   use `xattr -d com.apple.quarantine` in release evidence.

### Upgrade and uninstall

Install the previous signed release, create one manual region and setting, then
replace the app with the candidate through the documented distribution method.
Verify settings/regions and accepted monotonic model state survive. Verify a
lower/equal model sequence is rejected. Move the app to Trash and confirm user
data is retained until the operator explicitly deletes
`~/Library/Application Support/LiveBlock`.

### Permissions and lifecycle

Exercise Screen Recording and Accessibility in this order: initial denial,
relaunch, grant, active revocation, recovery, and a second relaunch. Perform 10
cycles each of sleep/wake and lock/unlock while capture is active, plus Spaces
and fullscreen transitions. Capture must resume only when intent remains active;
panic or quit during suspension must prevent resurrection. During a controlled
topology transition that temporarily exposes no usable display, require capture
to stop, patches and detections to clear, the render surface to hide, and a
specific unavailable-display error; reconnecting a display may resume only if
intent remains active. Start a labeling snapshot request immediately before
panic, then restart capture and prove that no pre-panic request consumes a frame
or creates a screenshot from the new capture generation. Enable automatic
labeling capture, arrange for a queued auto-capture callback and the delayed onboarding
**Try your first block** action immediately before panic, and verify neither can
restart capture nor reopen a privacy-visible window afterward. Panic must turn
auto-capture off. Repeat while stream teardown is deliberately slow and verify
render/editor/labeling/training/HUD windows hide before teardown completes. In a
source/developer build, quit once during environment setup and once during an
active training/export subprocess. Verify the exact owned process tree is
terminated, no cancellation completion overwrites the terminal state, and no
stale callback launches or terminates a later workflow. Repeat both through the
in-app Quit action and the native Command-Q/application-termination path; both
must return `terminateLater` until capture and owned background teardown are
ready. Signed release packages must expose no source-training runtime.

### Assistive technology

With VoiceOver and Full Keyboard Access enabled, traverse onboarding, the control
panel, Settings, region/class/app toggles, and the Mini HUD without a pointer.
Every custom toggle must announce its purpose and current On/Off value; critical
actions must expose the stable identifiers checked by
`tools/test_macos_accessibility.py`. With **Reduce motion** enabled, toggle slides
must change without animation and status dots must not pulse. Confirm focus is
visible, labels do not depend on color, dynamic frame status announces measured
state rather than a fixed cadence, and both required permission explanations
match their implemented use. Source tests and hosted compilation prove only the
semantic seams; this manual VoiceOver/keyboard pass remains required before
accessibility certification.

### Displays, panic, and soak

Test one, two, and three displays, primary-display changes, negative origins,
hot-plug, and every available scale corresponding to 100/125/150/175/200%.
Record selected display and overlay alignment without including private pixels.
Panic must clear overlays/capture within 500 ms. Run the reference workload for
30 minutes and retain diagnostics plus memory/latency observations. Real Apple
hardware is mandatory for these claims.

## Windows 11 x64

### Build-only preflight

On a Windows runner or developer machine with Rust, Node, and MSVC:

```powershell
npm --prefix platform\_shared-frontend ci
npm --prefix platform\_shared-frontend run build
cargo test --manifest-path platform\windows\src-tauri\Cargo.toml --locked --lib
cargo check --manifest-path platform\windows\src-tauri\Cargo.toml --locked --all-targets
$env:LIVEBLOCK_ALLOW_EMPTY_MODEL_KEYRING='1'
cargo check --manifest-path platform\windows\src-tauri\Cargo.toml --locked --release --all-targets
Remove-Item Env:LIVEBLOCK_ALLOW_EMPTY_MODEL_KEYRING
```

The override makes this non-distributable. A production build must use the exact
promoted ONNX artifact, nonempty ring, Authenticode identity, signed MSI plus
the selected staged NSIS update bundle. MSIX/App Installer is not a supported
channel.

### Clean install, upgrade, and uninstall

On a Windows Sandbox or clean VM, verify the complete staged bundle offline,
then run the signed NSIS installer normally (never bypass SmartScreen):

```powershell
pwsh -File tools/verify_windows_application_update.ps1 `
  -BundleDir $env:LB_WINDOWS_UPDATE_BUNDLE `
  -ExpectedSignerCertificateSha256 $env:LB_WINDOWS_EXPECTED_SIGNER_SHA256 `
  -CurrentVersion $env:LB_WINDOWS_CURRENT_VERSION
Get-AuthenticodeSignature $env:LB_WINDOWS_INSTALLER | Format-List *
Get-FileHash -Algorithm SHA256 $env:LB_WINDOWS_INSTALLER
```

Launch as a standard user. Upgrade from the previous signed release and verify
regions/settings plus authenticated model sequence survive. Confirm an old model
update and installer downgrade are rejected. Uninstall through **Installed
apps** and verify `%APPDATA%\LiveBlock` is retained unless explicit data removal
was selected.

### Permissions, displays, lifecycle, and panic

Test WGC consent/availability, denial, monitor removal during startup and active
capture, orientation/resolution changes, primary changes, negative origins, and
mixed 100/125/150/175/200% scaling. Repeat sleep/wake, display sleep, lock/unlock,
and display-driver reset 10 times. Native lifecycle observation and bounded
recovery are implemented but uncertified; record any failed or manually restarted
scenario as failed/blocked rather than calling it a pass. Verify click-through, top-most behavior, and
`WDA_EXCLUDEFROMCAPTURE` with an independent screenshot/capture tool. Panic must
win against active inference, editor/label actions, and quit within 500 ms.

Exercise NVIDIA, AMD, Intel, and CPU configurations separately. Record the
actual ORT provider selected; configured DirectML does not prove it executed.

### Protected/unavailable content and permitted game environments

Use only a synthetic fullscreen test surface for the deterministic black-frame
scenario; never retain protected media, game frames, window/process names, or
private desktop content. While capture is active:

1. establish visible test content and at least one overlay patch;
2. replace it with an opaque RGB `(0,0,0)` fullscreen surface for at least eight
   processed frames;
3. require **Possible protected or unavailable content · overlays paused**, an
   empty patch set, and a hidden render surface (verify with an independent
   screenshot/capture tool);
4. restore visible test content for at least three processed frames and require
   the render surface to return without a stale patch; and
5. repeat with dark-but-nonblack synthetic content and require no protected-state
   transition.

Record this as `windows-protected-unavailable-safe-state` in a
`verified-real-device` run and retain only a sanitized command/test log plus UI
state evidence. A synthetic black surface validates the conservative safe-state
mechanism, **not** DRM detection. Any true protected-content result is separately
hardware/vendor evidence and must use content the operator is authorized to test.

Do not run or claim game/anti-cheat compatibility without the game vendor's
permitted test environment. When no such environment is available, create a
separate `blocked-hardware` run, record scenario
`windows-vendor-permitted-game-anti-cheat`, result `blocked`, and explain the
missing environment in `--notes`; the evidence helper will reject a pass under
that state. LiveBlock never injects into applications, hooks games, or attempts
an anti-cheat bypass.

## Linux

### Native package build-only preflight

On the target distribution after the documented package prerequisites:

```bash
npm --prefix platform/_shared-frontend ci
npm --prefix platform/_shared-frontend run build
cargo test --manifest-path platform/linux/src-tauri/Cargo.toml --locked --lib
cargo check --manifest-path platform/linux/src-tauri/Cargo.toml --locked --all-targets
LIVEBLOCK_ALLOW_EMPTY_MODEL_KEYRING=1 \
  cargo check --manifest-path platform/linux/src-tauri/Cargo.toml --locked --release --all-targets
```

The override is development-only. Production `.deb`, `.rpm`, AppImage, or
Flatpak evidence requires a promoted ONNX model, nonempty ring, packaged ORT CPU
runtime, signed repository/package metadata, and no training runtime.

### Clean install, upgrade, and uninstall

Use a clean VM/user for each supported distribution and compositor. Install with
the native package manager or Flatpak command, retain package-manager signature
output and package SHA-256, launch without a development shell, upgrade from the
previous signed package, and verify schema/model state. Uninstall and confirm
`$XDG_DATA_HOME/LiveBlock` remains unless explicit data deletion was requested.
Flatpak validation is portal-only; never count host X11 fallback as sandbox
evidence.

### Portal/session, displays, lifecycle, and panic

For KDE Wayland and each wlroots target, test portal denial, approval, remembered
selection, revocation, PipeWire stream end, format negotiation, resize, and
DMA-BUF-only offers. Revocation or an unexpected post-connect `Unconnected`
state must clear frames/patches, hide rendering, publish stopped, and surface a
specific local error; an explicit stop must not publish that error. A valid
renegotiated format/size must replace the old dimensions. Format removal must
clear the active layout immediately, allow at most two seconds for a replacement,
and then fail closed; malformed, unsupported, zero, or over-16384 format data
must fail immediately rather than process a stale layout. For X11, test
MIT-SHM/XComposite availability, root geometry/stride changes, and a real
`XGrabKey` conflict. A virtual-root resize must clear old output before the
pre-created replacement SHM mapping publishes a new frame; mapping/reply failures
must stop rather than reuse stale dimensions. Test multi-output layouts,
negative origins where exposed,
scaling, hot-plug, 10 suspend/resume and lock/unlock cycles, and compositor
restart. Verify layer-shell/XFixes click-through using an independent pointer
observer. GNOME evidence must use the bounded movable preview and must not claim
click-through parity. Panic must immediately hide render state, then complete
source teardown within 500 ms. Start a labeling capture immediately before each
stop/panic/restart probe and verify generation invalidation prevents a stale
capture task from publishing patches or retaining a screenshot written after
that boundary.

CPU is the only packaging baseline today. CUDA, ROCm, OpenVINO, and TensorRT are
blocked until provider libraries are packaged and selected-provider evidence is
recorded on matching hardware.

## Crash-recovery scenarios (all platforms)

Run each scenario only against disposable test data and retain before/after
hashes:

1. terminate the process during settings/regions/label atomic replacement;
2. terminate during model staging, artifact swap, production load, and signed
   state commit;
3. restart with an interrupted first model install and with a valid accepted
   backup;
4. restart with a tampered artifact, manifest, state, symlink, and future schema;
5. run two concurrent updater attempts and verify one advisory lock winner.

Expected result: no partial document is accepted; authenticated previous state is
restored where defined; future/malformed state remains untouched; no fallback to
an older unauthenticated model occurs. Process termination tests are not a
substitute for power-loss/filesystem testing on real target machines.

## Completion rule

A checklist row may be closed only when every required scenario has a retained
result and the advertised support tier matches that evidence. Missing hardware,
credentials, promoted models, package signing, or portal access is recorded as a
blocker—not inferred from CI or source inspection.
