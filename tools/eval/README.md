# LiveBlock detector eval harness

Measures the open-vocab detector's accuracy so "blocks any logo" is a number,
not a hope. Reports precision/recall overall and per vocab class, supports a
regression floor via `--assert-recall`, and includes hard negatives that must
*not* be blocked.

## Files

- `run_eval.py` — `iou()`, `greedy_match()`, and `evaluate(model, fixtures)`.
  Model/torch/ultralytics imports are **lazy** (done inside the functions that
  load a model) so the metric code and unit tests run on a bare system Python
  with no ML venv.
- `test_eval.py` — pure-metric unit tests (no model required).
- `fixtures/` — labeled images + JSON sidecars (added once a model exists).

## Run the unit tests (no venv needed)

```bash
cd tools/eval
python3 -m pytest test_eval.py -q
```

## Run a full evaluation (needs the model + ML venv)

The baked detector artifact (`Sources/liveblock-detector.mlpackage` /
`models/liveblock-detector.onnx`) is produced by plan Task 10. Once it exists:

```bash
cd tools/eval
../.venv/bin/python run_eval.py \
    --model ../../Sources/liveblock-detector.mlpackage \
    --fixtures fixtures \
    --assert-recall 0.30
```

`--assert-recall FLOAT` exits non-zero when overall recall drops below the
floor. Start at `0.30` and raise it as the baseline improves.

## Fixture format

For each image `foo.png`, an optional sidecar `foo.json`:

```json
{
  "boxes": [
    { "class_id": 0, "box": [x, y, w, h] }
  ],
  "negative": false
}
```

- `box` is `[x, y, w, h]` in pixels.
- A fixture with no boxes, or `"negative": true`, is a **hard negative**: any
  detection on it is counted as a false positive (app toolbars, real content).

## Baseline

_Pending_: recorded against the Task 10 model. The harness drives which prompts
survive and what per-class thresholds get written back into
`tools/vocab/liveblock-vocab.json` + the default `DetectionSettings`.
