# Desktop CI evidence boundaries

`.github/workflows/ci.yml` runs four independent jobs:

- shared Rust contracts/tests, managed-path confinement, schema parsing,
  Python adapter/model-distribution/documentation claim tests, focused
  migration/installer/update-recovery gates, the release-profile training gate,
  and frontend build (using only a deterministic test key, never a release key);
- unsigned macOS 26 Debug tests plus a Release compile, with the `.xcresult` uploaded;
- Windows library tests plus debug/release all-target compilation on a Windows runner,
  followed by unsigned build-only MSI/NSIS construction, checksum, and final-package
  byte inventory; and
- Linux library tests plus debug/release all-target compilation after installing
  Tauri, PipeWire, Wayland, and X11 development headers; Release separately
  proves that an unpackaged ONNX Runtime is rejected before using explicit
  compile-only empty-keyring/runtime overrides; then CI checksum-stages the
  pinned CPU runtime, builds build-only `.deb`, `.rpm`, and `.AppImage` bundles,
  closes package bytes and extracted application payloads in canonical
  byte/mode inventories, and runs bounded hosted-Ubuntu lifecycle smoke checks.

Cargo lockfiles pin `ort`, `ort-sys`, and `ndarray` exactly because mismatched
prerelease versions do not compile together.

The shared job uploads `release-dependency-integrity-evidence`: a deterministic
CycloneDX 1.5 build-input SBOM, declared-license policy/report, a verified
schema-1 inventory of the built webview payload, and a deterministic tar plus
SHA-256 preserving the exact inventoried bytes/modes for independent recheck. See
[`RELEASE_ARTIFACTS.md`](RELEASE_ARTIFACTS.md). This webview payload is explicitly `build-only`; it is not an installer or
signed production package. The Linux job separately uploads
`linux-build-only-package-evidence` containing the exact deb/rpm/AppImage,
rerunnable package hashes, package and payload inventories, runtime staging
manifest, lifecycle summary, and install/launch/uninstall logs. Hosted Ubuntu
installs, bounded-smoke-launches, and removes the deb, then bounded-smoke-launches
the extracted AppImage. RPM lifecycle is not run on Ubuntu. Every embedded
keyring is intentionally empty, so this is build-only evidence and must not be
distributed. The Windows job uploads `windows-build-only-package-evidence` with
one unsigned MSI, one unsigned NSIS executable, basename-rerunnable SHA-256 lines,
a verified package-byte inventory, an MSI administrative-extraction payload and
inventory, and a manifest whose signed/timestamped/promoted flags are false. The
extracted payload must include the executable, ONNX Runtime DLL, and keyring.
Hosted Windows then clean-installs, inventories, bounded-smoke-launches, and
uninstalls both BuildOnly formats, preserving lifecycle logs and a limitations
summary. See
[`LINUX_PACKAGING.md`](LINUX_PACKAGING.md) and
[`WINDOWS_RELEASE.md`](WINDOWS_RELEASE.md).

A green compile job proves source/build compatibility on that hosted image. It
does **not** certify DirectML/CUDA/ROCm/CoreML performance, capture permission
flows, display hot-plug, DRM behavior, network-denied operation, click-through overlays, compositor
support, signing, notarization, production installer trust, upgrade/repair
behavior, RPM transactions, or real-hardware reliability. Those remain separate release-matrix
evidence gates.
