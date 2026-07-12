# Desktop behavior and detector processing contract

Rust is the source of truth for behavior that must not drift across macOS,
Windows, and Linux. `liveblock-config::desktop_contract` publishes capability
profiles and user-visible behavior; `liveblock-detection` owns detector pixel
preprocessing and YOLOv8 postprocessing.

## Fixed behavior

Runtime classes, in model-index order:

1. `Logo`
2. `Ad banner`
3. `Sponsored`

| Action | macOS | Windows/Linux |
|---|---|---|
| Toggle capture | Command+Shift+L | Control+Shift+L |
| Toggle region editor | Command+Shift+B | Control+Shift+B |
| Capture for labeling | Command+Shift+S | Control+Shift+S |
| Panic disable | Command+Shift+Option+Period | Control+Shift+Alt+Period |

Panic disable must clear capture intent, cancel recovery, clear overlays, and
close the editor. Runtime inference and frame processing stay local; frame
telemetry is not networked. Production packages are inference-only.

Windows and Linux expose their validated profile through the
`get_capabilities` Tauri command. Profiles report current software state, not
the target roadmap: incomplete Windows/Linux paths are `limited`, experimental
or not implemented backends are named explicitly, and `releaseReady` remains
false. Unknown Linux sessions/compositors fail the capability check. GNOME
Wayland also reports no global click-through overlay. macOS reads the same Rust
contract through the Swift bridge and validates it during controller startup;
its release readiness remains false pending the external evidence matrix.

## Detector processing v1

`DetectorProcessingContract::YOLOV8_BGRA_640` fixes:

- packed BGRA8 source input;
- nearest-neighbor aspect-preserving letterbox to 640x640;
- padding value 114;
- RGB channel order, NCHW layout, and `[0,1]` normalization;
- YOLOv8 `[1, 4 + classes, anchors]` `cx,cy,w,h` output;
- reversal to normalized top-left source coordinates;
- finite-score filtering; and
- class-aware NMS.

Windows and Linux adapters now call the shared preprocessing and decoder rather
than maintaining separate algorithms. CoreML/Vision still performs framework
preprocessing/postprocessing; equivalent fixture tests and exported-model parity
remain required before the cross-platform parity checklist can be considered
release-certified.
