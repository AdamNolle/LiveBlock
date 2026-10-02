# LiveBlock handoff — 2026-10-01

The user requested pushing all work to GitHub and retaining only `main`.
The broad improvement goal is paused. Do not claim the app is perfect,
Apple notarized, or undetectable to DRM systems.

## This session's implementation and evidence

This session began from main commit `c151d18`. It modernized the SwiftUI and
shared desktop UI, hardened capture/overlay lifecycle and model loading,
trained an openly sourced starter ad detector, and built all three desktop
ports. There is no iOS app target. `validation/release-packages.json` records
exact artifact hashes and distinguishes compile checks from native acceptance.
If main is consolidated using the newer release branch, the session snapshot
must remain in Git history; its reports describe that snapshot, not the newer
implementation. Reconcile competing model contracts before moving weights.

### macOS

The universal Developer ID build is signed with team `RSDJXF7DXN`, uses hardened
runtime, and passed 17 tests. Native arm64 capture and manual covering were
verified. Universal compilation does not prove native Intel execution.
Local signed app: `/Users/adamnolle/Library/Caches/LiveBlock/release/LiveBlock.app`.
The signed ZIP is recorded in the release manifest. It is **not notarized**.
The last `notarytool history --keychain-profile LiveBlock` exited 69 because
that profile is missing. The user does not know an existing profile name.
Follow `NOTARIZATION.md`: have the user save Apple credentials locally; never
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
- Latest observed progress is in `validation/combined-v3-small/training-start.json`.
- Command: `tools/.venv/bin/python -u tools/train_logos.py --data tools/datasets/combined/yolo/data.yaml --model tools/pretrained/yolo11s.pt --epochs 30 --batch 2 --imgsz 960 --device mps --run-name combined-v3-small --patience 8 --screen-ads --no-int8`.

Do not launch a duplicate job, infer completion from old logs, or install an
unfinished checkpoint. Observe process and terminal results. Export the final
candidate only after completion; evaluate CoreML and both Rust ONNX adapters
on independent holdouts **by domain** at the actual deployment thresholds.
Training validation at an optimal threshold is not deployment acceptance.
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

Latest source additionally initializes actual Tauri GTK renderer/editor
windows using GTK layer-shell before realization. Compilation passed; release
build session 59153 runs in the owned Lima VM `liveblock-check`, with guest
log `/tmp/liveblock-wayland-layer-release.log`. Native application acceptance
is pending until explicitly recorded. The separately tested Python layer
probe proved fullscreen placement and click-through after remapping on Sway
1.11, not the application's capture pipeline. Start from
`tools/validate_wayland_overlay/README.md` and its validation report.
The host temporary fixture is `/tmp/liveblock-wayland-app-session.sh`.
The VM currently has no fixture viewer/app running. Do not stop it while the
release build remains live. Afterwards stop only this owned VM and close owned
viewer tabs. Do not kill unrelated processes or reset user privacy settings.

Verify the actual release renderer above fullscreen content, remap click-through,
editor keyboard/escape behavior and X11 regression before promoting the new
package. A headless single-output software-rendered result does not establish
multiple-output mapping, fractional scaling, hardware GPU behavior or portal
capture. The current Sway fixture lacks a GlobalShortcuts portal provider;
configure portal environment/services for real capture acceptance.
GNOME without layer-shell remains a degraded floating-window path.

Protected video can intentionally return black frames or reject capture.
Handle this transparently and fail closed. Do not promise invisible capture,
modify third-party players, or claim that overlay/capture settings defeat DRM.

## Next agent priorities

1. Confirm which Git snapshot main uses and read its release/support contracts.
   Preserve both implementations' evidence; don't mix their model formats blindly.
2. Finish Apple credential setup, notarization, stapling and Gatekeeper acceptance.
3. Observe the existing training job, evaluate completed exports by domain,
   promote only a candidate that passes recorded deployment gates.
4. Finish native Wayland application verification and native Windows acceptance.
5. Expand display/scale/fullscreen/runtime matrix; record precise limitations.

Local release binaries live outside Git in
`/Users/adamnolle/Library/Caches/LiveBlock/release/`. Hashes are in the manifest.
GitHub repository source is not equivalent to notarized release publication.
