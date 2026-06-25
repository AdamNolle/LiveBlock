# Eval fixtures

Labeled images for the detector eval harness. **Added once the model exists**
(plan Task 10) — there is no point measuring accuracy before there is a baked
`liveblock-detector` artifact to measure.

Each fixture is an image plus an optional JSON sidecar of the same stem:

```
logo_acme.png
logo_acme.json
toolbar_negative.png    # hard negative, no sidecar (or "negative": true)
```

Sidecar format:

```json
{
  "boxes": [
    { "class_id": 0, "box": [x, y, w, h] }
  ],
  "negative": false
}
```

- `class_id` indexes `tools/vocab/liveblock-vocab.json` (0=Logo, 1=Ad banner,
  2=Sponsored).
- `box` is `[x, y, w, h]` in pixels.
- Include **hard negatives** (app toolbars, real article content, UI chrome)
  with no boxes so false positives are penalized.

See `../README.md` for how to run an evaluation.
