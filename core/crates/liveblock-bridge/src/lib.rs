//! Swift / C-ABI bridge for the LiveBlock core engine.
//!
//! Exposes a minimal opaque-handle API around `liveblock_regions::RegionStore`
//! so the macOS Swift target can drive the same store the Tauri/Linux/Windows
//! targets use. Generated headers + Swift glue land in
//! `core/crates/liveblock-bridge/generated/` after `cargo build`.
//!
//! Compatibility note: the underlying `RegionStore` writes JSON with sorted
//! keys + 2-space pretty indentation, so files written by Swift's
//! `RegionStore.swift` and by the bridge are byte-identical.

// swift-bridge 0.1's generated glue performs same-type raw-pointer casts.
// They are harmless and outside this crate's handwritten code.
#![allow(clippy::unnecessary_cast)]

use liveblock_config::{SettingsStore, Vocabulary};
use liveblock_regions::{NormalizedRegion, RegionStore};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use uuid::Uuid;

#[swift_bridge::bridge]
mod ffi {
    extern "Rust" {
        type RegionStoreHandle;

        #[swift_bridge(init)]
        fn new() -> RegionStoreHandle;

        fn region_store_open(path: String) -> RegionStoreHandle;

        fn count(self: &RegionStoreHandle) -> u64;

        fn add(self: &RegionStoreHandle, x: f64, y: f64, width: f64, height: f64) -> String;

        fn add_with_id(
            self: &RegionStoreHandle,
            id: String,
            x: f64,
            y: f64,
            width: f64,
            height: f64,
        ) -> bool;

        fn replace_id(
            self: &RegionStoreHandle,
            id: String,
            x: f64,
            y: f64,
            width: f64,
            height: f64,
        ) -> bool;

        fn remove(self: &RegionStoreHandle, id: String) -> bool;

        fn clear(self: &RegionStoreHandle) -> bool;

        fn to_json(self: &RegionStoreHandle) -> String;
    }

    extern "Rust" {
        type VocabularyHandle;

        #[swift_bridge(init)]
        fn new() -> VocabularyHandle;

        fn vocabulary_open(path: String) -> VocabularyHandle;

        fn set_vocabulary(self: &VocabularyHandle, json: String) -> bool;

        fn get_detector_classes(self: &VocabularyHandle) -> String;

        fn set_class_enabled(self: &VocabularyHandle, class_id: u32, enabled: bool) -> bool;

        fn set_class_threshold(self: &VocabularyHandle, class_id: u32, threshold: f32) -> bool;

        fn to_json(self: &VocabularyHandle) -> String;
    }
}

pub struct RegionStoreHandle {
    inner: Arc<RegionStore>,
}

impl RegionStoreHandle {
    fn new() -> Self {
        Self {
            inner: Arc::new(RegionStore::in_memory()),
        }
    }

    fn count(&self) -> u64 {
        self.inner.current().len() as u64
    }

    fn add(&self, x: f64, y: f64, width: f64, height: f64) -> String {
        let region = NormalizedRegion::new(x, y, width, height);
        let id = region.id;
        if self.inner.add(region).is_err() {
            return String::new();
        }
        id.to_string()
    }

    fn add_with_id(&self, id: String, x: f64, y: f64, width: f64, height: f64) -> bool {
        let Ok(uuid) = Uuid::parse_str(&id) else {
            return false;
        };
        let region = NormalizedRegion::with_id(uuid, x, y, width, height);
        self.inner.add(region).is_ok()
    }

    fn replace_id(&self, id: String, x: f64, y: f64, width: f64, height: f64) -> bool {
        let Ok(uuid) = Uuid::parse_str(&id) else {
            return false;
        };
        let region = NormalizedRegion::with_id(uuid, x, y, width, height);
        self.inner.replace_id(uuid, region).is_ok()
    }

    fn remove(&self, id: String) -> bool {
        let Ok(uuid) = Uuid::parse_str(&id) else {
            return false;
        };
        self.inner.remove(uuid).is_ok()
    }

    fn clear(&self) -> bool {
        self.inner.clear().is_ok()
    }

    fn to_json(&self) -> String {
        let regions = self.inner.current();
        // Match the on-disk format: sorted keys, uppercase UUID, 2-space pretty.
        let array: Vec<std::collections::BTreeMap<&'static str, serde_json::Value>> = regions
            .iter()
            .map(|r| {
                let mut m: std::collections::BTreeMap<&'static str, serde_json::Value> =
                    std::collections::BTreeMap::new();
                m.insert(
                    "id",
                    serde_json::Value::String(r.id.to_string().to_uppercase()),
                );
                m.insert("x", serde_json::json!(r.x));
                m.insert("y", serde_json::json!(r.y));
                m.insert("width", serde_json::json!(r.width));
                m.insert("height", serde_json::json!(r.height));
                m
            })
            .collect();
        let value = serde_json::Value::Array(
            array
                .into_iter()
                .map(serde_json::Value::from_iter)
                .collect(),
        );
        let mut buf = Vec::new();
        let formatter = serde_json::ser::PrettyFormatter::with_indent(b"  ");
        let mut ser = serde_json::Serializer::with_formatter(&mut buf, formatter);
        if serde::Serialize::serialize(&value, &mut ser).is_err() {
            return String::new();
        }
        String::from_utf8(buf).unwrap_or_default()
    }
}

fn region_store_open(path: String) -> RegionStoreHandle {
    let store = RegionStore::open(PathBuf::from(path)).unwrap_or_else(|_| RegionStore::in_memory());
    RegionStoreHandle {
        inner: Arc::new(store),
    }
}

/// Opaque handle exposing the shared open-vocabulary config to Swift.
///
/// Wraps a `Mutex<(Vocabulary, SettingsStore)>`: the vocabulary supplies class
/// ids/names while the settings store supplies per-class enable flags + score
/// thresholds. swift-bridge 0.1 can't marshal rich structs, so every read
/// crosses the FFI boundary as a JSON string serialized with the same 2-space,
/// sorted-key formatter used on disk (byte-compatible with Swift's
/// `JSONEncoder([.prettyPrinted, .sortedKeys])`).
pub struct VocabularyHandle {
    inner: Mutex<(Vocabulary, SettingsStore)>,
}

/// One row of `get_detector_classes`. Fields are declared in alphabetical order
/// so direct serialization yields sorted keys and the `f32` threshold keeps its
/// shortest decimal form (e.g. `0.6`, not `0.6000000238418579`).
#[derive(serde::Serialize)]
struct DetectorClass {
    enabled: bool,
    id: u32,
    name: String,
    threshold: f32,
}

impl VocabularyHandle {
    fn new() -> Self {
        Self {
            inner: Mutex::new((
                Vocabulary {
                    version: 1,
                    classes: Vec::new(),
                },
                SettingsStore::in_memory(),
            )),
        }
    }

    fn set_vocabulary(&self, json: String) -> bool {
        let Ok(vocab) = Vocabulary::from_json(&json) else {
            return false;
        };
        let mut guard = self.inner.lock().expect("vocabulary mutex poisoned");
        guard.0 = vocab;
        true
    }

    fn get_detector_classes(&self) -> String {
        let guard = self.inner.lock().expect("vocabulary mutex poisoned");
        let (vocab, store) = &*guard;
        let settings = store.current();
        let rows: Vec<DetectorClass> = vocab
            .classes
            .iter()
            .map(|c| {
                let enabled = settings
                    .classes
                    .iter()
                    .find(|r| r.class_id == c.id)
                    .map(|r| r.enabled)
                    .unwrap_or(true);
                DetectorClass {
                    enabled,
                    id: c.id,
                    name: c.name.clone(),
                    threshold: settings.effective_threshold(c.id),
                }
            })
            .collect();
        // Serialize the struct slice DIRECTLY (not through serde_json::Value) so
        // each f32 keeps its shortest decimal form, matching Swift byte-for-byte.
        let mut buf = Vec::new();
        let fmt = serde_json::ser::PrettyFormatter::with_indent(b"  ");
        let mut ser = serde_json::Serializer::with_formatter(&mut buf, fmt);
        if serde::Serialize::serialize(&rows, &mut ser).is_err() {
            return String::new();
        }
        String::from_utf8(buf).unwrap_or_default()
    }

    fn set_class_enabled(&self, class_id: u32, enabled: bool) -> bool {
        let guard = self.inner.lock().expect("vocabulary mutex poisoned");
        let store = &guard.1;
        if store.set_class_enabled(class_id, enabled).is_err() {
            return false;
        }
        store.persist().is_ok()
    }

    fn set_class_threshold(&self, class_id: u32, threshold: f32) -> bool {
        let guard = self.inner.lock().expect("vocabulary mutex poisoned");
        let store = &guard.1;
        if store
            .set_class_threshold(class_id, Some(threshold))
            .is_err()
        {
            return false;
        }
        store.persist().is_ok()
    }

    fn to_json(&self) -> String {
        let guard = self.inner.lock().expect("vocabulary mutex poisoned");
        guard.0.to_json()
    }
}

fn vocabulary_open(path: String) -> VocabularyHandle {
    let store =
        SettingsStore::open(PathBuf::from(path)).unwrap_or_else(|_| SettingsStore::in_memory());
    VocabularyHandle {
        inner: Mutex::new((
            Vocabulary {
                version: 1,
                classes: Vec::new(),
            },
            store,
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn add_then_count() {
        let store = RegionStoreHandle::new();
        let _ = store.add(0.1, 0.1, 0.2, 0.2);
        let _ = store.add(0.4, 0.4, 0.2, 0.2);
        assert_eq!(store.count(), 2);
    }

    #[test]
    fn remove_round_trip() {
        let store = RegionStoreHandle::new();
        let id = store.add(0.0, 0.0, 0.5, 0.5);
        assert!(!id.is_empty());
        assert!(store.remove(id));
        assert_eq!(store.count(), 0);
    }

    #[test]
    fn add_with_id_preserves_uuid() {
        let store = RegionStoreHandle::new();
        let id = "11111111-2222-3333-4444-555555555555".to_string();
        assert!(store.add_with_id(id.clone(), 0.1, 0.1, 0.2, 0.2));
        let s = store.to_json();
        assert!(s.contains("11111111-2222-3333-4444-555555555555"));
    }

    #[test]
    fn replace_id_updates_in_place() {
        let store = RegionStoreHandle::new();
        let id = store.add(0.1, 0.1, 0.1, 0.1);
        assert!(store.replace_id(id.clone(), 0.5, 0.5, 0.4, 0.4));
        assert_eq!(store.count(), 1);
        let s = store.to_json();
        assert!(s.contains("0.5"));
    }

    #[test]
    fn clear_empties_store() {
        let store = RegionStoreHandle::new();
        let _ = store.add(0.1, 0.1, 0.2, 0.2);
        let _ = store.add(0.3, 0.3, 0.2, 0.2);
        assert!(store.clear());
        assert_eq!(store.count(), 0);
    }

    #[test]
    fn vocab_handle_get_classes_json() {
        let h = VocabularyHandle::new();
        assert!(h.set_vocabulary(
            r#"{"version":1,"classes":[{"id":0,"name":"Logo","prompts":["logo"]}]}"#.to_string()
        ));
        let classes = h.get_detector_classes();
        assert!(classes.contains("\"name\": \"Logo\""));
        assert!(classes.contains("\"enabled\": true"));
    }

    #[test]
    fn to_json_keys_are_sorted() {
        let store = RegionStoreHandle::new();
        let _ = store.add(0.25, 0.25, 0.5, 0.5);
        let s = store.to_json();
        let h = s.find("\"height\"").unwrap();
        let i = s.find("\"id\"").unwrap();
        let w = s.find("\"width\"").unwrap();
        let x = s.find("\"x\"").unwrap();
        let y = s.find("\"y\"").unwrap();
        assert!(h < i && i < w && w < x && x < y, "keys must be sorted: {s}");
    }
}
