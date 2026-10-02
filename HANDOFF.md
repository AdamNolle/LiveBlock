# LiveBlock handoff — 2026-10-01

The user requested pushing all work to GitHub and retaining only `main`.
The broad improvement goal is paused. Do not claim the app is perfect,
Apple notarized, or undetectable to DRM systems.

## This session's implementation and evidence

This session began from main commit `c151d18`. It modernized the SwiftUI and
shared desktop UI, hardened capture/overlay lifecycle and model loading,
trained an openly sourced starter ad detector, and built all three desktop
ports. There is no iOS app target. The snapshot
[release manifest](https://github.com/AdamNolle/LiveBlock/blob/31d0195/validation/release-packages.json) records
exact artifact hashes and distinguishes compile checks from native acceptance.
Main is being consolidated using the newer release branch at `54c7756`.
The complete session snapshot is commit `31d0195`, retained as a merge parent.
Its reports describe that snapshot, not the newer implementation. Reconcile
competing model contracts before moving weights.

Retrieve the full snapshot without creating a branch: `git archive 31d0195 |
tar -x -C /path/to/an/empty/snapshot-directory`. Its source, training tools,
model card, notarization runbook and validation reports are available there.
The current main release contract is in `docs/DESKTOP_RELEASE_MATRIX.md`;
its notarization runbook is `docs/MACOS_RELEASE.md`. Do not use stale `AI.md`
or older assessments as the source of current implementation facts.

### macOS

The universal Developer ID build is signed with team `RSDJXF7DXN`, uses hardened
runtime, and passed 17 tests. Native arm64 capture and manual covering were
verified. Universal compilation does not prove native Intel execution.
Local signed app: `/Users/adamnolle/Library/Caches/LiveBlock/release/LiveBlock.app`.
The signed ZIP is recorded in the release manifest. It is **not notarized**.
The last `notarytool history --keychain-profile LiveBlock` exited 69 because
that profile is missing. The user does not know an existing profile name.
Follow the snapshot `NOTARIZATION.md` and current `docs/MACOS_RELEASE.md`: have the user save Apple credentials locally; never
request passwords, private keys, or app-specific passwords in chat. Then run
`tools/release_macos.sh --notarize LiveBlock`, verify Apple's Accepted result,
staple, and verify Gatekeeper. Signing alone is not notarization.

### Installed model and training

The installed AdSERP starter has independent held-out search-ad results:
compiled CoreML at confidence 0.5 and NMS IoU 0.45: TP 941, FP 3, FN 8;
Rust ONNX adapter: TP 940, FP 3, FN 9, over 573 images. Those results do not
establish reliable sports-logo detection or arbitrary ad blocking.
`MODEL_CARD.md`, `models/ATTRIBUTION.md` and validation reports record scope,
provenance, dataset attribution and separate model/software license obligations.
Combined v1 and v2 were rejected. Do not install them.

A larger combined-v3-small training job is still running locally:

- PID 46356; original exec session 16945.
- Log: `/tmp/liveblock-combined-v3-training.log`.
- Results: `tools/runs/combined-v3-small/results.csv`.
- Checkpoints: `tools/runs/combined-v3-small/weights/`.
- Last confirmed completed epoch: 17 of 30; the process remains active.
- Earlier observation report is in the snapshot `validation/combined-v3-small/training-start.json`.
- Command: `tools/.venv/bin/python -u tools/train_logos.py --data tools/datasets/combined/yolo/data.yaml --model tools/pretrained/yolo11s.pt --epochs 30 --batch 2 --imgsz 960 --device mps --run-name combined-v3-small --patience 8 --screen-ads --no-int8`.

Do not launch a duplicate job, infer completion from old logs, or install an
unfinished checkpoint. Observe process and terminal results. Export the final
candidate only after completion; evaluate CoreML and both Rust ONNX adapters
on independent holdouts **by domain** at the actual deployment thresholds.
Training validation at an optimal threshold is not deployment acceptance.
The running trainer loaded the snapshot code before branch consolidation.
Its final automatic export imports helpers from the tools directory at that
time; newer main has different helpers. If that export fails or uses different
input dimensions, retain completed best.pt and export manually with the
snapshot tooling in an isolated directory. No --install was requested.
Sports v2 recall was roughly 48%, so improvement remains necessary.

Datasets, virtual environments, training outputs and downloaded pretrained
checkpoints are ignored local files, not pushed raw data. Reproduction tools:
`prepare_adserp.py`, `prepare_exposure.py`, `prepare_combined_ads.py`.
Sources are AdSERP (Zenodo 15236546, CC BY 4.0) and ExposureEngine
(SimulaMet-HOST, pinned revision recorded in provenance, declared MIT).
Retain query/clip grouped splits, deduplication, and search negatives.
Do not fabricate human labels or treat every sponsor/logo as a contextual ad.

### Windows and Linux

Windows x64 is cross-built with `custom-protocol`, its embedded UI and packaged
file hashes were checked. Native Windows/WebView2/DirectML execution remains
unverified; the portable build is unsigned. Use an actual Windows machine for
capture, protected output, panic, editor, inference and installation acceptance.

Linux ARM64 previously passed native X11 cover/editor/hotkey/panic and
compositor-loss checks in an isolated Openbox/xcompmgr display. Reports bind
those observations to exact binary hashes. Ubuntu 26.04 build requires glibc
2.43; do not advertise compatibility with older distributions.

The session snapshot additionally initializes the actual Tauri GTK renderer/editor
windows using GTK layer-shell before realization. The release compile retry
passed, the package installed, and native application checks passed on Sway:
cover-only Start, editor above fullscreen, Escape, precise cover placement,
initial click-through, Stop, restart/remap, and click-through after remapping.
The underlying test counter reached 1 initially and 2 after remapping.
See [the exact application report](docs/session-2026-10-01/wayland-app-cover.json).
This report applies to snapshot `31d0195`, not the different current main source.
The snapshot package is retained separately in the local release directory as
`LiveBlock-linux-arm64-session-31d0195-layer-shell.deb`; it does not replace the
previously recorded package or establish a release for current main.

The initial release build hit VM memory pressure. A retry succeeded with a
1 GiB disk-backed swap file, subsequently removed. The viewer and owned app,
fixture, compositor and network processes have been stopped. The owned VM is confirmed Stopped. Graceful Lima shutdown timed out after
fixture cleanup; a scoped force-stop completed successfully. Temporary fixture files were in `/tmp`; no user display or
privacy setting was changed.

Next verify portal capture, global shortcuts with an actual provider, X11
regression for this new package, multiple-output mapping, fractional scaling
and hardware GPUs. The headless Sway single-output cover test does not prove
those paths. GNOME without layer-shell remains a degraded floating-window path.

Protected video can intentionally return black frames or reject capture.
Handle this transparently and fail closed. Do not promise invisible capture,
modify third-party players, or claim that overlay/capture settings defeat DRM.

## Next agent priorities

1. Confirm which Git snapshot main uses and read its release/support contracts.
   Preserve both implementations' evidence; don't mix their model formats blindly.
2. Finish Apple credential setup, notarization, stapling and Gatekeeper acceptance.
3. Observe the existing training job, evaluate completed exports by domain,
   promote only a candidate that passes recorded deployment gates.
4. Extend Wayland acceptance to portal capture, display/scale cases and
   hardware; verify the current main implementation and native Windows.
5. Expand display/scale/fullscreen/runtime matrix; record precise limitations.

Local release binaries live outside Git in
`/Users/adamnolle/Library/Caches/LiveBlock/release/`. Hashes are in the manifest.
GitHub repository source is not equivalent to notarized release publication.


## Consolidation verification

Merge `2dbee79` has parents `31d0195` (this session) and `54c7756` (existing
release implementation). The latter also contains the former open-vocabulary
branch head `37f8e2d`. Both remote feature branches were deleted only after
confirming all three heads are ancestors of main. The GitHub default branch
and sole local/remote branch are `main`. The main working tree differs from
the existing release implementation only by these handoff notes and local
artifact ignore rules; no competing app sources were mixed automatically.

[Desktop CI](https://github.com/AdamNolle/LiveBlock/actions/runs/36953691194)
for `2dbee79`: shared contract checks, macOS 26 build/tests, Ubuntu compilation, Fedora RPM
lifecycle and Flatpak lifecycle passed. Windows compilation was still running
when last checked; the next agent should inspect its final result. This does not
prove notarization, real-device Windows acceptance or model promotion.
