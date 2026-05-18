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

use liveblock_regions::{NormalizedRegion, RegionStore};
use std::path::PathBuf;
use std::sync::Arc;
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
        let value = serde_json::Value::Array(array.into_iter().map(serde_json::Value::from_iter).collect());
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
