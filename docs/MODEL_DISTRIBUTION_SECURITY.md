# Model distribution security

## Status

The repository has a promotion-bound distribution contract and signing entry
point. Windows/Linux use the shared authenticated ONNX coordinator; macOS uses
strict CryptoKit verification plus atomic CoreML directory exchange. All three
platform implementations require embedded nonempty production keyrings, signed
manifests, monotonic state, production-runtime validation, and startup
reauthentication. The committed rings are intentionally empty, so no production
package is currently distributable.

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

### Threat boundary

Rollback state prevents a compromised update channel, stale signed payload, or
normal application workflow from installing an equal/older sequence. It does
not defend against an attacker already able to rewrite arbitrary same-user
application data, replay an old signed state/artifact pair, or modify the running
process; such access can also disable or replace the application. The packaged
signed release floor limits rollback below the current application bundle.
Hardware/OS-backed non-replayable counters are not implemented, so claims must
not imply protection from a fully compromised local user session.

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
sequence; deleting local state must not be a normal updater operation. The authenticated
manifest packaged with the application is also a release floor: an initial
application-data update must be newer, and a newer packaged detector supersedes
an older persisted update at startup.

Windows and Linux persist closed schema-1 `model-update-state.json` content that
contains the complete accepted signed manifest and highest sequence. Startup
reverifies that signature and active artifact before loading. An interrupted
swap whose backup matches the last accepted manifest is restored before load;
an interrupted first installation with no committed state is removed from the
new update-only path. Malformed/future state, symlinks, and mismatched state fail
closed and remain untouched. The currently accepted signing key must remain in the packaged
ring through rotation, because startup and the next update reauthenticate the
accepted manifest.

The staged model is loaded with the production ONNX Runtime adapter before
success is reported, then swapped into the detector mutex only after state
commit while update serialization remains held. A packaged bootstrap detector
is also loaded only when its colocated signed manifest verifies against the
embedded keyring; rejected accepted state never falls back to an older package.
A failed load restores the previous active artifact and leaves the old
signed state intact. An OS advisory transaction lock spans sequence checks,
recovery, install, runtime validation, state commit, and the in-memory detector
swap; its marker is safely reusable after process death. A platform application
mutex also serializes command preparation in one process. Staging paths and
non-regular lock/backup files fail closed. A regular previous backup is never
loaded without matching accepted state and is removed only while holding the
next update transaction lock.

macOS CoreML activation copies to a fixed same-volume staging directory, rejects
symlinks/special entries, re-hashes and production-loads it, synchronizes files
and directories, and uses `renameatx_np(RENAME_SWAP)` when replacing an active
model. Schema-1 state contains the complete accepted manifest. Startup restores
an accepted model from staging/backup after an interrupted swap, and an injected
state-commit failure test verifies immediate rollback. Debug/source model loading
remains explicitly separate; Release `VisionProcessor` accepts only this path.

## Key rotation and revocation

- Add a new public key through a reviewed application update before using it.
- Keep the old key only for an explicit overlap window.
- Remove a compromised/retired key through a signed application update.
- Never download or accept a user-selected trust root at model-update time.
- Linux resource integrity additionally depends on signed package/repository
  metadata and owner-only installation paths.

## Current automated evidence

- Rust rejects future manifest/keyring/update-state schemas, taxonomy drift,
  malformed hashes/keys/signatures, duplicate key IDs, empty release rings,
  rollback sequences, untracked active files, symlinks, staging collisions,
  concurrent locks, and artifact tampering.
- Python and Rust implement the same domain-separated,
  length-prefixed `sha256-file-or-tree-v1` algorithm.
- The signing tests prove failed/stale reports, mutated artifacts, absent ONNX
  parity artifacts, missing secrets, and symlinked trees cannot be signed.
- CI parses all three distribution schemas and runs these tests without a
  production private key. Windows/Linux Release compilation uses the explicit
  `LIVEBLOCK_ALLOW_EMPTY_MODEL_KEYRING=1` development override; both build
  scripts otherwise reject empty production rings.

These are contract, adapter, simulator/host, and tool tests, not evidence of
production key custody, package signing, notarization, power-loss behavior, or
rollback on every real target device. The committed development rings are
intentionally empty, so no production package can yet be represented as
distributable. A bundled compiled `.mlmodelc` also needs its own exact passing
schema-5 artifact/parity evidence before it may receive a packaged manifest; a
source `.mlpackage` report cannot be reused for that compiled artifact.
