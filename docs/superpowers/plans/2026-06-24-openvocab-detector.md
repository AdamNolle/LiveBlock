# Open-Vocabulary "No-Training" Detector Engine — Implementation Plan (Slice 1)

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Ship an open-vocabulary, zero-shot ad/logo detector that works out-of-the-box (no user training), built around the shared Rust core, and prove it blocks a real on-screen logo on macOS.

**Architecture:** A new `liveblock-config` core leaf crate becomes the single source of truth for the open-vocab vocabulary, the precomputed text-embedding asset format, and per-class detection settings — bridged to Swift. An offline tool bakes a fixed concept-level vocabulary into a pretrained YOLO-World-v2 model (`set_classes` → CLIP embeddings folded into the head) and exports CoreML + ONNX. Each platform reads class names/thresholds from the shared config and loads the new model. Post-processing (decode/NMS) is already shared and class-count-agnostic, so it does not change.

**Tech Stack:** Rust (serde, swift-bridge), Python 3.12 (ultralytics YOLO-World-v2, coremltools, onnx), Swift (Vision + CoreML), macOS ScreenCaptureKit.

## Global Constraints

- **JSON byte-compatibility:** all new persisted JSON uses `BTreeMap<&'static str, Value>` + `serde_json::ser::PrettyFormatter::with_indent(b"  ")` (2-space indent, sorted keys); UUIDs uppercased; dates ISO-8601 `yyyy-MM-ddTHH:mm:ssZ`. Must match Swift `JSONEncoder([.prettyPrinted, .sortedKeys])`.
- **`liveblock-config` crate deps:** `serde`, `serde_json`, `thiserror` only. No platform deps. No `uuid`/`tokio`.
- **NMS stays baked into the exported model** (`nms=True`) — the macOS Vision path has no raw-tensor decoder in slice 1.
- **Model artifact name:** `liveblock-detector` (rename every hardcoded `yolov8n`). Input size **640×640**.
- **Export toolchain:** Python **3.10–3.12** (coremltools breaks on 3.13). This machine has 3.13 → provision a pinned 3.12 venv.
- **Privacy:** no frames leave the machine. The runtime model is fully offline (only the *build* step downloads weights).
- **Verification honesty:** do not claim the app works until it has been run live (repo house rule). Build-green + tests-green is necessary, not sufficient, for UI/runtime claims.
- **Deployment target:** macOS 26.0, Swift 5.10.

---

## File Structure

**Create:**
- `core/crates/liveblock-config/Cargo.toml` — new leaf crate manifest.
- `core/crates/liveblock-config/src/lib.rs` — crate root, re-exports modules, shared JSON helper.
- `core/crates/liveblock-config/src/vocabulary.rs` — `VocabClass`, `Vocabulary`.
- `core/crates/liveblock-config/src/embeddings.rs` — `ClassEmbeddings` validator/loader.
- `core/crates/liveblock-config/src/settings.rs` — `ClassRule`, `DetectionSettings`, `SettingsStore`.
- `tools/vocab/liveblock-vocab.json` — default vocabulary (shared by crate tests + offline tool).
- `tools/build_openvocab.py` — bake vocab into YOLO-World-v2, save `.pt`.
- `tools/eval/run_eval.py` + `tools/eval/test_eval.py` + `tools/eval/README.md` — accuracy harness.

**Modify:**
- `core/Cargo.toml` — add `crates/liveblock-config` member.
- `core/crates/liveblock-core/Cargo.toml` + `src/lib.rs` — depend on + re-export `liveblock-config`.
- `core/crates/liveblock-bridge/Cargo.toml` + `src/lib.rs` — add `VocabularyHandle` FFI.
- `tools/setup_env.sh` + `tools/requirements.txt` — 3.12 venv + ML deps.
- `tools/export_to_coreml.py` — rename target to `liveblock-detector`, add ONNX export branch.
- `Sources/VisionProcessor.swift` — model name rename, read vocab/thresholds from config.
- `Sources/TrainingController.swift` — hot-reload copy name rename.
- `platform/windows/src-tauri/src/detection.rs` — class names from config; model path.
- `platform/linux/src-tauri/src/detection.rs` — class names from config; model path.
- `platform/_shared-frontend/dist/*` — delete `" 2.html"` duplicates.

---

## Component A — `liveblock-config` core crate

### Task 1: Scaffold crate + `Vocabulary` with byte-compatible JSON

**Files:**
- Create: `core/crates/liveblock-config/Cargo.toml`, `core/crates/liveblock-config/src/lib.rs`, `core/crates/liveblock-config/src/vocabulary.rs`
- Modify: `core/Cargo.toml` (workspace members)

**Interfaces:**
- Produces: `VocabClass { id: u32, name: String, prompts: Vec<String> }`, `Vocabulary { version: u32, classes: Vec<VocabClass> }`, `Vocabulary::to_json(&self) -> String`, `Vocabulary::from_json(&str) -> Result<Vocabulary, ConfigError>`, `pub fn pretty_two_space(value: &serde_json::Value) -> String`.

- [ ] **Step 1: Add crate to workspace.** In `core/Cargo.toml` add `"crates/liveblock-config"` to `members`.

- [ ] **Step 2: Write `Cargo.toml`.**
```toml
[package]
name = "liveblock-config"
version = "0.1.0"
edition = "2021"

[dependencies]
serde = { workspace = true, features = ["derive"] }
serde_json = { workspace = true }
thiserror = { workspace = true }
```

- [ ] **Step 3: Write the failing test** in `src/vocabulary.rs`:
```rust
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn vocabulary_json_is_sorted_two_space() {
        let v = Vocabulary { version: 1, classes: vec![
            VocabClass { id: 0, name: "Logo".into(), prompts: vec!["logo".into(), "brand logo".into()] },
        ]};
        let json = v.to_json();
        // sorted keys: classes < version; class keys: id < name < prompts
        assert!(json.starts_with("{\n  \"classes\": [\n"));
        assert!(json.contains("\n      \"id\": 0,\n      \"name\": \"Logo\",\n      \"prompts\": ["));
        assert!(json.trim_end().ends_with("\"version\": 1\n}"));
        // round-trips
        let back = Vocabulary::from_json(&json).unwrap();
        assert_eq!(back.classes[0].prompts.len(), 2);
    }
}
```

- [ ] **Step 4: Run it, verify it fails.** Run: `cargo test --manifest-path core/Cargo.toml -p liveblock-config` → FAIL (types undefined).

- [ ] **Step 5: Implement `lib.rs`** (root + shared helper + error):
```rust
pub mod vocabulary;
pub mod embeddings;
pub mod settings;

pub use vocabulary::{VocabClass, Vocabulary};
pub use embeddings::ClassEmbeddings;
pub use settings::{ClassRule, DetectionSettings, SettingsStore};

#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    #[error("json error: {0}")] Json(#[from] serde_json::Error),
    #[error("io error: {0}")] Io(String),
    #[error("vocab version mismatch: settings {settings} vs vocabulary {vocab}")]
    VocabVersion { settings: u32, vocab: u32 },
    #[error("embedding version mismatch: {emb} vs vocabulary {vocab}")]
    EmbeddingVersion { emb: u32, vocab: u32 },
}

/// 2-space, sorted-key pretty JSON identical to Swift JSONEncoder([.prettyPrinted,.sortedKeys]).
pub fn pretty_two_space(value: &serde_json::Value) -> String {
    let mut buf = Vec::new();
    let fmt = serde_json::ser::PrettyFormatter::with_indent(b"  ");
    let mut ser = serde_json::Serializer::with_formatter(&mut buf, fmt);
    serde::Serialize::serialize(value, &mut ser).expect("serialize value");
    String::from_utf8(buf).expect("utf8")
}
```
Then `vocabulary.rs`:
```rust
use serde::{Deserialize, Serialize};
use crate::{pretty_two_space, ConfigError};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct VocabClass { pub id: u32, pub name: String, pub prompts: Vec<String> }

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Vocabulary { pub version: u32, pub classes: Vec<VocabClass> }

impl Vocabulary {
    pub fn to_json(&self) -> String {
        // serde_json::Value sorts object keys via BTreeMap when the "preserve_order"
        // feature is OFF (the default), giving sorted keys for free.
        let value = serde_json::to_value(self).expect("to_value");
        pretty_two_space(&value)
    }
    pub fn from_json(s: &str) -> Result<Self, ConfigError> { Ok(serde_json::from_str(s)?) }
    pub fn class_name(&self, id: u32) -> Option<&str> {
        self.classes.iter().find(|c| c.id == id).map(|c| c.name.as_str())
    }
}
```

- [ ] **Step 6: Run tests, verify pass.** Run: `cargo test --manifest-path core/Cargo.toml -p liveblock-config` → PASS. (If key order differs, confirm `serde_json` has no `preserve_order` feature in the workspace.)

- [ ] **Step 7: Commit.**
```bash
git add core/Cargo.toml core/crates/liveblock-config/
git commit -m "feat(config): scaffold liveblock-config crate with Vocabulary"
```

---

### Task 2: `ClassEmbeddings` validator/loader

**Files:**
- Create/Modify: `core/crates/liveblock-config/src/embeddings.rs`

**Interfaces:**
- Consumes: `ConfigError`, `Vocabulary` (Task 1).
- Produces: `ClassEmbeddings { vocab_version: u32, dim: u32, model_tag: String, vectors: BTreeMap<u32, Vec<f32>> }`, `ClassEmbeddings::validate_against(&self, &Vocabulary) -> Result<(), ConfigError>`, `from_json`/`to_json`.

- [ ] **Step 1: Write failing test:**
```rust
#[cfg(test)]
mod tests {
    use super::*; use crate::vocabulary::*;
    #[test]
    fn embeddings_reject_version_mismatch() {
        let mut vectors = std::collections::BTreeMap::new();
        vectors.insert(0u32, vec![0.1f32, 0.2, 0.3]);
        let emb = ClassEmbeddings { vocab_version: 2, dim: 3, model_tag: "clip-vit-b32".into(), vectors };
        let vocab = Vocabulary { version: 1, classes: vec![VocabClass{id:0,name:"Logo".into(),prompts:vec!["logo".into()]}] };
        assert!(matches!(emb.validate_against(&vocab), Err(ConfigError::EmbeddingVersion{..})));
    }
    #[test]
    fn embeddings_accept_matching() {
        let mut vectors = std::collections::BTreeMap::new();
        vectors.insert(0u32, vec![0.1f32,0.2,0.3]);
        let emb = ClassEmbeddings { vocab_version: 1, dim: 3, model_tag: "clip-vit-b32".into(), vectors };
        let vocab = Vocabulary { version: 1, classes: vec![VocabClass{id:0,name:"Logo".into(),prompts:vec!["logo".into()]}] };
        assert!(emb.validate_against(&vocab).is_ok());
    }
}
```

- [ ] **Step 2: Run, verify fail.** `cargo test --manifest-path core/Cargo.toml -p liveblock-config embeddings` → FAIL.

- [ ] **Step 3: Implement:**
```rust
use std::collections::BTreeMap;
use serde::{Deserialize, Serialize};
use crate::{pretty_two_space, ConfigError, vocabulary::Vocabulary};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ClassEmbeddings {
    pub vocab_version: u32,
    pub dim: u32,
    pub model_tag: String,
    pub vectors: BTreeMap<u32, Vec<f32>>,
}

impl ClassEmbeddings {
    pub fn to_json(&self) -> String { pretty_two_space(&serde_json::to_value(self).expect("to_value")) }
    pub fn from_json(s: &str) -> Result<Self, ConfigError> { Ok(serde_json::from_str(s)?) }
    pub fn validate_against(&self, vocab: &Vocabulary) -> Result<(), ConfigError> {
        if self.vocab_version != vocab.version {
            return Err(ConfigError::EmbeddingVersion { emb: self.vocab_version, vocab: vocab.version });
        }
        for c in &vocab.classes {
            match self.vectors.get(&c.id) {
                Some(v) if v.len() as u32 == self.dim => {}
                _ => return Err(ConfigError::Io(format!("missing/mis-sized embedding for class {}", c.id))),
            }
        }
        Ok(())
    }
}
```

- [ ] **Step 4: Run, verify pass.** `cargo test --manifest-path core/Cargo.toml -p liveblock-config embeddings` → PASS.

- [ ] **Step 5: Commit.**
```bash
git add core/crates/liveblock-config/src/embeddings.rs
git commit -m "feat(config): add ClassEmbeddings validator"
```

---

### Task 3: `DetectionSettings` + `SettingsStore` (RegionStore-style persistence)

**Files:**
- Create/Modify: `core/crates/liveblock-config/src/settings.rs`

**Interfaces:**
- Consumes: `ConfigError`, `pretty_two_space`.
- Produces: `CoordinatorConfig { score_threshold: f32, iou_threshold: f32, detect_every: u32 }` (local mirror; or import from core if cycle-free), `ClassRule { class_id: u32, enabled: bool, score_threshold: Option<f32> }`, `DetectionSettings { global: CoordinatorConfig, classes: Vec<ClassRule>, vocabulary_version: u32 }`, `SettingsStore::{in_memory, open, current, set_class_enabled, set_class_threshold, persist}`, `DetectionSettings::effective_threshold(&self, class_id) -> f32`.

> Note: keep `CoordinatorConfig` defined **here** to avoid a `core → config` and `config → core` cycle (config is a leaf). `liveblock-core` will re-export config's `CoordinatorConfig`; remove its own duplicate if present, or alias.

- [ ] **Step 1: Write failing test:**
```rust
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn effective_threshold_prefers_class_override() {
        let s = DetectionSettings {
            global: CoordinatorConfig { score_threshold: 0.35, iou_threshold: 0.45, detect_every: 4 },
            classes: vec![ ClassRule{ class_id: 1, enabled: true, score_threshold: Some(0.6) },
                           ClassRule{ class_id: 2, enabled: true, score_threshold: None } ],
            vocabulary_version: 1,
        };
        assert_eq!(s.effective_threshold(1), 0.6);
        assert_eq!(s.effective_threshold(2), 0.35); // inherits global
        assert_eq!(s.effective_threshold(99), 0.35); // unknown -> global
    }
    #[test]
    fn store_roundtrips_to_tempfile() {
        let dir = std::env::temp_dir().join(format!("lbcfg-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("detection-settings.json");
        let store = SettingsStore::open(path.clone()).unwrap();
        store.set_class_threshold(1, Some(0.7)).unwrap();
        store.persist().unwrap();
        let json = std::fs::read_to_string(&path).unwrap();
        assert!(json.contains("\"score_threshold\": 0.7"));
        assert!(json.starts_with("{\n  \"classes\":")); // sorted, 2-space
    }
}
```

- [ ] **Step 2: Run, verify fail.** `cargo test --manifest-path core/Cargo.toml -p liveblock-config settings` → FAIL.

- [ ] **Step 3: Implement** (mirror `liveblock-regions::RegionStore` atomic-write shape: `Mutex<DetectionSettings>` + `Option<PathBuf>`, write to `pid.nanos.tmp` sibling → fsync → rename). Pull the atomic-write helper pattern from `core/crates/liveblock-regions/src/lib.rs:203-218`. Provide `in_memory()`, `open(path)` (load-or-default), `current()`, mutators that lock+mutate, and `persist()` that serializes via `pretty_two_space`.

- [ ] **Step 4: Run, verify pass.** `cargo test --manifest-path core/Cargo.toml -p liveblock-config settings` → PASS.

- [ ] **Step 5: Commit.**
```bash
git add core/crates/liveblock-config/src/settings.rs
git commit -m "feat(config): add DetectionSettings + SettingsStore"
```

---

### Task 4: Re-export `liveblock-config` through `liveblock-core`

**Files:**
- Modify: `core/crates/liveblock-core/Cargo.toml`, `core/crates/liveblock-core/src/lib.rs`

**Interfaces:**
- Produces: `liveblock_core::config` re-export; `liveblock_core::CoordinatorConfig` resolves to the config crate's type.

- [ ] **Step 1: Add dep** in `liveblock-core/Cargo.toml`: `liveblock-config = { path = "../liveblock-config" }`.

- [ ] **Step 2: Write failing test** in `liveblock-core/src/lib.rs`:
```rust
#[test]
fn reexports_config() {
    let v = liveblock_core::config::Vocabulary { version: 1, classes: vec![] };
    assert_eq!(v.version, 1);
}
```

- [ ] **Step 3: Run, verify fail.** `cargo test --manifest-path core/Cargo.toml -p liveblock-core reexports_config` → FAIL.

- [ ] **Step 4: Implement.** Add near the existing re-exports (`lib.rs:16-21`): `pub use liveblock_config as config;`. If `liveblock-core` defines its own `CoordinatorConfig`, replace it with `pub use liveblock_config::settings::CoordinatorConfig;` and fix references in `Coordinator`. Ensure `cargo build` workspace-wide still passes.

- [ ] **Step 5: Run, verify pass + whole workspace builds.** `cargo test --manifest-path core/Cargo.toml --workspace` → PASS.

- [ ] **Step 6: Commit.**
```bash
git add core/crates/liveblock-core/
git commit -m "feat(core): re-export liveblock-config"
```

---

### Task 5: Bridge `VocabularyHandle` to Swift

**Files:**
- Modify: `core/crates/liveblock-bridge/Cargo.toml`, `core/crates/liveblock-bridge/src/lib.rs`

**Interfaces:**
- Produces (FFI, mirrors `RegionStoreHandle`): `VocabularyHandle::new()`, `vocabulary_open(path)`, `set_vocabulary(json) -> bool`, `get_detector_classes() -> String` (`[{id,name,enabled,threshold}]`), `set_class_enabled(class_id,enabled) -> bool`, `set_class_threshold(class_id,threshold) -> bool`, `to_json() -> String`.

- [ ] **Step 1: Add dep** in `bridge/Cargo.toml`: `liveblock-config = { path = "../liveblock-config" }`.

- [ ] **Step 2: Write failing Rust test** in `bridge/src/lib.rs` (test the inner impl, not the FFI shim):
```rust
#[test]
fn vocab_handle_get_classes_json() {
    let h = VocabularyHandle::new();
    assert!(h.set_vocabulary(r#"{"version":1,"classes":[{"id":0,"name":"Logo","prompts":["logo"]}]}"#.to_string()));
    let classes = h.get_detector_classes();
    assert!(classes.contains("\"name\": \"Logo\""));
    assert!(classes.contains("\"enabled\": true"));
}
```

- [ ] **Step 3: Run, verify fail.** `cargo test --manifest-path core/Cargo.toml -p liveblock-bridge vocab_handle` → FAIL.

- [ ] **Step 4: Implement** a `VocabularyHandle` struct wrapping `Mutex<(Vocabulary, SettingsStore)>`, declare it inside the existing `#[swift_bridge::bridge] mod ffi` block (next to `RegionStoreHandle`), with the signatures above. Transport all rich data as JSON strings (swift-bridge 0.1 limitation), serialized with the 2-space sorted helper. `get_detector_classes` joins vocab names with settings enable/threshold.

- [ ] **Step 5: Run, verify pass.** `cargo test --manifest-path core/Cargo.toml -p liveblock-bridge vocab_handle` → PASS.

- [ ] **Step 6: Regenerate Swift glue + confirm it builds.** Run: `LB_BRIDGE_HOST_ONLY=1 tools/build_bridge.sh`. Expected: regenerates `core/crates/liveblock-bridge/generated/LiveBlockBridge/LiveBlockBridge.{swift,h}` including `VocabularyHandle`; exit 0.

- [ ] **Step 7: Commit.**
```bash
git add core/crates/liveblock-bridge/
git commit -m "feat(bridge): expose VocabularyHandle to Swift"
```

---

## Component B — offline open-vocab model build

### Task 6: Pin a Python 3.12 venv + ML deps

**Files:**
- Modify: `tools/setup_env.sh`, `tools/requirements.txt`

- [ ] **Step 1:** In `tools/requirements.txt` add (keep existing): `onnx>=1.16`, `onnxruntime>=1.18`, `onnxslim>=0.1.34`, `ftfy>=6.1`, `regex>=2024.5`. (ultralytics already pulls CLIP for `set_classes`.)

- [ ] **Step 2:** In `tools/setup_env.sh`, prefer a 3.10–3.12 interpreter: try `python3.12`/`python3.11`/`python3.10` (and `uv python install 3.12` if `uv` exists) before falling back; fail loudly if only 3.13 is available, printing the exact `uv`/`pyenv` command to get 3.12.

- [ ] **Step 3: Verify.** Run: `bash tools/setup_env.sh` → creates `tools/.venv` on a 3.10–3.12 interpreter, `pip install` succeeds. Expected: `tools/.venv/bin/python --version` prints 3.10–3.12. *(Environment-heavy; if the sandbox cannot install torch/coremltools, record the blocker and continue — Tasks 7 + 8's logic is still unit-testable.)*

- [ ] **Step 4: Commit.**
```bash
git add tools/setup_env.sh tools/requirements.txt
git commit -m "build(tools): pin py3.12 venv + add onnx/clip deps"
```

---

### Task 7: Default vocabulary JSON (shared by crate + tool)

**Files:**
- Create: `tools/vocab/liveblock-vocab.json`

- [ ] **Step 1: Write it** (must parse as `Vocabulary`; this is the §7 starting point, later tuned by eval):
```json
{
  "classes": [
    { "id": 0, "name": "Logo", "prompts": ["logo", "brand logo", "company logo"] },
    { "id": 1, "name": "Ad banner", "prompts": ["advertisement", "banner advertisement", "advertising banner"] },
    { "id": 2, "name": "Sponsored", "prompts": ["sponsored content", "sponsored post", "promotional banner"] }
  ],
  "version": 1
}
```

- [ ] **Step 2: Add a crate test** in `core/crates/liveblock-config/src/vocabulary.rs` that parses the checked-in file (path via `CARGO_MANIFEST_DIR`-relative `../../../tools/vocab/liveblock-vocab.json`) and asserts 3 classes + version 1. Run `cargo test --manifest-path core/Cargo.toml -p liveblock-config default_vocab_parses` → PASS.

- [ ] **Step 3: Commit.**
```bash
git add tools/vocab/liveblock-vocab.json core/crates/liveblock-config/src/vocabulary.rs
git commit -m "feat(vocab): default open-vocab list + crate parse test"
```

---

### Task 8: `tools/build_openvocab.py` — bake vocab into YOLO-World-v2

**Files:**
- Create: `tools/build_openvocab.py`, `tools/test_build_openvocab.py`

**Interfaces:**
- Produces: `load_vocab(path) -> list[str]` (flattens prompts), `build(base="yolov8s-worldv2.pt", vocab_path, out="liveblock-detector.pt") -> Path`.

- [ ] **Step 1: Write failing pytest** `tools/test_build_openvocab.py` for the pure logic (no model download):
```python
from build_openvocab import load_vocab
def test_load_vocab_flattens_prompts(tmp_path):
    p = tmp_path / "v.json"
    p.write_text('{"version":1,"classes":[{"id":0,"name":"Logo","prompts":["logo","brand logo"]},{"id":1,"name":"Ad","prompts":["advertisement"]}]}')
    assert load_vocab(str(p)) == ["logo", "brand logo", "advertisement"]
```

- [ ] **Step 2: Run, verify fail.** Run: `cd tools && ../tools/.venv/bin/python -m pytest test_build_openvocab.py -q` → FAIL (module missing).

- [ ] **Step 3: Implement `build_openvocab.py`:**
```python
import json, argparse
from pathlib import Path

def load_vocab(path: str) -> list[str]:
    data = json.loads(Path(path).read_text())
    prompts: list[str] = []
    for c in data["classes"]:
        prompts.extend(c["prompts"])
    return prompts

def build(base="yolov8s-worldv2.pt", vocab_path="tools/vocab/liveblock-vocab.json",
          out="liveblock-detector.pt"):
    from ultralytics import YOLOWorld
    prompts = load_vocab(vocab_path)
    model = YOLOWorld(base)
    model.set_classes(prompts)      # bakes CLIP text embeddings into the head
    model.save(out)
    return Path(out)

if __name__ == "__main__":
    ap = argparse.ArgumentParser()
    ap.add_argument("--base", default="yolov8s-worldv2.pt")
    ap.add_argument("--vocab", default="tools/vocab/liveblock-vocab.json")
    ap.add_argument("--out", default="liveblock-detector.pt")
    a = ap.parse_args()
    print(build(a.base, a.vocab, a.out))
```

- [ ] **Step 4: Run, verify pass.** Run: `cd tools && ../tools/.venv/bin/python -m pytest test_build_openvocab.py -q` → PASS.

- [ ] **Step 5: Commit.**
```bash
git add tools/build_openvocab.py tools/test_build_openvocab.py
git commit -m "feat(tools): build_openvocab bakes vocab into YOLO-World-v2"
```

---

### Task 9: Extend exporter — rename target + ONNX branch

**Files:**
- Modify: `tools/export_to_coreml.py`

**Interfaces:**
- Produces: `export(pt_path, fmt="coreml"|"onnx", int8=True, nms=True) -> Path`; `install_into_repo` targets `Sources/liveblock-detector.mlpackage`.

- [ ] **Step 1:** Change `TARGET_PATH` / install destination from `Sources/yolov8n.mlpackage` to `Sources/liveblock-detector.mlpackage` (back up any existing `.bak`).

- [ ] **Step 2:** Add a `--format {coreml,onnx}` arg; for `onnx` call `model.export(format="onnx", nms=True, imgsz=640, opset=13)` (raise opset only if onnxruntime rejects 13); write `liveblock-detector.onnx`.

- [ ] **Step 3: Verify arg parsing** with a quick unit check (mock `YOLO`): `../tools/.venv/bin/python -c "import export_to_coreml; print('ok')"` → `ok`.

- [ ] **Step 4: Commit.**
```bash
git add tools/export_to_coreml.py
git commit -m "feat(tools): export liveblock-detector to coreml + onnx"
```

---

### Task 10: Produce the real model artifacts (run-once, environment-heavy)

**Files:**
- Create (build output): `Sources/liveblock-detector.mlpackage`, `models/liveblock-detector.onnx`, `tools/vocab/liveblock-embeddings.bin`

- [ ] **Step 1:** `cd <repo> && tools/.venv/bin/python tools/build_openvocab.py` → `liveblock-detector.pt`.
- [ ] **Step 2:** `tools/.venv/bin/python tools/export_to_coreml.py liveblock-detector.pt --format coreml` and `--format onnx`.
- [ ] **Step 3: Validate the `.mlpackage` loads + detects** on a fixture PNG via a 10-line Vision smoke script; confirm it returns the vocab class names (not COCO). Expected: at least one detection on an image containing an obvious logo, label ∈ {Logo, Ad banner, Sponsored}.
- [ ] **Step 4:** If CoreML NMS export fails for the world model, record it in the plan's notes and fall back per spec §9 (try YOLOE base; else defer raw-decode to Route B). Do **not** silently ship a COCO model.
- [ ] **Step 5: Commit** the artifacts (or document why they're git-ignored due to size; if ignored, add a `make model` target).
```bash
git add Sources/liveblock-detector.mlpackage models/liveblock-detector.onnx tools/vocab/liveblock-embeddings.bin
git commit -m "feat(model): baked open-vocab detector artifacts"
```

---

## Component C — eval harness

### Task 11: `tools/eval/` accuracy + regression harness

**Files:**
- Create: `tools/eval/run_eval.py`, `tools/eval/test_eval.py`, `tools/eval/README.md`, `tools/eval/fixtures/` (a few labeled logo/ad images + hard negatives)

**Interfaces:**
- Produces: `evaluate(model_path, fixtures_dir) -> {"precision":float,"recall":float,"per_class":{...}}`, `iou(box_a, box_b) -> float`.

- [ ] **Step 1: Write failing test** `tools/eval/test_eval.py` for the metric math (pure, no model):
```python
from run_eval import iou
def test_iou_half_overlap():
    a = (0,0,10,10); b = (5,0,10,10)  # x,y,w,h
    assert abs(iou(a,b) - (50/150)) < 1e-6
```
- [ ] **Step 2: Run, verify fail.** `cd tools/eval && ../.venv/bin/python -m pytest test_eval.py -q` → FAIL.
- [ ] **Step 3: Implement** `iou` + `evaluate` (load model, run over `fixtures/`, greedy-match preds↔truth at IoU≥0.5, compute precision/recall overall + per class; hard negatives count false positives). Add a `--assert-recall FLOAT` flag that exits non-zero below the floor (start `0.30`, raise later).
- [ ] **Step 4: Run, verify pass.** `cd tools/eval && ../.venv/bin/python -m pytest test_eval.py -q` → PASS.
- [ ] **Step 5: Record a baseline** number against the Task-10 model into `tools/eval/README.md`. Use it to drop weak prompts / tune thresholds (write chosen thresholds back into `tools/vocab/liveblock-vocab.json` + default `DetectionSettings`).
- [ ] **Step 6: Commit.**
```bash
git add tools/eval/
git commit -m "feat(eval): precision/recall harness + baseline"
```

---

## Component D — per-platform wiring + verification

### Task 12: macOS — load `liveblock-detector` + read vocab/thresholds from config

**Files:**
- Modify: `Sources/VisionProcessor.swift`, `Sources/TrainingController.swift`

**Interfaces:**
- Consumes: `VocabularyHandle` (Task 5), `liveblock-detector.mlpackage` (Task 10).

- [ ] **Step 1:** In `VisionProcessor.loadModel()` (`:189-211`) replace both `yolov8n` lookups (runtime dir + bundle) with `liveblock-detector`. In `TrainingController.installFreshlyTrainedModel()` (~`:278-302`) rename the hot-reload copy source/dest to `liveblock-detector.mlpackage`.

- [ ] **Step 2:** Add a `VocabularyHandle` (open at `~/Library/Application Support/LiveBlock/detection-settings.json`, seed from the bundled `liveblock-vocab.json` on first run). Replace the single `_minimumConfidence` filter (`:84`) with a per-class effective threshold lookup keyed on `obs.labels.first.identifier` → class id. Disabled classes are dropped.

- [ ] **Step 3: Build, no launch.** Run: `./run.sh --build` → exit 0.

- [ ] **Step 4: Swift tests.** Run: `./run.sh --test` → existing `RegionStoreTests` + `WindowRetentionTests` green; add one test asserting a disabled class id is filtered out.

- [ ] **Step 5: Commit.**
```bash
git add Sources/VisionProcessor.swift Sources/TrainingController.swift Tests/
git commit -m "feat(macos): load liveblock-detector + per-class thresholds from config"
```

---

### Task 13: Windows — class names from shared config

**Files:**
- Modify: `platform/windows/src-tauri/src/detection.rs`

- [ ] **Step 1:** Replace `coco_class_names()` (`:219-233`) usage in `load()` (`:63`) with names parsed from the bundled `liveblock-vocab.json` (via `liveblock-config`); add `liveblock-config` to that crate's deps. Swap the model path to `liveblock-detector.onnx`.
- [ ] **Step 2: Compile-check** (host can `cargo check` the crate even if not the full Tauri app): `cargo check --manifest-path platform/windows/src-tauri/Cargo.toml` → if Windows-only deps block this on macOS, record that and rely on CI. Do not claim Windows runtime works.
- [ ] **Step 3: Commit.**
```bash
git add platform/windows/src-tauri/src/detection.rs platform/windows/src-tauri/Cargo.toml
git commit -m "feat(windows): detector class names from shared config"
```

---

### Task 14: Linux — class names from shared config

**Files:**
- Modify: `platform/linux/src-tauri/src/detection.rs`

- [ ] **Step 1:** Add class names from `liveblock-config` (currently Linux has only `class_id`); attach to `DetBox` or a lookup. Swap model path to `liveblock-detector.onnx`. Decode/NMS unchanged (already shared).
- [ ] **Step 2: Compile-check** what the macOS host can: `cargo check --manifest-path platform/linux/src-tauri/Cargo.toml` (record if blocked by Linux-only deps; rely on CI). Do not claim Linux runtime works.
- [ ] **Step 3: Commit.**
```bash
git add platform/linux/src-tauri/src/detection.rs platform/linux/src-tauri/Cargo.toml
git commit -m "feat(linux): detector class names from shared config"
```

---

### Task 15: Clear footguns + full build/test sweep

**Files:**
- Delete: `platform/_shared-frontend/dist/* 2.html`

- [ ] **Step 1:** `find platform/_shared-frontend -name '* 2.*' -print -delete`.
- [ ] **Step 2:** Confirm no `LiveBlock *.xcodeproj` duplicate exists.
- [ ] **Step 3: Full sweep:** `cargo test --manifest-path core/Cargo.toml --workspace` → green; `LB_BRIDGE_HOST_ONLY=1 tools/build_bridge.sh` → exit 0; `./run.sh --build` → exit 0.
- [ ] **Step 4: Commit.**
```bash
git add -A
git commit -m "chore: remove dist duplicates; green build sweep"
```

---

### Task 16: Live verification on this Mac

- [ ] **Step 1:** `tools/setup_codesign_identity.sh` (once, if not already) so TCC grants persist.
- [ ] **Step 2:** `./run.sh` → launch; grant Screen Recording.
- [ ] **Step 3:** Put an obvious logo/ad on screen (a website with a brand banner). Click Start; confirm the region is **detected and inpainted** with a vocab label (Logo/Ad banner/Sponsored), not a COCO label. Capture what actually happened (works / partially / not) — report honestly per the repo house rule.
- [ ] **Step 4:** If detection is weak, iterate vocabulary/thresholds via the eval harness (Task 11) and re-run. This is the slice's real success gate.
- [ ] **Step 5: Final commit + open PR** (do not merge without the user).
```bash
git commit -am "feat: open-vocab detector slice 1 verified on macOS"
```

---

## Self-Review

**Spec coverage:** Component 1 (config crate) → Tasks 1–5; Component 2 (offline build) → Tasks 6–10; Component 3 (per-platform wiring) → Tasks 12–14; Component 4 (eval) → Task 11; model rename → Tasks 9, 12; verification plan → Tasks 15–16; footguns → Task 15. Route B explicitly deferred (spec §13). All spec §11 success criteria mapped.

**Placeholder scan:** Task 3 step 3 and Task 13/14 describe pattern-following implementations rather than full code — acceptable because they mirror an exact existing file cited by path (`RegionStore` atomic-write; the existing detection.rs decode), and full novel code would duplicate that file. All *new* logic (types, metrics, vocab loading, bridge) has complete code.

**Type consistency:** `Vocabulary`/`VocabClass`/`ClassEmbeddings`/`ClassRule`/`DetectionSettings`/`SettingsStore`/`CoordinatorConfig` names are used identically across Tasks 1–5, 12–14. `load_vocab`/`build`/`export`/`evaluate`/`iou` consistent across Tasks 8–11. Model name `liveblock-detector` consistent across Tasks 9, 10, 12, 13, 14.

**Known environment risk:** Tasks 6, 10, 16 require heavy ML installs / a GUI run that may exceed sandbox limits; each says to record the blocker and continue rather than fake success. Pure-logic tasks (1–5, 7, 8 logic, 9 parse, 11 math) are fully runnable offline.
