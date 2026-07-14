# Release artifact integrity, SBOM, and license evidence

LiveBlock release artifacts must be attributable to one full Git commit and must
be verified as bytes, not inferred from a successful build log. This contract is
separate from code signing, notarization, Authenticode, model authentication,
and real-device certification.

## Artifact inventory contract

`tools/release_evidence.py inventory` walks an already staged package directory
without following links. It rejects an empty root, symlinks, special files,
unsafe paths, and absent required files. Schema 1 records every directory and
regular file's canonical relative path and full permission mode, plus each file's
byte size and SHA-256, the exact 40-character commit, the platform/artifact type,
and a domain-separated aggregate fingerprint. Output is
create-new so an earlier inventory is never silently replaced.

`tools/release_evidence.py verify` strictly parses the closed inventory, checks
sorted/unique entries and required paths, rescans the directory, and rejects any
changed, added, or removed directory/file. Regular files are opened no-follow and
checked by descriptor identity while hashing. The staging tree must still be
private and quiescent; this is not a defense against a same-user process racing
ancestor directories. The JSON contract is published at
`contracts/release-artifact-inventory.schema.json`.

Every packaging pipeline must:

1. build into a new staging directory;
2. include only the inference runtime, authenticated promoted model resources,
   application resources, and required notices;
3. inventory the complete staged payload with required executable, model,
   manifest, keyring, notice, and uninstaller/update metadata paths;
4. verify that inventory immediately before signing;
5. sign the exact payload;
6. inventory the final installer/archive separately, then preserve both
   inventories, signatures, checksums, and signing responses beside the build;
7. verify the published download against its final inventory before release.

An inventory labeled `build-only` or `webview-payload-build-only` is not a
signature, distributable release, or release-readiness claim. Current CI exercises
the contract against the deterministic webview payload, Linux deb/rpm/AppImage
package bytes and application payloads, and final unsigned Windows MSI/NSIS
package bytes. Their trusted-key rings are intentionally empty because production
model keys/artifacts and signing credentials do not exist yet. Linux hosted CI
installs, bounded-smoke-launches, and removes the deb and launches the extracted
AppImage; RPM is extraction-only on Ubuntu. Windows evidence inventories both
final installer bytes and a non-installing MSI administrative extraction requiring
the executable, ONNX Runtime DLL, and keyring. It does not claim the Windows
payload was installed, launched, or executed. CI preserves the
exact webview payload as a normalized tar plus SHA-256 beside its inventory, so an operator can
extract it and rerun `verify` without rebuilding.

Example from the repository root:

```bash
commit="$(git rev-parse HEAD)"
python3 tools/release_evidence.py inventory \
  --root platform/_shared-frontend/dist \
  --output tools/runs/release-evidence/webview-inventory.json \
  --platform shared --artifact-type webview-payload-build-only \
  --commit "$commit" --require control-panel.html \
  --require labeling.html --require region-editor.html \
  --require render-layer.html --require training.html
python3 tools/release_evidence.py verify \
  --root platform/_shared-frontend/dist \
  --manifest tools/runs/release-evidence/webview-inventory.json
```

## Dependency SBOM and license policy

CI emits deterministic CycloneDX 1.5 build-input evidence from all three locked
Cargo graphs and the locked shared-frontend npm graph. It records dependency
contexts, declared licenses, npm integrity values where available, a normalized
Cargo component-set fingerprint, the exact npm lockfile hash, and the Git commit.
The Cargo fingerprint intentionally excludes checkout paths and is not a hash of
resolved edges/features; the SBOM is authoritative for the emitted component set.
Absolute checkout paths and timestamps are omitted.
Duplicate packages shared by platform graphs appear once with all contexts.

The companion `dependency-licenses.json` parses the supported SPDX expression
subset and fails CI when a third-party component is unknown/malformed, has no
declared license, has mandatory restricted GPL/AGPL/SSPL/BUSL terms, or has no
approved distribution choice. A permissive `OR` choice is accepted; `AND` keeps
every obligation. LGPL/MPL occurrences are preserved under
`reviewRequired` so release operators can satisfy notice, relinking, source, and
other distribution obligations; a green report is not legal advice and does
not remove that review. Final release approval requires the review list to be
resolved and required notices bundled.

`tools/requirements.txt` is intentionally excluded: it defines the source-only
training environment and production packages are inference-only. OS shared
libraries/drivers and Apple system frameworks are platform prerequisites rather
than bundled dependencies. No Swift Package Manager dependencies are currently
used. Linux production builds now fail unless packaging stages a regular non-symlink
`resources/onnxruntime/libonnxruntime.so` and `THIRD-PARTY-NOTICES.txt`.
The CPU staging helper pins official ONNX Runtime archives by complete SHA-256,
verifies target ELF identity, preserves license/notices, and records every output
hash. CI closes all three native package bytes and inventories the deb/rpm
extractions plus a mode-preserving AppImage application payload under explicit
build-only artifact types; see [`LINUX_PACKAGING.md`](LINUX_PACKAGING.md).
Optional CUDA/ROCm/OpenVINO/TensorRT builds may select only one provider and
also require the shared provider library plus its selected provider library.
Their exact binaries, transitive vendor libraries, licenses, and notices must be
present in the package inventory and final SBOM; source metadata and the
compile-only `LIVEBLOCK_ALLOW_UNPACKAGED_ORT=1` override are insufficient.

Reproduce CI evidence from the repository root after creating the three Cargo
metadata files:

```bash
python3 tools/release_evidence.py sbom \
  --cargo-metadata tools/runs/release-evidence/core-metadata.json \
  --cargo-metadata tools/runs/release-evidence/windows-metadata.json \
  --cargo-metadata tools/runs/release-evidence/linux-metadata.json \
  --npm-lock platform/_shared-frontend/package-lock.json \
  --output tools/runs/release-evidence/liveblock.cdx.json \
  --license-report tools/runs/release-evidence/dependency-licenses.json \
  --commit "$(git rev-parse HEAD)"
```

## CI evidence boundaries

The shared job also reruns focused legacy-schema migrations, atomic installer
behavior, signed CoreML bundle tamper rejection, and ONNX update rollback/crash
recovery tests. These are deterministic implementation tests. Linux hosted CI
extracts/inventories all three native formats, clean-installs and removes the deb,
and bounded-smoke-launches both installed deb and extracted AppImage payloads.
It does not run a native RPM transaction, Flatpak build/install, prior-version
upgrade, portal/compositor flow, or production model authentication. Windows CI
builds and inventories unsigned MSI/NSIS bytes under
`windows-installers-build-only` and the administratively extracted MSI payload
under a separate build-only inventory, but does not install or launch them. CI
does not test production package signing, OS upgrade, power loss, model parity,
or hardware execution. Those remain open until exact production artifacts and
suitable hosts/credentials exist.
