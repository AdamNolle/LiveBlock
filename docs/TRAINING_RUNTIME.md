# Training runtime distribution policy

## Decision

Production desktop packages are **local-inference-only**. They bundle only the
platform detector runtime and a verified promoted model. They do not bundle or
download Python, pip, Ultralytics, PyTorch, coremltools, compilers, training
code, or replacement weights.

Training remains an explicit source/developer companion workflow for v1.0:

1. capture and label screenshots locally;
2. export labels from a trusted source checkout;
3. explicitly run `tools/setup_env.sh` and the training command;
4. retain the output as an untrusted candidate;
5. run the complete human-reviewed schema-5 promotion gate; and
6. install only the fingerprinted passing artifact, with a trusted signed model
   manifest for distributed updates.

This avoids silently executing a package manager from a signed GUI, keeps the
release attack surface and download size bounded, and applies one promotion
policy to personal and project candidates.

## Build behavior

`LIVEBLOCK_TRAINING_RUNTIME_ENABLED` is `YES` for macOS Debug/source builds and
`NO` for Release builds. The Release dashboard explains that the package is
inference-only and does not offer environment installation or training.
Windows and Linux release packages must follow the same rule; their source
workflows may call the same tools, but the distributed Tauri applications must
not invoke pip or fetch training code.

Label capture/export is user data functionality and may remain available in a
release. Model installation is a separate authenticated update operation, not
part of training.

## Future companion requirements

A distributable training companion can replace this staged policy only after it
has:

- a separately signed/notarized package and isolated writable directories;
- a locked dependency graph/SBOM with reproducible hashes;
- no administrator/root requirement;
- explicit network disclosure and opt-in downloads;
- resource, disk, cancellation, and cleanup limits;
- candidate-only output with license/provenance metadata;
- no access to production signing keys;
- schema-5 promotion and signed-manifest handoff; and
- macOS, Windows, and Linux packaging/security review.

Until then, release documentation must describe training as a source workflow,
not a one-click production feature.
