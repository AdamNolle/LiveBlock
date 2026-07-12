# Shared desktop contracts

Canonical JSON Schemas live in [`contracts/`](../contracts). Rust owns parsing,
migration, and model-manifest verification; platform adapters should not invent
independent wire formats.

## Versioning rules

- `schemaVersion` is required in every current persisted document.
- Version 1 is the current version for regions, label sidecars, detector
  settings, and signed model manifests.
- Legacy region arrays and unversioned label/settings objects are version 0.
  They are accepted only when structurally valid and atomically rewritten to
  version 1.
- Unknown future versions fail closed and remain byte-for-byte untouched. A
  downgraded app must never quarantine or overwrite valid future data.
- Malformed data is never silently treated as an empty current document.
- Migrations are one-way and must have legacy, current round-trip, future
  rejection, and no-rewrite tests.

Normalized region/box constructors additionally enforce `x + width <= 1` and
`y + height <= 1`; JSON Schema cannot express those cross-property constraints.

## Signed model manifests

`liveblock-config::model_manifest` defines schema 1 and requires:

- immutable runtime classes, in order: `Logo`, `Ad banner`, `Sponsored`;
- CoreML or ONNX artifact format and input dimensions;
- `sha256-file-or-tree-v1`: ordinary SHA-256 for a file, or a domain-separated tree digest using length-prefixed `/`-normalized paths plus fixed-size per-file content digests;
- a trusted `keyId` and Ed25519 signature over compact lexicographically keyed
  JSON with the `signature` field removed; and
- exact artifact fingerprint agreement before and after staging.

Private signing keys are release-infrastructure inputs and must never be stored
in the repository or desktop application. Applications receive only an
allowlisted public-key ring. Key rotation adds a new public key; revocation
ships an application update that removes the old key.

The shared installer rejects symlinked content, concurrent update locks,
existing backups, untrusted keys, signature changes, taxonomy drift, future
schemas, and hash mismatches. For single-file artifacts (ONNX), it creates a
unique staging file with create-new semantics, fsyncs and re-hashes it, then
uses POSIX atomic rename or Windows `ReplaceFileW`/`MoveFileExW`; the previous
file is preserved as `.pre-update`. Directory artifacts such as CoreML
`.mlpackage` can be authenticated but are intentionally rejected by this
portable installer until the macOS updater supplies atomic directory-swap
semantics. Documentation and checklist status must not claim otherwise.

A signed manifest authenticates distribution but does **not** prove detector
quality. Release models must independently pass the complete schema-5 promotion
report and fingerprint-bound installer policy.

## Platform adoption

Windows and Linux already depend on `liveblock-config`, `liveblock-regions`, and
`liveblock-labels`; their future ONNX update commands should call the shared
file installer rather than adding platform copy logic. macOS currently uses the
stricter schema-5 Python installer for developer promotion and still needs a
signed, atomic directory-aware production updater. Therefore “signed-manifest
verification on every platform” remains open despite the shared primitive.
