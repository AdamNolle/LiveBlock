# Platform shells

This directory will eventually hold the platform-specific application shells
that link the shared `core/` Rust crate. **Today none of this is built yet** —
the macOS app still lives in the top-level `Sources/`. See
[`../ARCHITECTURE.md`](../ARCHITECTURE.md) for the migration plan.

```
platform/
├── macos/        # Swift + AppKit/SwiftUI; uses swift-bridge to talk to core/
├── windows/      # C++/Rust + WinUI 3 (or Tauri); WGC + DirectML + D3D11
└── linux/        # Rust + GTK4 (or Tauri); PipeWire (Wayland) + XComposite (X11)
```

## When this gets populated

- **`platform/macos/`** — first, when we move existing `Sources/` after
  Phase 1 of the migration plan (extract pure logic into core). Until then,
  the canonical macOS app is the top-level `Sources/`.
- **`platform/windows/`** — after `core/` reaches MVP and macOS keeps
  working. Anchor goal: parity with macOS for capture + detection +
  inpainting + control panel UI.
- **`platform/linux/`** — last. Two capture paths (Wayland portal vs X11)
  and the GNOME-no-layer-shell problem make it the most-uncertain target.

## Shared concerns to keep in mind

- The label JSON format (in `~/Library/Application Support/LiveBlock/training/`
  on macOS, `%APPDATA%\LiveBlock\training\` on Windows,
  `~/.local/share/LiveBlock/training/` on Linux) is the same across platforms
  so labels are portable.
- Trained `.pt` and `.onnx` weights live alongside platform models. macOS
  uses `.mlpackage`, Windows uses `.onnx` (DirectML), Linux uses `.onnx`
  (CUDA/ROCm/CPU). The ultralytics export script writes both formats from
  the same training run.
