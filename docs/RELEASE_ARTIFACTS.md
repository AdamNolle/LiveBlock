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
AppImage; checksum-pinned Fedora separately exercises the RPM. Windows evidence
inventories both final installer bytes and a non-installing MSI administrative
extraction, then separately exercises prior/current MSI and NSIS transitions,
bounded launch, payload validation, downgrade rejection, data preservation, and
uninstall. None of these BuildOnly runs is signed execution. CI preserves the
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
declared license, contains any restricted GPL/AGPL/SSPL/BUSL term (including
under `OR`), or has no approved distribution choice. `AND` keeps every
obligation. LGPL/MPL occurrences remain under `reviewRequired` unless an exact
component-bound decision selects a declared permissive `OR` branch.
`licenses/dependency-license-decisions.json` currently elects MIT for the two
locked `r-efi` versions; decisions cannot override restricted/unknown terms and
fail on component, expression, or selected-branch drift.

The remaining MPL review list is exactly covered by
`licenses/mpl-source-offer.json`: five unmodified crate archives with Cargo.lock
checksums, versioned HTTPS source URLs, and pinned MPL-2.0 text. CI emits
`dependency-obligations.json` only when the decisions, generated report, three
locks, source offers, and license text agree. This prepares notice/source
evidence but is not legal advice or accountable human approval. Final release
approval still requires operator review and delivery of these files through the
chosen package/update channel.

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
  --license-decisions licenses/dependency-license-decisions.json \
  --output tools/runs/release-evidence/liveblock.cdx.json \
  --license-report tools/runs/release-evidence/dependency-licenses.json \
  --commit "$(git rev-parse HEAD)"
python3 tools/verify_dependency_obligations.py \
  --license-report tools/runs/release-evidence/dependency-licenses.json \
  --license-decisions licenses/dependency-license-decisions.json \
  --source-offer licenses/mpl-source-offer.json \
  --cargo-lock core/Cargo.lock \
  --cargo-lock platform/windows/Cargo.lock \
  --cargo-lock platform/linux/Cargo.lock \
  --output tools/runs/release-evidence/dependency-obligations.json
```

## Automatable blocker assessment

`tools/release_blockers.py` emits the closed schema-1 diagnostic defined by
`contracts/release-blocker-assessment.schema.json`. It derives four current
code/data blockers instead of copying prose status:

- the human-review plan status is recomputed from the local pool and must
  byte-semantically match the saved status, including its plan and review-state
  fingerprints;
- a schema-5 promotion report must use the current gate-code fingerprints and
  immutable limits. A potentially passing report is not trusted directly: the
  tool reruns the complete gate into create-new preserved evidence before it can
  mark the gate satisfied;
- each packaged macOS, Windows, and Linux model keyring is strictly parsed and
  must contain at least one canonical Ed25519 public key; and
- the existing DirectML readiness verifier must report all 8/8 hash-bound gates
  ready rather than the current deferred CPU-uploaded transport.

Ignored corpus/promotion inputs are intentionally absent in clean hosted CI, so
that environment records `human-review-input-missing` and
`promotion-report-missing` rather than borrowing local state. A local checkout
with the current ignored inputs records 0/21 review, a failed or stale schema-5
report, three empty production keyrings, and DirectML 0/8. Unknown fields,
duplicate JSON keys, malformed keyrings, stale promotion code, mismatched review
status, links/special files, or changed readiness inputs fail closed.

From the repository root, create a new diagnostic without replacing prior bytes:

```bash
python3 tools/release_blockers.py \
  --promotion-report tools/runs/promotion-gate-current.json \
  --output tools/runs/release-blocker-assessment/current.json
```

Add `--require-clean-git` when recording operator evidence and `--require-clear`
when a caller must receive nonzero status while any automatable gate is blocked.
The report is diagnostic only. It deliberately lists accountable dependency
approval, production signing/publication, and real-device platform validation as
external prerequisites it does **not** assess. Even an
`external-review-required` result cannot authorize model installation, signing,
publication, or release.

## CI evidence boundaries

The shared job also reruns focused legacy-schema migrations, atomic installer
behavior, signed CoreML bundle tamper rejection, and ONNX update rollback/crash
recovery tests. These are deterministic implementation tests. Linux hosted CI
extracts/inventories all three native formats, exercises prior/current deb
transitions and AppImage launch on Ubuntu, exercises prior/current RPM
transitions in checksum-pinned Fedora userspace, and builds/updates/launches a
local GNOME 50 Flatpak repository. These jobs do not certify a real portal,
compositor, GPU, native Fedora kernel/desktop, or production repository metadata.
Windows CI builds and inventories unsigned MSI/NSIS bytes under
`windows-installers-build-only` and the administratively extracted MSI payload
under a separate build-only inventory. Hosted Windows exercises prior install,
current upgrade, same-version repair/reinstall, downgrade rejection, inventory,
bounded launch, user-data preservation, and uninstall for both formats. CI does not test production package signing, genuinely shipped historical
payloads, staged-update publication/execution, power loss, model parity, capture,
or hardware execution. The selected production application-update contract is
the offline-verified `stable-staged-nsis` bundle with a detached CMS-signed
closed descriptor described in `WINDOWS_RELEASE.md`; MSIX/App Installer remains
unimplemented rather than an
alternate channel. Production channel evidence remains open until exact promoted
artifacts, accountable dependency review, suitable credentials, publication, and
real Windows hosts exist.
