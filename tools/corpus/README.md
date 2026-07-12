# Sports advertising corpus

This directory contains reproducible tooling—not copyrighted training images—for
building LiveBlock's sports-sponsorship corpus.

## Coverage taxonomy

Every positive box uses one of the runtime-compatible classes:

- `Logo`: commercial sponsor or advertiser marks on jerseys, helmets, race-car
  liveries, equipment, and broadcast graphics. Never use this for a team name,
  jersey number, team crest, manufacturer badge, or other team identity.
- `Ad banner`: trackside boards, rink boards, stadium ribbons, billboards, and display creatives.
- `Sponsored`: explicit sponsored/promoted badges or paid-placement overlays.

Each annotation should also record a non-model `placement` such as `jersey`,
`car_livery`, `helmet`, `venue_board`, `broadcast_overlay`, `equipment`, or
`ordinary_screen_ad`. Placement is used for slice metrics, not model labels.

Team identity is explicitly protected with `preserve_regions`. These regions
remain unlabelled background examples for YOLO training and are exported to
held-out fixtures as a separate false-positive gate. Supported kinds are
`team_name`, `jersey_number`, `vehicle_number`, `team_crest`, and
`manufacturer_badge`.

## Approved source policy

Default automated ingestion accepts only Public Domain, CC0, CC BY 2.0/3.0/4.0,
and publisher-controlled MIT datasets. Each image must retain creator, source
page, exact license/version, retrieval date, and SHA-256. CC BY-SA is segregated
unless explicitly enabled. “Research only,” unclear, uploader-asserted broadcast
frames, and all-rights-reserved images are rejected.

Current leads:

| Source | Use | Status |
|---|---|---|
| Wikimedia Commons/Openverse | Motorsport, jerseys, venues, hard negatives | Approved per-file after metadata verification |
| Open Images V7 | Broad contextual images/hard negatives | Approved with per-image attribution verification |
| Vehicle Logos Dataset | Vehicle-logo shape pretraining | Approved, CC BY 4.0 |
| ExposureEngine | Soccer jersey/venue sponsor OBBs | Quarantined pending written confirmation of broadcast-frame rights |
| Advertising Panel Segmentation | Venue-board segmentation | Rejected until an explicit license is published |
| SMVL/OpenLogo/LogoDet-3K | Useful technically | Rejected for production corpus due research-only or unclear image rights |

The tracked targeted broadcast-positive audit is preserved at
`tools/corpus/broadcast-positive-source-audit.json`; it records per-source
license evidence, accepted frame timestamps and fingerprints, and why
scoreboards, channel marks, raw panoramic match footage, and scene-only sponsor
branding do not satisfy the positive-overlay requirement. Extracted pool frames
remain local/ignored and still require attributable human review.

Licensing research is technical due diligence, not legal advice. Trademark,
publicity, event, and venue rights remain separate concerns.

## 1. Fetch an annotation pool

```bash
tools/.venv/bin/python tools/corpus/fetch_wikimedia.py \
  --output tools/datasets/sports-ads/pool \
  --limit-per-query 100 \
  --query 'stock car racing sponsor livery' \
  --query 'association football jersey sponsor' \
  --query 'stadium advertising board' \
  --query 'Formula One sponsor livery' \
  --category 'Hong Kong v Inter Miami, 4 February 2024'
```

Full-text search and Commons categories can be mixed. Category mode only walks
file members; it does not silently recurse into subcategories. The fetcher
checks each file's own Commons metadata and writes `manifest.jsonl`. Re-running
the same command resumes safely through stable source IDs and existing manifest
records; use `--request-delay 3` (or higher) when Commons begins throttling.
Noisy full-text searches can use repeatable `--require-title-term football`
(or terms such as `match`, `scoreboard`, and `broadcast`); a result is downloaded
only when its Commons title contains at least one supplied term. This is an
acquisition filter, not a substitute for annotation review. For a category
containing related frames/photos from one event, pass
`--group-category-members` so every member shares one split group. Do not use
that flag for broad topical categories whose files are genuinely independent.
Downloaded data is gitignored.

## 2. Review and annotate

For each `image.jpg`, create `image.jpg.labels.json`:

```json
{
  "reviewed": true,
  "review_method": "human",
  "boxes": [
    {
      "class": "Logo",
      "x": 0.12,
      "y": 0.23,
      "width": 0.18,
      "height": 0.09,
      "placement": "car_livery"
    }
  ],
  "preserve_regions": [
    {
      "kind": "jersey_number",
      "x": 0.58,
      "y": 0.32,
      "width": 0.08,
      "height": 0.12
    }
  ]
}
```

Coordinates are normalized with a top-left origin. Keep reviewed images with an
empty `boxes` array: sports scenes, patterned clothing, car numbers, team names,
team crests, scoreboards, and text are essential hard negatives. Optional
`negative_placements` (for example `["ordinary_screen", "broadcast_overlay"]`)
is preserved through corpus and fixture export to document what a contextual
hard negative challenges without inventing a positive target. Add
`preserve_regions` for any visible team identity that must be checked explicitly
in held-out evaluation; never draw it as a sponsor box.

Split grouping must identify the event/video/photo sequence. Never let adjacent
frames from one source land in different splits.

Generate a compact, greedy independent-group review handoff with
`review_labels.py --pool tools/datasets/sports-ads/pool --plan`. The plan chooses
at most one representative per split group to cover positive placement,
preservation, and contextual-negative facets, and reports facets that the pool
cannot yet satisfy. It is prioritization only; every selected image still needs
human review. When served with `--plan-file`, each selected image shows its
`covers` facets in the review header so the reviewer knows why it was chosen;
these hints never replace inspection or determine saved labels.

For explicit human approval, launch the loopback-only review UI:

```bash
PYTHONPATH=tools tools/.venv/bin/python tools/corpus/review_labels.py \
  --pool tools/datasets/sports-ads/pool --port 8765 \
  --plan-file tools/datasets/sports-ads/human-review-plan.json \
  --reviewer 'reviewer@example.com'
# Open http://127.0.0.1:8765
```

Audit the preserved handoff at any time without starting the server:

```bash
PYTHONPATH=tools tools/.venv/bin/python tools/corpus/review_labels.py \
  --pool tools/datasets/sports-ads/pool \
  --plan-file tools/datasets/sports-ads/human-review-plan.json --plan-status
```

This reports planned approvals, exclusions, pending/missing files, deficits
from a freshly recomputed candidate-pool plan, and SHA-256 fingerprints of the
plan plus all manifests/label sidecars. The fingerprints make a copied status
artifact auditable against later review changes. Choose **Block sponsor** to draw a runtime block region, or **Keep team
identity** to draw a `team_name`, `jersey_number`, `vehicle_number`,
`team_crest`, or `manufacturer_badge` preservation
region. Contextual hard-negative checkboxes are visible and editable; they are
never silently inherited as human decisions. Click any region to delete it,
then approve. Approval validates runtime classes, placement taxonomy, and
normalized coordinates and atomically replaces the sidecar with
`review_method: human`, `reviewed_by`, and a timezone-aware `reviewed_at`.
Human-gated builds reject missing/malformed reviewer provenance. **Reject
unusable** records an attributed, timestamped exclusion reason; excluded images
leave the queue and are skipped by corpus builds rather than becoming accidental
hard negatives. The server rejects paths outside the pool. Use `--list`
to export the remaining queue without starting a server, or `--summary` for
machine-readable all-method versus human-approved source-group coverage by
placement and preservation kind.

The queue measures coverage using only human-approved source groups. It shows
one strongest candidate from each event/photo group before related frames, and
recalculates after every approval so reviewers spend early effort on independent
evidence rather than near-duplicate coverage. Remaining frames from a group
that already has a human approval are deferred as well.

## 3. Build the corpus

```bash
tools/.venv/bin/python tools/corpus/build_sports_corpus.py \
  --input tools/datasets/sports-ads/pool \
  --output tools/datasets/sports-ads/yolo-v1

# Mandatory for a promotion-grade corpus:
tools/.venv/bin/python tools/corpus/build_sports_corpus.py \
  --input tools/datasets/sports-ads/pool \
  --output tools/datasets/sports-ads/yolo-human-reviewed-v1 \
  --require-review-method human \
  --stratify-placement jersey \
  --stratify-placement car_livery \
  --stratify-placement venue_board \
  --stratify-preservation-kind vehicle_number \
  --stratify-preservation-kind team_crest \
  --stratify-preservation-kind manufacturer_badge \
  --require-test-placement jersey \
  --require-test-placement car_livery \
  --require-test-placement venue_board
```

Outputs:

- `data.yaml` and YOLO images/labels for train/val/test
- `provenance.jsonl` with source/license/checksum/split
- `stats.json` with class, placement, preservation-region, per-split placement,
  review-method, skipped, and split counts

The builder verifies checksums, enforces runtime class names, rejects unsafe
licenses, removes exact/perceptual duplicates, preserves hard negatives, and
assigns deterministic group-level splits. Placement stratification requires at
least three independent source groups per requested placement or preservation
kind and reserves one each for train, validation, and test; it fails rather than
duplicating a source group across splits.

## Human/source handoff

The concise resume checklist is
`tools/datasets/sports-ads/NEXT-ACTIONS.md`. It covers attributable review,
broadcast-positive acceptance criteria, readiness refresh, retraining, schema-5
verification, and fingerprint-bound installation.

## Full blocked-handoff verification

From the repository root, run:

```bash
tools/.venv/bin/python tools/verify_sports_ads_handoff.py
```

This fresh-shell command runs the Python and macOS suites, regenerates the
fingerprinted review-plan status, and reruns the unified promotion gate. Its
shared positive-placement requirements explicitly include `broadcast_overlay`
alongside livery, jersey, venue-board, and ordinary-screen evidence. The gate
and installer reject reports whose configuration omits any required positive,
preservation, or contextual-negative facet. They also reject weakened numeric
limits: aggregate and per-placement recall must each be at least 0.50, precision
at least 0.50, aggregate false positives at most 10, CoreML p95 at most 10 ms,
and required preservation/contextual-negative false positives must be zero. CLI
options may tighten but cannot weaken these policies. It
passes only when implementation tests pass, planned files are present, and the
current report fails for the explicitly expected missing-human-review blocker.
Logs, `environment.json`, and `summary.json` are preserved under
`tools/runs/sports-ads-handoff-verification/`; the environment snapshot records
Python/Xcode/macOS/architecture plus the requirements-file fingerprint. Once human review is complete,
replace this blocked-handoff check with a passing promotion verification rather
than weakening or inverting its expectation.

## Promotion requirements

A candidate cannot replace the bundled model until a held-out test set shows:

- overall precision and recall both improve over the current model;
- no placement slice (`jersey`, `car_livery`, `venue_board`, etc.) regresses;
- hard-negative false-positive rate stays within the recorded ceiling;
- team names, jersey/vehicle numbers, team crests, and manufacturer badges have zero (or explicitly gated)
  false positives on held-out preservation regions;
- CoreML labels are exactly `Logo`, `Ad banner`, `Sponsored`;
- CoreML p95 latency remains inside the configured inference budget.

The eval fixture exporter carries ground-truth placement metadata from
`annotations.jsonl`. Enforce slice recall explicitly, for example:

```bash
tools/.venv/bin/python tools/eval/run_eval.py \
  --model tools/runs/candidate/weights/best.pt \
  --fixtures tools/datasets/sports-ads/eval-real-v8 \
  --assert-placement-recall car_livery=0.50 \
  --assert-placement-recall jersey=0.50 \
  --assert-placement-recall venue_board=0.50 \
  --max-preservation-false-positives team_name=0 \
  --max-preservation-false-positives jersey_number=0 \
  --max-preservation-false-positives vehicle_number=0 \
  --max-preservation-false-positives team_crest=0 \
  --max-preservation-false-positives manufacturer_badge=0
```

Placement precision is not reported because placement is ground-truth metadata,
not a model output. Unmatched false positives remain in overall/per-class
precision instead of being assigned an invented placement. A preservation gate
fails when a detection covers at least half of an explicitly kept region; tune
that threshold with `--preservation-coverage` only with documented evidence.
For diagnostic threshold selection, `tools/eval/run_score_sweep.py` evaluates
explicit repeatable `--score` values on one fixed fixture set and records every
row plus best F1. A best threshold does not relax any promotion floor.
Contextual hard negatives are reported under `negative_placements`; use repeatable
`--max-negative-placement-false-positives ordinary_screen=0` gates to enforce a
slice ceiling without pretending the detector predicts placement. Corpus builds
can reserve independent hard-negative groups with repeatable
`--stratify-negative-placement`; `verify_promotion.py --negative-placement`
applies that prerequisite before comparing candidate and baseline slices.

For final promotion, use `tools/verify_promotion.py` rather than running gates
individually. It builds only explicit human reviews, stratifies requested
placements, evaluates candidate and baseline on the same fixtures, rejects
aggregate/slice/preservation regressions, validates both CoreML artifacts, and
benchmarks p95 latency. It fingerprints the source pool, generated corpus and
fixtures, and every candidate/baseline artifact, writes a machine-readable
failure report, and never installs a model. Only after
`passed: true`, run `tools/install_verified_model.py --report REPORT
--destination Sources/liveblock-detector.mlpackage`; the installer requires all
gate sections, the current gate schema and fingerprints for corpus build, fixture export,
evaluation, CoreML validation, and orchestration code, plus matching pool,
corpus, fixture, baseline, and candidate path/content fingerprints. It rejects
stale or failed reports, swaps atomically,
and preserves the previous bundle as `.pre-promotion`.
