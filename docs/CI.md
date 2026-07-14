# Desktop CI evidence boundaries

`.github/workflows/ci.yml` runs four independent jobs:

- shared Rust contracts/tests, managed-path confinement, schema parsing,
  Python adapter/model-distribution/documentation claim tests,
  release-profile training gate, and frontend build (using only a
  deterministic test key, never a release key);
- unsigned macOS 26 Debug tests plus a Release compile, with the `.xcresult` uploaded;
- Windows library tests plus debug/release all-target compilation on a Windows runner; and
- Linux library tests plus debug/release all-target compilation after installing
  Tauri, PipeWire, Wayland, and X11 development headers.

Cargo lockfiles pin `ort`, `ort-sys`, and `ndarray` exactly because mismatched
prerelease versions do not compile together.

A green compile job proves source/build compatibility on that hosted image. It
does **not** certify DirectML/CUDA/ROCm/CoreML performance, capture permission
flows, display hot-plug, DRM behavior, network-denied operation, click-through overlays, compositor
support, signing, notarization, installers, or real-hardware reliability. Those
remain separate release-matrix evidence gates.
