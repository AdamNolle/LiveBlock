//! Normalized regions for LiveBlock.
//!
//! Pure-Rust port of `Sources/RegionStore.swift`. The on-disk JSON format is
//! byte-compatible with the Swift version: pretty-printed with 2-space
//! indentation and sorted keys, so existing user data on macOS keeps working.

use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use thiserror::Error;
use uuid::Uuid;

#[derive(Debug, Error)]
pub enum RegionError {
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("json error: {0}")]
    Json(#[from] serde_json::Error),
}

/// A rectangle in normalized [0..1] coordinates with origin at top-left.
///
/// Coordinates survive window resize, display change, and DPI change. All
/// fields are clamped on construction so `x + width <= 1` and `y + height <= 1`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct NormalizedRegion {
    pub id: Uuid,
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

impl NormalizedRegion {
    /// Create a new region, clamping all fields into the legal range.
    pub fn new(x: f64, y: f64, width: f64, height: f64) -> Self {
        Self::with_id(Uuid::new_v4(), x, y, width, height)
    }

    /// Create a region with an explicit id, clamping all fields.
    pub fn with_id(id: Uuid, x: f64, y: f64, width: f64, height: f64) -> Self {
        let cx = x.clamp(0.0, 1.0);
        let cy = y.clamp(0.0, 1.0);
        let cw = width.clamp(0.0, 1.0 - cx);
        let ch = height.clamp(0.0, 1.0 - cy);
        Self {
            id,
            x: cx,
            y: cy,
            width: cw,
            height: ch,
        }
    }

    /// Top-left-origin pixel rectangle for the given canvas size.
    /// Returns `(x, y, width, height)` in pixels.
    pub fn rect_in(&self, width: f64, height: f64) -> (f64, f64, f64, f64) {
        (
            self.x * width,
            self.y * height,
            self.width * width,
            self.height * height,
        )
    }

    /// Bottom-left-origin (CoreVideo / pixel buffer) pixel rectangle.
    pub fn cv_rect_in(&self, width: f64, height: f64) -> (f64, f64, f64, f64) {
        let (rx, ry, rw, rh) = self.rect_in(width, height);
        (rx, height - (ry + rh), rw, rh)
    }
}

/// Thread-safe persistent store of user-drawn regions.
///
/// On disk the regions are written as a JSON array, pretty-printed with 2-space
/// indentation and sorted keys, matching `JSONEncoder.outputFormatting =
/// [.prettyPrinted, .sortedKeys]` in the Swift implementation.
#[derive(Debug)]
pub struct RegionStore {
    inner: Mutex<Vec<NormalizedRegion>>,
    path: Option<PathBuf>,
}

impl RegionStore {
    /// In-memory store with no on-disk persistence.
    pub fn in_memory() -> Self {
        Self {
            inner: Mutex::new(Vec::new()),
            path: None,
        }
    }

    /// Open or create a store backed by the given JSON file. If the file
    /// exists and parses, its contents are loaded; otherwise the store starts
    /// empty.
    pub fn open<P: AsRef<Path>>(path: P) -> Result<Self, RegionError> {
        let path = path.as_ref().to_path_buf();
        let regions = match fs::read(&path) {
            Ok(bytes) if bytes.is_empty() => Vec::new(),
            Ok(bytes) => {
                // Surface parse errors instead of silently nuking user data.
                // If the file is malformed (truncated mid-write, manual edit,
                // version skew), the caller decides whether to back up or abort.
                serde_json::from_slice::<Vec<NormalizedRegion>>(&bytes)
                    .map_err(RegionError::Json)?
            }
            Err(ref e) if e.kind() == std::io::ErrorKind::NotFound => Vec::new(),
            Err(e) => return Err(RegionError::Io(e)),
        };
        Ok(Self {
            inner: Mutex::new(regions),
            path: Some(path),
        })
    }

    /// Snapshot of the current regions.
    pub fn current(&self) -> Vec<NormalizedRegion> {
        self.inner.lock().clone()
    }

    pub fn add(&self, region: NormalizedRegion) -> Result<(), RegionError> {
        let snapshot = {
            let mut g = self.inner.lock();
            g.push(region);
            g.clone()
        };
        self.persist(&snapshot)
    }

    pub fn remove(&self, id: Uuid) -> Result<(), RegionError> {
        let snapshot = {
            let mut g = self.inner.lock();
            g.retain(|r| r.id != id);
            g.clone()
        };
        self.persist(&snapshot)
    }

    pub fn replace(&self, new_regions: Vec<NormalizedRegion>) -> Result<(), RegionError> {
        let snapshot = {
            let mut g = self.inner.lock();
            *g = new_regions;
            g.clone()
        };
        self.persist(&snapshot)
    }

    /// Replace a single region by id, preserving its id.
    pub fn replace_id(&self, id: Uuid, new_region: NormalizedRegion) -> Result<(), RegionError> {
        let snapshot = {
            let mut g = self.inner.lock();
            if let Some(idx) = g.iter().position(|r| r.id == id) {
                g[idx] = NormalizedRegion::with_id(
                    id,
                    new_region.x,
                    new_region.y,
                    new_region.width,
                    new_region.height,
                );
            }
            g.clone()
        };
        self.persist(&snapshot)
    }

    pub fn clear(&self) -> Result<(), RegionError> {
        self.replace(Vec::new())
    }

    fn persist(&self, regions: &[NormalizedRegion]) -> Result<(), RegionError> {
        let Some(path) = &self.path else {
            return Ok(());
        };
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }

        // Encode each region through a BTreeMap so keys come out sorted, matching
        // Swift's JSONEncoder with `.sortedKeys`.
        let array: Vec<BTreeMap<&'static str, serde_json::Value>> = regions
            .iter()
            .map(|r| {
                let mut m: BTreeMap<&'static str, serde_json::Value> = BTreeMap::new();
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

        let bytes = pretty_two_space(&serde_json::Value::Array(
            array
                .into_iter()
                .map(serde_json::Value::from_iter)
                .collect(),
        ))?;

        // Atomic write via a unique tmp file (process+nanos suffix) so two
        // writers can't clobber each other's tmp. The tmp lives in the same
        // directory as the target so the rename is atomic on the same fs.
        let stem = path
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or("regions.json");
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.subsec_nanos())
            .unwrap_or(0);
        let pid = std::process::id();
        let tmp = path.with_file_name(format!("{stem}.{pid}.{nanos}.tmp"));
        {
            let mut f = fs::File::create(&tmp)?;
            f.write_all(&bytes)?;
            f.sync_all()?;
        }
        fs::rename(&tmp, path)?;
        Ok(())
    }
}

/// Serialize a JSON value with 2-space indentation, matching Swift's default
/// pretty-printer.
fn pretty_two_space(value: &serde_json::Value) -> Result<Vec<u8>, RegionError> {
    let mut buf = Vec::new();
    let formatter = serde_json::ser::PrettyFormatter::with_indent(b"  ");
    let mut ser = serde_json::Serializer::with_formatter(&mut buf, formatter);
    serde::Serialize::serialize(value, &mut ser)?;
    Ok(buf)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn clamps_out_of_range_inputs() {
        let r = NormalizedRegion::new(-0.1, 1.5, 2.0, 0.4);
        assert!(r.x >= 0.0 && r.x <= 1.0);
        assert!(r.y >= 0.0 && r.y <= 1.0);
        assert!(r.x + r.width <= 1.0 + 1e-12);
        assert!(r.y + r.height <= 1.0 + 1e-12);
    }

    #[test]
    fn rect_in_scales_correctly() {
        let r = NormalizedRegion::with_id(Uuid::nil(), 0.25, 0.5, 0.5, 0.25);
        let (x, y, w, h) = r.rect_in(100.0, 200.0);
        assert_eq!((x, y, w, h), (25.0, 100.0, 50.0, 50.0));
    }

    #[test]
    fn cv_rect_flips_y_axis() {
        let r = NormalizedRegion::with_id(Uuid::nil(), 0.0, 0.0, 0.5, 0.25);
        let (_x, y, _w, h) = r.cv_rect_in(100.0, 200.0);
        // top-left rect would have y=0, h=50; flipped: y = 200 - (0 + 50) = 150
        assert_eq!(y, 150.0);
        assert_eq!(h, 50.0);
    }

    #[test]
    fn store_round_trips_through_disk() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("regions.json");

        let store = RegionStore::open(&path).unwrap();
        let r = NormalizedRegion::with_id(Uuid::new_v4(), 0.1, 0.2, 0.3, 0.4);
        store.add(r.clone()).unwrap();

        let store2 = RegionStore::open(&path).unwrap();
        let loaded = store2.current();
        assert_eq!(loaded.len(), 1);
        assert_eq!(loaded[0].id, r.id);
        assert!((loaded[0].x - 0.1).abs() < 1e-9);
    }

    #[test]
    fn persisted_json_has_sorted_keys_and_two_space_indent() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("regions.json");
        let store = RegionStore::open(&path).unwrap();
        store
            .add(NormalizedRegion::with_id(Uuid::nil(), 0.0, 0.0, 0.5, 0.5))
            .unwrap();

        let on_disk = std::fs::read_to_string(&path).unwrap();
        // sorted keys: height, id, width, x, y
        let h_pos = on_disk.find("\"height\"").unwrap();
        let id_pos = on_disk.find("\"id\"").unwrap();
        let w_pos = on_disk.find("\"width\"").unwrap();
        let x_pos = on_disk.find("\"x\"").unwrap();
        let y_pos = on_disk.find("\"y\"").unwrap();
        assert!(h_pos < id_pos && id_pos < w_pos && w_pos < x_pos && x_pos < y_pos);
        // 2-space indent
        assert!(on_disk.contains("\n    \"height\""));
    }

    #[test]
    fn replace_by_id_preserves_id() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("regions.json");
        let store = RegionStore::open(&path).unwrap();
        let id = Uuid::new_v4();
        store
            .add(NormalizedRegion::with_id(id, 0.0, 0.0, 0.1, 0.1))
            .unwrap();
        store
            .replace_id(id, NormalizedRegion::new(0.5, 0.5, 0.2, 0.2))
            .unwrap();
        let cur = store.current();
        assert_eq!(cur[0].id, id);
        assert!((cur[0].x - 0.5).abs() < 1e-9);
    }

    #[test]
    fn remove_and_clear() {
        let store = RegionStore::in_memory();
        let r = NormalizedRegion::new(0.0, 0.0, 0.1, 0.1);
        store.add(r.clone()).unwrap();
        store.remove(r.id).unwrap();
        assert!(store.current().is_empty());
        store
            .add(NormalizedRegion::new(0.0, 0.0, 0.1, 0.1))
            .unwrap();
        store.clear().unwrap();
        assert!(store.current().is_empty());
    }
}
