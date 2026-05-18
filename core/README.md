# LiveBlock Core (Rust)

Cross-platform engine for LiveBlock. All non-GUI logic lives here so it can be
shared between the macOS app, a future Windows app, and a future Linux app.
This crate has zero GUI or platform dependencies.

## Workspace layout

```
core/
├── Cargo.toml                  # workspace manifest
└── crates/
    ├── liveblock-regions/      # NormalizedRegion + thread-safe RegionStore
    │                           # (port of Sources/RegionStore.swift)
    ├── liveblock-labels/       # LabelBox + LabelDocument JSON sidecar
    │                           # (port of Sources/LabelingController.swift types)
    ├── liveblock-detection/    # YOLO post-processing: NMS, score filter,
    │                           # normalized<->pixel coords, top-left/bottom-left flip
    └── liveblock-core/         # Capture / Detector / Inpainter traits, MaskCache,
                                # Coordinator with detect-every-N-frames scheduler
```

Each crate is independent and unit-tested. `liveblock-core` re-exports the key
types from its siblings so downstream code only needs `use liveblock_core::*;`.

## Build

```bash
cd core
cargo check
cargo build
cargo test
```

The workspace pins Rust 2021 edition and uses `resolver = "2"`.

## JSON byte-compatibility

The Swift app encodes regions and labels with
`JSONEncoder.outputFormatting = [.prettyPrinted, .sortedKeys]` and
`dateEncodingStrategy = .iso8601`. The Rust ports match this format byte for
byte:

- 2-space indentation (`PrettyFormatter::with_indent(b"  ")`)
- keys sorted alphabetically inside every object
- UUIDs rendered upper-case (`8-4-4-4-12` Foundation default)
- `labeledAt` written as `yyyy-MM-ddTHH:mm:ssZ`

So existing on-disk data on macOS continues to load, and writes from the Rust
core are diff-clean against writes from the Swift app.

## Next steps

1. **macOS bindings**: add a `core/bindings/macos` adapter using
   [`swift-bridge`](https://github.com/chinedufn/swift-bridge) so the existing
   Swift app links the Rust crates as a static library and the Swift types in
   `Sources/RegionStore.swift` / `Sources/LabelingController.swift` become
   thin wrappers that call into Rust.
2. **Windows / Linux bindings**: add a `core/bindings/c` adapter using
   [`cbindgen`](https://github.com/mozilla/cbindgen) to expose a stable C ABI.
   Windows and Linux hosts (in `platform/`) link the same `cdylib` and provide
   their own implementations of `Capture` / `Detector` / `Inpainter`.
3. **Inpainting**: add `liveblock-inpainting` with a CPU reference fill
   (mirror-blend, edge interpolation). The macOS Metal / CoreImage path stays
   platform-side and implements the `Inpainter` trait.
