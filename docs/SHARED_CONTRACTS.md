# Shared desktop contracts

Canonical JSON Schemas live in [`contracts/`](../contracts). Rust owns parsing,
migration, and model-manifest verification; platform adapters should not invent
independent wire formats.

## Versioning rules

- `schemaVersion` is required in every current persisted document.
- Version 1 is current for regions, label sidecars, detector settings, and
  trusted-public-key rings. Signed model manifests are version 2.
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

`liveblock-config::model_manifest` defines manifest schema 2 and requires:

- immutable runtime classes, in order: `Logo`, `Ad banner`, `Sponsored`;
- CoreML or ONNX artifact format and input dimensions;
- `sha256-file-or-tree-v1`: ordinary SHA-256 for a file, or a domain-separated tree digest using length-prefixed `/`-normalized paths plus fixed-size per-file content digests;
- a monotonically increasing `releaseSequence` for platform rollback policy;
- immutable schema-5 `promotionGateSchema` plus the exact
  `promotionReportSha256` produced by the promotion-only signing workflow;
- a trusted `keyId` and Ed25519 signature over compact lexicographically keyed
  JSON with the `signature` field removed; and
- exact artifact fingerprint agreement before and after staging.

Private signing keys are release-infrastructure inputs and must never be stored
in the repository or desktop application. `tools/sign_model_manifest.py` reads
a 32-byte Ed25519 seed only from a named environment variable and refuses to
sign until the selected CoreML/ONNX artifact is present in a complete passing
schema-5 report, all report fingerprints still match, and the report's gate
code is current. The resulting signature binds the report hash, artifact hash,
release sequence, taxonomy, and runtime shape.

Applications receive only a strict schema-1 public-key ring. Unknown fields,
future schemas, duplicate key IDs, malformed keys, and empty release keyrings
fail closed. Key rotation adds a new public key; revocation ships an application
update that removes the old key. Development may use an empty ring, but release
packaging must reject it before signing.

The shared installer rejects symlinked content, concurrent update locks,
existing backups, untrusted keys, signature changes, taxonomy drift, future
schemas, and hash mismatches. For single-file artifacts (ONNX), it creates a
unique staging file with create-new semantics, fsyncs and re-hashes it, then
uses POSIX atomic rename or Windows `ReplaceFileW`/`MoveFileExW`; the previous
file is preserved as `.pre-update`. Directory artifacts such as CoreML
`.mlpackage` can be authenticated but are intentionally rejected by this
portable installer until the macOS updater supplies atomic directory-swap
semantics. Documentation and checklist status must not claim otherwise.

A signature made outside the promotion-only signer still cannot prove detector
quality. Release-key access and signing logs are security-sensitive controls;
the official signer binds the complete passing schema-5 report so distribution
authentication cannot silently replace promotion. Platform installers must
also persist the highest accepted `releaseSequence` per `modelId` and reject
equal/lower sequences unless a separately authorized application release
changes rollback policy.

## Platform adoption

Windows and Linux already depend on `liveblock-config`, `liveblock-regions`, and
`liveblock-labels`; their future ONNX update commands should call the shared
file installer rather than adding platform copy logic. macOS currently uses the
stricter schema-5 Python installer for developer promotion and still needs a
signed, atomic directory-aware production updater. Therefore “signed-manifest
verification on every platform” remains open despite the shared primitive.
