# Desktop CI evidence boundaries

`.github/workflows/ci.yml` runs four independent jobs:

- shared Rust contracts/tests, schema parsing, Python adapter-contract tests,
  release-profile training gate, and frontend build;
- unsigned macOS 26 Debug tests plus a Release compile, with the `.xcresult` uploaded;
- Windows debug/release all-target compilation on a Windows runner; and
- Linux debug/release all-target compilation after installing Tauri,
  PipeWire, Wayland, and X11 development headers.

Cargo lockfiles pin `ort`, `ort-sys`, and `ndarray` exactly because mismatched
prerelease versions do not compile together.

A green compile job proves source/build compatibility on that hosted image. It
does **not** certify DirectML/CUDA/ROCm/CoreML performance, capture permission
flows, display hot-plug, DRM behavior, click-through overlays, compositor
support, signing, notarization, installers, or real-hardware reliability. Those
remain separate release-matrix evidence gates.
