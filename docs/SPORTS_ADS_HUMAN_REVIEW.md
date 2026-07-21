# Sports-ad human review

Detector promotion requires attributable human decisions for 21 independent,
licensed source groups. AI-assisted proposals are starting points only and never
count as human review.

## Start the guided reviewer

From the repository root, after `./tools/setup_env.sh` has created the local
Python environment:

```bash
./tools/review_sports_ads.sh
```

The command starts a loopback-only server and opens
<http://127.0.0.1:8765>. Enter your name or email once. The identity is fixed for
the server session and recorded with a timezone-aware timestamp and the
`personally-inspected-full-image-v1` attestation.

For every planned image:

1. Inspect the full image after it has loaded.
2. Correct sponsor boxes and their immutable runtime class and placement.
3. Correct preservation regions for team names, numbers, crests, and
   manufacturer badges.
4. Verify contextual hard-negative checkboxes.
5. Check the personal-review attestation only after the final annotation state
   is correct, then approve or exclude the source with a reason.

The UI shows source/license context, plan coverage, progress, selectable
annotations, keyboard shortcuts, load-state gating, and unsaved-change warnings.
Stop the server with **Ctrl-C**. Advanced use can preconfigure attribution or
suppress browser opening:

```bash
./tools/review_sports_ads.sh \
  --reviewer 'reviewer@example.com' \
  --no-open-browser
```

## Integrity boundary

The server accepts only its configured loopback Host and Origin, requires an
ephemeral request token and explicit attestation, serializes decisions, and
binds approval to the exact displayed image, proposal sidecar, and source
context revision. Sidecars are written from unique synchronized same-directory
temporary files. Stale pages cannot overwrite newer or existing attributable
human decisions.

A fully compromised same-user process remains outside this local workflow's
threat boundary. Attribution still depends on the named person actually
inspecting the image; automation must not submit human decisions.

## Check progress

```bash
./tools/review_sports_ads.sh --plan-status
```

Proceed only when all planned representatives have attributable decisions and
there are no required-facet candidate deficits. After exclusions or material
corrections, regenerate the plan/status with `tools/corpus/review_labels.py` as
documented by `--help`.

## After review

Build a human-only leakage-safe corpus, train a new candidate rather than
reusing `real-v5`, export equivalent CoreML and ONNX artifacts, and run the full
schema-5 promotion/parity/latency gate. Installation remains forbidden unless
that fresh gate passes completely through `tools/install_verified_model.py`.
