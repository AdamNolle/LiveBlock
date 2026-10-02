# LiveBlock training and release tools

The desktop apps use local inference. Training runs as a foreground, cancellable
job and retains its checkpoint and evaluation report. Automatic removal accepts
only `ad` and `sponsor_remove`; `team_keep` and `number_keep` protect content.
Generic COCO class IDs never authorize removal.

## Public starter dataset

The starter uses [AdSERP](https://zenodo.org/records/15236546), licensed under
[CC BY 4.0](https://creativecommons.org/licenses/by/4.0/). Attribution:
Kayhan Latifzadeh, Jacek Gwizdka, Luis A. Leiva. 2025. *A Versatile Dataset of
Mouse and Eye Movements on Search Engine Results Pages*. SIGIR 2025.
DOI: 10.1145/3726302.3730325.

Only screenshots, ad boundaries and query metadata are imported. The importer
verifies all archive checksums, selects one page per query, creates viewport
crops, keeps negative examples, and groups train/validation/test splits by query.
It writes `ATTRIBUTION.md` and `provenance.json` beside `data.yaml`.

```bash
PYTHON=python3.12 tools/setup_env.sh
tools/.venv/bin/python tools/prepare_adserp.py --download
tools/.venv/bin/python tools/train_logos.py \
  --data tools/datasets/adserp/yolo/data.yaml \
  --epochs 20 --batch 8 --imgsz 640 --screen-ads \
  --run-name adserp-v1 --no-int8 --install
```

The prepared starter contains 2,494 training, 571 validation and 573 test
screenshots from 900 distinct queries. It covers search ads. It has no labeled
sports sponsors, team emblems or jersey numbers, and establishes no accuracy
for those domains. See [MODEL_CARD.md](../MODEL_CARD.md) for measured results.

Windows setup uses `pwsh -File tools/setup_env.ps1`, then
`tools/.venv/Scripts/python.exe` for these commands. Linux uses `setup_env.sh`.
Ultralytics code is provided under its
[AGPL-3.0 license](https://github.com/ultralytics/ultralytics/blob/main/LICENSE);
dataset attribution and model/dependency licensing are separate.

## Personal labels and custom datasets

Draw boxes in the Labeling window and select each box's intent. Explicitly mark
screenshots without ads as negatives. The Training window can train these
labels or an existing YOLO `data.yaml`.

```bash
tools/.venv/bin/python tools/train_from_labels.py --epochs 50 --batch 8
```

Exports reject unknown classes, invalid coordinates and empty splits. A candidate
must achieve at least 0.90 precision and 0.80 recall before `--install` succeeds.
When a test split exists, evaluation uses it; otherwise the report clearly says
`val`. These thresholds validate a dataset, not every screen or streaming service.
The Mac app compiles and reloads the installed CoreML model. The other ports
load a fresh model on capture start.

## Verify deployment exports

Use the same screenshot split through the actual inference path:

```bash
swift tools/validate_coreml.swift MODEL.mlpackage IMAGES_DIR coreml-predictions.json
tools/.venv/bin/python tools/evaluate_predictions.py \
  --predictions coreml-predictions.json --labels LABELS_DIR --output coreml-results.json

cargo run --release --manifest-path tools/validate_onnx/Cargo.toml -- \
  linux PATH_TO_ORT_LIBRARY MODEL.onnx IMAGES_DIR linux-predictions.json
```

The Rust harness also accepts `windows` and imports that port's real detector.
It tests preprocessing, ONNX inference and decoding; it does not test native
screen capture or compositor behavior on a different OS. ONNX class metadata
must match the model's SHA-256 before automatic removal is permitted.

## Bundle ONNX Runtime

```bash
python3 tools/setup_onnx_runtime.py --platform windows --arch x64
python3 tools/setup_onnx_runtime.py --platform linux --arch x64
# For a native Linux ARM64 build:
python3 tools/setup_onnx_runtime.py --platform linux --arch aarch64
```

This downloads official ONNX Runtime 1.24.4 CPU binaries, verifies their published
SHA-256 digests, and preserves LICENSE and ThirdPartyNotices in the app resources.
Bundle the matching architecture before building each installer. Missing runtime
or model files leave manual blocking available and disable automatic detection.
An optional GPU runtime can be selected with `ORT_DYLIB_PATH`.

## Mac signing and notarization

```bash
tools/release_macos.sh
# Save credentials locally once; never put passwords in a repository or chat:
xcrun notarytool store-credentials LiveBlock --team-id RSDJXF7DXN
tools/release_macos.sh --notarize LiveBlock
```

The script builds ARM64 and Intel, signs with Developer ID and hardened runtime,
checks both architectures and the compiled model, and creates the ZIP under
`~/Library/Caches/LiveBlock/release`. Notarization requires a valid keychain
profile. Only Apple's `Accepted` response, successful stapling and Gatekeeper
assessment establish a notarized release. No Gatekeeper exceptions are installed.
