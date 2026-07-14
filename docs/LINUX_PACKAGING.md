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
  ../../_shared-frontend/node_modules/.bin/tauri build --bundles deb,rpm,appimage --ci
```

The override above is allowed only for CI/source build evidence. A production
package must replace `resources/trusted-model-keys.json` with a protected,
nonempty schema-1 ring and include an authenticated packaged detector manifest
and artifact. Never set `LIVEBLOCK_ALLOW_UNPACKAGED_ORT=1` for a package.

Native Ubuntu CI builds `.deb`, `.rpm`, and `.AppImage` from one Release binary
and the same checksum-pinned runtime staging directory. It closes the three
package bytes in one schema-1 inventory, writes basename-rerunnable SHA-256
checksums, and inspects each extracted payload for the canonical
`/usr/bin/liveblock-linux` and
`/usr/lib/LiveBlock/resources/onnxruntime/` paths. The complete deb and rpm
extractions are immediately inventoried and reverified. Because AppImage
internals contain bundler-created links, CI inventories a mode-preserving copy
of the application binary and complete `LiveBlock/resources` subtree while the
closed outer package inventory binds all remaining AppImage bytes.

On the clean hosted Ubuntu runner, CI then installs the local build-only deb,
requires the installed UI process to remain alive for a bounded 12-second Xvfb
smoke window, verifies that packaged ONNX Runtime 1.18.1 loaded, verifies the
empty development keyring was rejected, and verifies X11 shortcut registration.
It then uninstalls the deb and checks that the system executable and resource
tree were removed. CI also extracts the AppImage without FUSE and requires its
`AppRun` process to satisfy the same bounded startup assertions. Exact package
bytes, payload inventories, runtime staging manifest, install/launch/uninstall
logs, and a machine-readable lifecycle summary are preserved together.

This proves build-only package construction, payload placement, a Debian package
transaction, and bounded X11 startup on the hosted Ubuntu image. It does not
prove promoted-model authentication, production trust roots, signatures,
updates, capture, compositor/portal behavior, GPUs, accessibility, or sustained
operation. RPM install/launch/uninstall remains untested because Ubuntu is not a
Fedora/RHEL package-manager environment. Native upgrade testing also remains
open until a prior installable version is available. None of these empty-keyring,
no-model artifacts is distributable.

## Flatpak boundary

`platform/linux/flatpak/com.adamnolle.LiveBlock.json` now uses the real workspace
paths, installs resources under `/app/lib/LiveBlock/resources`, pins both architecture
runtime archives, and limits runtime permissions to Wayland/fallback X11, DRI,
IPC, and the desktop portal. It intentionally has no network, all-device, host
filesystem, FileChooser, Notifications, or RealtimeKit permission.

Cargo and npm build dependencies are committed as
`cargo-sources.json` and `node-sources.json`, generated twice identically from
the Linux Cargo lock and frontend npm lock with upstream
`flatpak-builder-tools` commit
`737c0085912f9f7dabf9341d4608e2a77a51a73a`. The generator commands, input
hashes, and output hashes are closed in `generated-sources.lock.json`.
`tools/verify_flatpak_sources.py` independently requires exact crates.io
URL/SHA-256 coverage, exact npm registry URL/integrity coverage, HTTPS-only
downloads, no generated network commands, the Cargo vendor replacement config,
and the offline Cargo/npm cache environment. Refreshes must use the recorded
commands from a checkout of that exact generator commit, run twice byte-for-byte
identically, then pass:

```bash
PYTHONPATH=tools python3 -m unittest tools.test_flatpak_sources
python3 tools/verify_flatpak_sources.py
```

The committed manifest can therefore fetch every hash-pinned dependency before
the sandbox build and run `npm ci --offline` plus Cargo offline inside the build.
It remains build-only: its source is a local `type: dir`, its embedded keyring is
empty, and it has no promoted model or signed repository metadata. Do not publish
it or check the broad Flatpak checklist item until a pinned release source,
production trust/model inputs, install/runtime evidence, and portal-only
compositor behavior are present.
