# Model distribution security

## Status

The repository now has a promotion-bound distribution contract and signing
entry point. Platform adoption is **not complete**: Windows/Linux still load a
packaged ONNX file directly, and macOS production updates still need an atomic
directory swap plus embedded release keyring. No checklist or release claim may
state that every platform authenticates updates yet.

## Trust chain

1. `tools/verify_promotion.py` creates immutable schema-5 evidence. It enforces
   fixed quality, placement, preservation, contextual-negative, compatibility,
   baseline, and CoreML latency gates.
2. `tools/sign_model_manifest.py` revalidates the complete report and every
   referenced fingerprint. It selects only `candidate_coreml`, or
   `candidate_onnx` once ONNX parity evidence and gate arguments are added to
  schema-5 output.
3. The signer emits manifest schema 2. Its Ed25519 signature covers the artifact
   hash, exact promotion-report hash, schema-5 discriminator, runtime taxonomy,
   input shape, model version, and monotonic release sequence.
4. A desktop updater must load its public keys only from a strict application
   resource, verify the manifest and staged artifact, reject rollback, validate
   model loadability, atomically activate, and preserve/restore the previous
   artifact if activation fails.

A valid signature from an arbitrary key is worthless. The allowlisted keyring
is part of the signed/notarized application or signed Linux package. A release
package must contain a nonempty approved ring before platform signing.

## Signing command

Run from the repository root only after schema-5 reports `passed: true`:

```bash
export LIVEBLOCK_MODEL_SIGNING_KEY_B64='<base64 32-byte Ed25519 private seed>'
PYTHONPATH=tools python3 tools/sign_model_manifest.py \
  --promotion-report tools/runs/promotion-gate-current.json \
  --attested-report-output tools/runs/promotion-gate-release-attested.json \
  --artifact-format coreml \
  --model-version 1.0.0 \
  --release-sequence 1 \
  --key-id release-2026 \
  --output tools/runs/liveblock-detector.manifest.json
unset LIVEBLOCK_MODEL_SIGNING_KEY_B64
```

The supplied report is only a recipe: the signer reconstructs and runs the
current `verify_promotion.py` command in a fresh subprocess, creates the
attested report at a new path, and signs only that exact output if it passes.
This prevents a caller from fabricating `passed: true` around arbitrary metric
objects. The full corpus, fixtures, baseline, candidate, CoreML runtime, and
promotion dependencies must therefore be present in protected signing
infrastructure. The signing-secret environment variable is explicitly removed
from the gate subprocess and is read only after that fresh gate passes.

The tool derives input dimensions, runtime classes, and the NMS contract from
the freshly validated promotion evidence; callers cannot override them. It does not
accept private key material as a CLI value and creates the output without
overwriting. Signing must occur in protected release
infrastructure with secret masking, restricted environments, immutable logs,
and human authorization. The private seed, generated manifest working copy,
and promotion corpus remain uncommitted.

ONNX signing fails until the passing report contains both
`config.candidate_onnx` and an exact `artifacts.candidate_onnx` fingerprint.
This intentionally prevents an unmeasured conversion from inheriting CoreML's
promotion result.

## Rollback and activation requirements

`releaseSequence` starts at 1 and increases for every signed model release.
Each platform must atomically persist the highest accepted value per `modelId`.
It must reject equal or lower values before staging. Downgrade recovery requires
a newly signed application release or a newly promoted model with a higher
sequence; deleting local state must not be a normal updater operation.

The staged model must be loaded with the production runtime before success is
reported. Inference/reload must be serialized so no thread observes partial
state. A failed load restores the previous active artifact and leaves evidence
for diagnosis. Existing lock, staging, or backup paths fail closed rather than
being silently deleted.

CoreML directory activation requires macOS `renameatx_np(RENAME_SWAP)` (or an
equivalent proven atomic exchange), recursive no-symlink/no-special-file copy,
file and directory synchronization, post-copy hashing, a cooperating update
lock, and crash recovery tests. Two sequential `rename` calls are not an
atomic replacement and are not sufficient.

## Key rotation and revocation

- Add a new public key through a reviewed application update before using it.
- Keep the old key only for an explicit overlap window.
- Remove a compromised/retired key through a signed application update.
- Never download or accept a user-selected trust root at model-update time.
- Linux resource integrity additionally depends on signed package/repository
  metadata and owner-only installation paths.

## Current automated evidence

- Rust rejects future manifest/keyring schemas, taxonomy drift, malformed
  hashes/keys/signatures, duplicate key IDs, empty release rings, symlinks,
  staging collisions, concurrent locks, and artifact tampering.
- Python and Rust implement the same domain-separated,
  length-prefixed `sha256-file-or-tree-v1` algorithm.
- The signing tests prove failed/stale reports, mutated artifacts, absent ONNX
  parity artifacts, missing secrets, and symlinked trees cannot be signed.
- CI parses both schemas and runs these tests without a production private key.

These are contract and tool tests, not evidence of completed platform updater
adoption, production key custody, package signing, notarization, or rollback on
real devices.
