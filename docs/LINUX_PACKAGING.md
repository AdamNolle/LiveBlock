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

CI also builds separately inventoried `0.0.9` deb/RPM package-mechanics fixtures
from the same source payload. Ubuntu clean-installs the prior deb, upgrades to
current `0.1.0`, reinstalls current as a repair, and requires the default prior
package install to leave current installed. The Fedora 44 job performs the same
prior-install, upgrade, reinstall, and current-version-retention sequence through
DNF before its bounded launch.

This proves build-only package construction, payload placement, Debian/Fedora
package-manager transitions, and bounded X11 startup in hosted environments. The
prior fixture changes package metadata but is not a historical release payload.
It does not prove promoted-model authentication, production trust roots, signatures,
updates, capture, compositor/portal behavior, GPUs, accessibility, or sustained
operation. A separate CI job clean-installs the RPM through DNF inside the
checksum-pinned Fedora 44 container, verifies declared runtime requirements and
`ldd` closure, repeats the 12-second Xvfb startup assertions, removes the package,
and rejects any remaining application file or special node. Tauri's generated
RPM may leave only empty intermediate resource directories; the lifecycle
records those paths and removes only empty directories before confirming a clean
container. This is real Fedora userspace/package-manager evidence but not a
native Fedora kernel, desktop, portal, compositor, or hardware certification.
Upgrade compatibility against a genuinely shipped historical payload remains
open until such a release exists. None of these empty-keyring, no-model artifacts
is distributable.

## Flatpak boundary

`platform/linux/flatpak/com.adamnolle.LiveBlock.json` now uses the real workspace
paths, installs resources under `/app/lib/LiveBlock/resources`, pins both architecture
runtime archives, and limits runtime permissions to Wayland/fallback X11, DRI,
IPC, the desktop portal, and `org.kde.StatusNotifierWatcher` for Tauri tray
registration. It intentionally has no network, all-device, host filesystem,
FileChooser, Notifications, or RealtimeKit permission.

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
It also builds pinned GTK 3 native dependencies that are absent from the GNOME
SDK: `gtk-layer-shell` v0.8.2 at commit
`91e5ef02b557f93337bcc11ffe8c0a251aa9ab52`, plus the vendored
`libayatana-appindicator` module chain described in
`platform/linux/flatpak/shared-modules/README.md`. The LLVM 20 SDK extension matching Freedesktop 25.08 is used only to provide
`libclang` for PipeWire's generated bindings.

Hosted CI run
[29380181732](https://github.com/AdamNolle/LiveBlock/actions/runs/29380181732)
built the complete Flatpak in the sandbox, exported a local unsigned repository,
created and verified a build-only bundle inventory, clean-installed the app,
verified the regular ONNX Runtime/license/notices/keyring payloads, kept the app
alive for 12 seconds under Xvfb, observed packaged ONNX Runtime 1.18.1 loading,
empty-keyring rejection, and X11 shortcut registration, then uninstalled it.
Artifact `8329534260` preserves the bundle, build/launch logs, inventory, checksum,
and lifecycle summary. The x86_64 bundle SHA-256 is
`fc09952f2414a46b4003ed7b65e62921590a75e5fbc9be6c8b48aa63345752e2` and its
inventory aggregate is
`b06811bb4901d128cd7de6a0871a1191a684f441621d6aed136fddd547a0bddb`.

That cited iteration-25 artifact remains build-only: its source is a local
`type: dir`, its embedded keyring is empty, it has no promoted model or signed
repository metadata, and it used the now end-of-life GNOME 47 runtime. The
current manifest pins GNOME 50; separate hosted build/lifecycle evidence must be
recorded before that migration is considered verified. Xvfb startup does not
certify real portals, tray hosts, Wayland compositors, GPUs, multi-output
geometry, accessibility, or pointer behavior. Do not publish it or check the
broad Flatpak checklist item until a pinned release source, production
trust/model inputs, signed repository, and real compositor behavior are present.
