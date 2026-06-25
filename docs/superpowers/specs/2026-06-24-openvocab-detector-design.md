# LiveBlock — Open-Vocabulary "No-Training" Detector Engine (Slice 1)

**Date:** 2026-06-24
**Status:** Approved (design). Route A now, Route B (live prompts) committed as the next slice.
**Author:** Claude (ultracode session) + AdamNolle
**Supersedes detector portions of:** `ASSESSMENT-2026-05-13.md` §3 (Phase β)

---

## 1. Goal & context

LiveBlock visually removes ads/logos from the screen: capture → detect → inpaint → click-through overlay. The shipped detector is **generic YOLOv8n on COCO** (finds people/cars/dogs, *not* ads). The product requirement:

> Block any brand sponsorship or logo **without the user having to train data.**

This slice builds the **open-vocabulary, zero-shot detector engine** that satisfies that requirement, in/around the **shared Rust core** (the user's chosen starting point). The user never captures or labels anything; the "vocabulary" is *words*, not labeled images; and because the model is open-vocabulary it generalizes to brands it has never seen.

### Decision (approved)

- **Route A (this slice):** Ship a model whose vocabulary is **baked offline**. A pretrained **YOLO-World-v2** open-vocab model is reparameterized with a fixed *concept-level* vocabulary via `set_classes(VOCAB)` (which folds CLIP text embeddings into the detection head), then exported to **CoreML** (macOS) and **ONNX** (Windows/Linux). At runtime it runs as a plain YOLO — no text encoder, real-time, near-zero Swift change.
- **Route B (next slice, committed):** Add an on-device CLIP text encoder + Swift raw-decode/NMS so the user can *type new prompts live*. Out of scope here; previewed in §13.

### Why Route A is correct for "any logo without training"

Open-vocabulary means concept prompts generalize: `logo` / `brand logo` fire on a never-before-seen startup logo, not just trained brands. The user never labels. Re-vocabbing is a one-command offline rebuild. It holds 60fps on-device and preserves the project's "no frames leave the machine" rule. Reference: [YOLO-World (Ultralytics)](https://docs.ultralytics.com/models/yolo-world), [YOLOE](https://docs.ultralytics.com/models/yoloe), [CoreML export](https://docs.ultralytics.com/integrations/coreml).

---

## 2. Architecture: where things live today (verified)

- **macOS (live app):** `Sources/VisionProcessor.swift` uses Apple **Vision + a CoreML NMS-pipeline model** (`Sources/yolov8n.mlpackage`). Letterbox/decode/NMS are **baked inside the model**; Vision returns decoded `VNRecognizedObjectObservation`s. Classes live in the model's `classes` metadata. **The macOS path never touches the Rust crate.**
- **Windows/Linux (Tauri ports):** `platform/{windows,linux}/src-tauri/src/detection.rs` run a raw ONNX head via `ort` and decode/NMS in Rust. Linux uses the shared `liveblock-detection` crate; Windows has an inline copy.
- **Shared core (`core/crates/`):** `liveblock-detection` owns `Detection`, `filter_by_score`, `iou`, `non_max_suppression` (all class-count-agnostic — they key on `class_id: u32` and derive `nc` dynamically, so they need **no change** for an open vocab). `liveblock-core` owns the `Detector`/`Capture`/`Inpainter` traits + `Coordinator`. `liveblock-bridge` exposes only `RegionStore` to Swift today.

**Key consequence:** "detector engine in the shared core" = (a) add the shared **vocabulary/settings config** to the core, (b) build the shared **offline model-build pipeline**, (c) **wire each platform** to read names/thresholds from the shared config, (d) an **eval harness**. The post-processing math is already shared and already correct for any `nc`.

---

## 3. Component 1 — `liveblock-config` crate (new core leaf)

New leaf crate `core/crates/liveblock-config`, registered in `core/Cargo.toml` members, re-exported through `liveblock-core/src/lib.rs` (mirroring regions/labels/detection), deps `serde + serde_json + thiserror` only (no platform deps).

### Modules

**`vocabulary.rs`** — the open-vocab class list (shared source of truth, replaces 3 hardcoded COCO tables):
```rust
pub struct VocabClass { pub id: u32, pub name: String, pub prompts: Vec<String> }
pub struct Vocabulary { pub version: u32, pub classes: Vec<VocabClass> }
```
`Detection.class_id` indexes `Vocabulary.classes`.

**`embeddings.rs`** — validator/loader for the precomputed text-embedding sidecar (used by the offline tool and, later, Route B):
```rust
pub struct ClassEmbeddings { pub vocab_version: u32, pub dim: u32, pub model_tag: String, pub vectors: BTreeMap<u32, Vec<f32>> }
```
`vocab_version` must match `Vocabulary.version` (guards stale assets). Bytes stored as a sidecar `embeddings.bin`; this struct only validates/loads.

**`settings.rs`** — per-class rules + global thresholds, persisted like `RegionStore`:
```rust
pub struct ClassRule { pub class_id: u32, pub enabled: bool, pub score_threshold: Option<f32> }
pub struct DetectionSettings { pub global: CoordinatorConfig, pub classes: Vec<ClassRule>, pub vocabulary_version: u32 }
pub struct SettingsStore { /* Mutex<...> + Option<PathBuf>, mirrors RegionStore::{in_memory,open,current,persist} */ }
```

### Serialization (must match Swift byte-for-byte)

Reuse the existing convention: encode through `BTreeMap<&'static str, Value>` + `PrettyFormatter::with_indent(b"  ")` (2-space, sorted keys), uppercase UUIDs, ISO-8601 dates. Add ordering-assertion tests mirroring `liveblock-regions/src/lib.rs:295-303`.

### swift-bridge surface (`liveblock-bridge/src/lib.rs`)

Add a second opaque handle next to `RegionStoreHandle`, JSON-string transport (swift-bridge 0.1 can't marshal rich structs):
```rust
type VocabularyHandle;
fn new() -> VocabularyHandle;
fn vocabulary_open(path: String) -> VocabularyHandle;
fn set_vocabulary(self: &VocabularyHandle, json: String) -> bool;
fn get_detector_classes(self: &VocabularyHandle) -> String;     // [{id,name,enabled,threshold}]
fn set_class_enabled(self: &VocabularyHandle, class_id: u32, enabled: bool) -> bool;
fn set_class_threshold(self: &VocabularyHandle, class_id: u32, threshold: f32) -> bool;
fn to_json(self: &VocabularyHandle) -> String;
```
`build.rs` auto-regenerates glue. `bridge/Cargo.toml` adds the `liveblock-config` path dep.

---

## 4. Component 2 — offline model-build tool (`tools/build_openvocab.py`)

The "no training" engine. Pure offline; nothing trains on user data.

```python
from ultralytics import YOLOWorld
m = YOLOWorld("yolov8s-worldv2.pt")   # pretrained open-vocab base
m.set_classes(VOCAB_PROMPTS)          # bakes CLIP text embeddings into the head -> prompt-free model
m.save("liveblock-detector.pt")
# export (reuse/extend tools/export_to_coreml.py):
m.export(format="coreml", nms=True, int8=True, imgsz=640)   # -> liveblock-detector.mlpackage (macOS)
m.export(format="onnx",  nms=True, imgsz=640, opset=...)     # -> liveblock-detector.onnx  (Win/Linux)
```

- **Reuse:** `tools/setup_env.sh` venv, `tools/export_to_coreml.py` (`model.export` + `install_into_repo`), `tools/auto.sh` orchestration shape.
- **New/changed:** add `build_openvocab.py`; extend exporter with an ONNX branch; the vocabulary is read from a checked-in `tools/vocab/liveblock-vocab.json` (same list the `liveblock-config` crate loads).
- **Toolchain provisioning:** export needs Python **3.10–3.12** (coremltools breaks on 3.13; this Mac has 3.13). Provision a pinned 3.12 venv (via `uv`/`pyenv`) in `setup_env.sh`. Add to `requirements.txt`: the CLIP text-encoder deps (`clip`/`ftfy`/`regex` or HF), `onnx`, `onnxruntime`, `onnxslim`. Pre-cache `yolov8s-worldv2.pt` + CLIP weights so the *build* is reproducible (the *runtime* is already fully offline).
- **NMS stays baked** (`nms=True`) — the macOS Vision path has no raw-tensor decoder, so baked NMS is required for Route A.

### Model naming (rename from `yolov8n` → `liveblock-detector`)

The name `yolov8n` is hardcoded in three places — update all to `liveblock-detector` (honest + de-COCOs the bundle):
1. `Sources/VisionProcessor.swift` loadModel (`:189-211`) runtime-dir + bundle lookups.
2. `Sources/TrainingController.swift` hot-reload copy (`installFreshlyTrainedModel` ~`:278-302`).
3. `tools/export_to_coreml.py` `TARGET_PATH`.
Bundled via the `Sources/` glob in `project.yml` (no resources entry needed).

---

## 5. Component 3 — per-platform wiring

- **macOS (Route A = minimal):** drop `liveblock-detector.mlpackage` (new vocab) into the bundle; loader picks it up. `obs.labels.first.identifier` now returns vocab names. No change to `AdBoundingBox`, threshold plumbing, cadence, cache, or inpaint. Per-class thresholds/enable read from `liveblock-config` via the new bridge handle (replacing the single `_minimumConfidence`).
- **Windows:** replace hardcoded `coco_class_names()` (`detection.rs:219-233`) with names from `liveblock-config`. Output head `[1, 4+nc, N]` already adapts (`nc = c-4`). Swap the model path to `liveblock-detector.onnx`. (Runtime parity beyond wiring is a later slice; this slice only removes the COCO hardcode + points at the new model.)
- **Linux:** add the shared class names (currently only `class_id`); point at `liveblock-detector.onnx`. Decode/filter/NMS unchanged.

**Unchanged everywhere:** `liveblock-detection` post-processing, letterbox math, `Detection`/`DetBox`/`AdBoundingBox`, the 60Hz-capture / 15Hz-detect / 60Hz-inpaint / 30Hz-publish decoupling.

---

## 6. Component 4 — eval harness (`tools/eval/`)

"Blocks any logo" is unverifiable without numbers.
- A pinned held-out image set (logos/ads + hard negatives that must **not** be blocked: app toolbars, real content).
- `pytest`-driven: report precision/recall per vocab class + overall; assert a regression floor (start low, raise as baselines ship).
- A before/after comparison vs. the previously shipped model so vocabulary/threshold tuning is measured, not guessed.
- This harness is how the default vocabulary in §7 gets *tuned* rather than assumed.

---

## 7. Default vocabulary (tuned via the eval harness, not fixed in stone)

Concept-level prompts grouped into a few UI-facing classes. Starting point:

| class_id | name (UI) | prompts |
|---|---|---|
| 0 | Logo | `logo`, `brand logo`, `company logo` |
| 1 | Ad banner | `advertisement`, `banner advertisement`, `advertising banner` |
| 2 | Sponsored | `sponsored content`, `sponsored post`, `promotional banner` |

Honest note: object-noun prompts (`logo`) score far better than layout-semantic ones (`sponsored`) on YOLO-World. The eval harness drives which prompts survive and what thresholds they get. Broader vocab → more false positives, bounded by per-class `score_threshold` + an app allowlist.

---

## 8. Verification plan

- **Core:** `cargo test --manifest-path core/Cargo.toml --workspace` (new `liveblock-config` tests + unchanged detection tests green).
- **Bridge:** `LB_BRIDGE_HOST_ONLY=1 tools/build_bridge.sh` regenerates Swift glue; compiles.
- **Swift app:** `./run.sh --build` (no GUI) green; then a **real launch** on this Mac (macOS 26.5.1) with Screen Recording granted — confirm it blocks a real on-screen ad/logo, not just compiles. (Repo house rule: UI changes need an actual run.)
- **Model:** the offline tool produces a loadable `.mlpackage` + `.onnx`; eval-harness numbers recorded before/after.
- **Footguns to clear first:** delete `platform/_shared-frontend/dist/* 2.html` duplicates; ensure no `LiveBlock *.xcodeproj` duplicate; keep the stable codesign identity so TCC grants persist.

---

## 9. Risks & mitigations

| Risk | Mitigation |
|---|---|
| YOLO-World CoreML/NMS export less battle-tested | Validate `.mlpackage` loads + detects on a fixture before wiring; if CoreML NMS export fails, evaluate YOLOE as the base, or fall back to `nms=False` + (defer to Route B's Swift decoder). |
| Concept prompts weak for "sponsored/ad" | Eval harness tunes/drops weak prompts; lead with strong ones (`logo`). |
| False positives blocking real UI | Per-class thresholds + confidence floor + app allowlist (per-app rules already exist). |
| Python 3.13 breaks coremltools | Pin a 3.12 venv in `setup_env.sh`. |
| Model size grows vs. 6.5MB yolov8n | Use `-s` variant + int8; measure bundle impact. |
| Byte-incompat JSON vs Swift | Reuse the exact BTreeMap/2-space/sorted-keys/uppercase-UUID path + ordering tests. |

---

## 10. Out of scope (this slice)

- v4 UI reskin (separate slice; design captured in `design/` + the design map).
- Windows/Linux *runtime* parity beyond config-wiring + model swap.
- Route B (on-device live prompts) — §13, next slice.
- New inpainting (LaMa/MPS), multi-display, auto-update, signing/notarization.

---

## 11. Success criteria

1. `liveblock-config` exists, is re-exported by core, bridged to Swift, fully tested, byte-compatible.
2. `tools/build_openvocab.py` produces `liveblock-detector.{mlpackage,onnx}` from a baked open-vocab model with **zero user-supplied training data**.
3. macOS app loads the new model, reads vocab/thresholds from shared config, and **visibly blocks a real on-screen logo/ad** on a live run.
4. Windows/Linux detectors read class names from shared config (no more hardcoded COCO) and point at the new model.
5. Eval harness reports numbers; default vocabulary chosen by data.
6. All existing tests stay green; core + bridge + Swift app all build.

---

## 12. Build/run quick reference

```bash
cargo test  --manifest-path core/Cargo.toml --workspace      # core loop
cargo check --manifest-path core/Cargo.toml --workspace      # fastest compile signal
LB_BRIDGE_HOST_ONLY=1 tools/build_bridge.sh                  # bridge glue (host-only)
./run.sh --build                                             # Swift compile, no launch
./run.sh                                                     # build + launch (verify live)
xcodegen generate                                           # after editing project.yml
```

---

## 13. Route B preview (next slice — committed)

Add `setPrompts([String])` on-device: ship a CLIP text-encoder CoreML model + tokenizer; on prompt change, tokenize → encode → L2-normalize → cache `[num_prompts, embed_dim]`; switch `VisionProcessor` from `VNCoreMLModel` to direct `MLModel.prediction` with two inputs (image + embeddings); add a Swift letterbox + Swift port of (or FFI call to) `decode_yolov8_head` + `filter_by_score` + `non_max_suppression`. The `liveblock-config` `embeddings.rs` asset format from this slice is the seam Route B plugs into.
