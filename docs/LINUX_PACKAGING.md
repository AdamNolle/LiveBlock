# Linux packaging

LiveBlock's Linux release packages are inference-only. They must contain an
authenticated promoted ONNX model, a nonempty trusted-key ring, the exact
ONNX Runtime selected at build time, and complete dependency notices. The
repository intentionally contains none of the protected model/trust inputs, so
current CI packages are **build-only evidence**, not distributable releases.

## Pinned CPU runtime staging

`tools/stage_linux_onnxruntime.py` is the only supported helper for staging the
CPU runtime into Tauri resources. It pins ONNX Runtime 1.18.1 official GitHub
release URLs and archive SHA-256 values for `x86_64` and `aarch64`. It:

- downloads only over HTTPS when `--archive` is omitted;
- rejects an archive whose complete SHA-256 differs from the committed pin;
- reads only exact expected archive members and rejects links or special files;
- copies the versioned regular ELF into a non-symlink `libonnxruntime.so`;
- verifies 64-bit little-endian shared-object type and target architecture;
- preserves the upstream license and complete `ThirdPartyNotices.txt`;
- emits a deterministic `STAGING-MANIFEST.json` with source and file hashes;
- writes a sibling staging directory before installation; and
- refuses to replace an existing destination unless `--replace` is explicit.

The application never invokes this helper and never downloads runtimes, models,
code, or trust roots.

From the repository root:

```bash
python3 tools/stage_linux_onnxruntime.py \
  --architecture x86_64 \
  --destination platform/linux/src-tauri/resources/onnxruntime
```

The destination is gitignored to prevent committing a large generated binary.
For an offline/operator-provided archive, add `--archive /absolute/path/file.tgz`;
the same committed SHA-256 is still required.

## Native bundles

Install the dependencies listed in `platform/linux/README.md`, then:

```bash
cd platform/_shared-frontend
npm ci
cd ../linux/src-tauri
LIVEBLOCK_ALLOW_EMPTY_MODEL_KEYRING=1 \
  ../../_shared-frontend/node_modules/.bin/tauri build --bundles deb
```

The override above is allowed only for CI/source build evidence. A production
package must replace `resources/trusted-model-keys.json` with a protected,
nonempty schema-1 ring and include an authenticated packaged detector manifest
and artifact. Never set `LIVEBLOCK_ALLOW_UNPACKAGED_ORT=1` for a package.

CI extracts the `.deb`, verifies the canonical `/usr/bin/liveblock-linux` and
`/usr/lib/LiveBlock/resources/onnxruntime/` paths, creates and immediately
re-verifies a schema-1 artifact inventory, and uploads the exact package,
extracted inventory, runtime staging manifest, and SHA-256. That proves package
construction and byte/mode identity only. It does not prove installation,
startup, model authentication, signatures, updates, or hardware behavior.

RPM and AppImage remain declared targets but are not called complete until CI
builds, inventories, installs/runs, upgrades, and uninstalls each format.

## Flatpak boundary

`platform/linux/flatpak/com.adamnolle.LiveBlock.json` now uses the real workspace
paths, installs resources under `/app/lib/LiveBlock/resources`, pins both architecture
runtime archives, and limits runtime permissions to Wayland/fallback X11, DRI,
IPC, and the desktop portal. It intentionally has no network, all-device, host
filesystem, FileChooser, Notifications, or RealtimeKit permission.

A clean Flathub build is still blocked: Cargo and npm sources must be converted
to pinned Flatpak source entries (or reviewed vendored inputs) before the
manifest can remain offline. The empty keyring in the manifest also makes it
build-only. Do not publish it or check the Flatpak checklist item until those
inputs, the promoted model, production trust roots, install/runtime evidence,
and portal-only behavior are present.
