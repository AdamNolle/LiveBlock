//! Training-label types for LiveBlock.
//!
//! Mirrors `LabelBox` and `LabelDocument` in `Sources/LabelingController.swift`.
//! Schema 1 sidecars use pretty-printed, sorted-key encoding with ISO-8601
//! dates. Legacy unversioned documents migrate atomically; future versions are
//! rejected without rewriting.

use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::collections::BTreeMap;
use std::fs;
use std::io::Write;
use std::path::Path;
use thiserror::Error;
use uuid::Uuid;

#[derive(Debug, Error)]
pub enum LabelError {
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("json error: {0}")]
    Json(#[from] serde_json::Error),
    #[error("invalid date: {0}")]
    Date(String),
    #[error("unsupported labels schema version {found}; current version is {current}")]
    UnsupportedSchema { found: u32, current: u32 },
}

/// A single labeled box on a screenshot. Coords normalized [0..1] with
/// origin top-left. Clamped on construction.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LabelBox {
    pub id: Uuid,
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

impl LabelBox {
    pub fn new(x: f64, y: f64, width: f64, height: f64) -> Self {
        Self::with_id(Uuid::new_v4(), x, y, width, height)
    }

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

    /// Pixel rect for the given image size. Returns `(x, y, w, h)`.
    pub fn rect_in(&self, width: f64, height: f64) -> (f64, f64, f64, f64) {
        (
            self.x * width,
            self.y * height,
            self.width * width,
            self.height * height,
        )
    }

    /// Intersection-over-union with another normalized box.
    pub fn iou(&self, other: &LabelBox) -> f64 {
        iou(
            (self.x, self.y, self.width, self.height),
            (other.x, other.y, other.width, other.height),
        )
    }
}

/// IoU on `(x, y, w, h)` tuples in any consistent coordinate system.
pub fn iou(a: (f64, f64, f64, f64), b: (f64, f64, f64, f64)) -> f64 {
    let (ax1, ay1, aw, ah) = a;
    let (bx1, by1, bw, bh) = b;
    let ax2 = ax1 + aw;
    let ay2 = ay1 + ah;
    let bx2 = bx1 + bw;
    let by2 = by1 + bh;
    let inter_x = (ax2.min(bx2) - ax1.max(bx1)).max(0.0);
    let inter_y = (ay2.min(by2) - ay1.max(by1)).max(0.0);
    let inter = inter_x * inter_y;
    let union = aw * ah + bw * bh - inter;
    if union <= 0.0 {
        0.0
    } else {
        inter / union
    }
}

pub const LABELS_SCHEMA_VERSION: u32 = 1;

fn current_labels_schema() -> u32 {
    LABELS_SCHEMA_VERSION
}

/// Persisted JSON sidecar for one labeled screenshot.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LabelDocument {
    #[serde(rename = "schemaVersion", default = "current_labels_schema")]
    pub schema_version: u32,
    /// Filename only (no path) so the dataset is portable.
    pub image: String,
    #[serde(rename = "imageWidth")]
    pub image_width: i64,
    #[serde(rename = "imageHeight")]
    pub image_height: i64,
    pub boxes: Vec<LabelBox>,
    /// ISO-8601 timestamp; serialized as a string to match Swift's
    /// `JSONEncoder.dateEncodingStrategy = .iso8601`.
    #[serde(
        rename = "labeledAt",
        serialize_with = "serialize_iso8601",
        deserialize_with = "deserialize_iso8601"
    )]
    pub labeled_at: Iso8601,
}

/// ISO-8601 timestamp with second precision and a trailing `Z`, matching
/// Foundation's default ISO-8601 output (`yyyy-MM-ddTHH:mm:ssZ`).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Iso8601(pub String);

impl Iso8601 {
    /// Build from UNIX seconds since the epoch (UTC).
    pub fn from_unix_seconds(secs: i64) -> Self {
        Self(format_iso8601(secs))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

fn serialize_iso8601<S>(v: &Iso8601, s: S) -> Result<S::Ok, S::Error>
where
    S: Serializer,
{
    s.serialize_str(&v.0)
}

fn deserialize_iso8601<'de, D>(d: D) -> Result<Iso8601, D::Error>
where
    D: Deserializer<'de>,
{
    let s = String::deserialize(d)?;
    if !is_valid_iso8601(&s) {
        return Err(serde::de::Error::custom(format!(
            "not an ISO-8601 datetime: {s}"
        )));
    }
    Ok(Iso8601(s))
}

fn is_valid_iso8601(s: &str) -> bool {
    // Cheap shape check: YYYY-MM-DDTHH:MM:SS(.fff)?(Z|+hh:mm)
    let bytes = s.as_bytes();
    if bytes.len() < 20 {
        return false;
    }
    let digits = |idx: &[usize]| idx.iter().all(|&i| bytes[i].is_ascii_digit());
    digits(&[0, 1, 2, 3])
        && bytes[4] == b'-'
        && digits(&[5, 6])
        && bytes[7] == b'-'
        && digits(&[8, 9])
        && bytes[10] == b'T'
        && digits(&[11, 12])
        && bytes[13] == b':'
        && digits(&[14, 15])
        && bytes[16] == b':'
        && digits(&[17, 18])
}

/// Format UNIX seconds (UTC) as `yyyy-MM-ddTHH:mm:ssZ`.
fn format_iso8601(unix_secs: i64) -> String {
    // Days since 1970-01-01.
    let days = unix_secs.div_euclid(86_400);
    let secs_of_day = unix_secs.rem_euclid(86_400);
    let h = secs_of_day / 3600;
    let m = (secs_of_day % 3600) / 60;
    let s = secs_of_day % 60;
    let (y, mo, d) = civil_from_days(days);
    format!("{y:04}-{mo:02}-{d:02}T{h:02}:{m:02}:{s:02}Z")
}

/// Howard Hinnant's days-from-civil algorithm, reversed.
fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719_468;
    let era = if z >= 0 {
        z / 146_097
    } else {
        (z - 146_096) / 146_097
    };
    let doe = (z - era * 146_097) as u64; // [0, 146096]
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146_096) / 365; // [0, 399]
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100); // [0, 365]
    let mp = (5 * doy + 2) / 153; // [0, 11]
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32; // [1, 31]
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32; // [1, 12]
    let y = if m <= 2 { y + 1 } else { y };
    (y, m, d)
}

impl LabelDocument {
    /// Save to disk as pretty-printed, sorted-key JSON (matches Swift output).
    pub fn save<P: AsRef<Path>>(&self, path: P) -> Result<(), LabelError> {
        let path = path.as_ref();
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let bytes = self.to_pretty_sorted_bytes()?;
        // Unique tmp suffix so concurrent writers don't clobber each other.
        let stem = path
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or("label.json");
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

    /// Load a document from disk.
    pub fn load<P: AsRef<Path>>(path: P) -> Result<Self, LabelError> {
        let path = path.as_ref();
        let bytes = fs::read(path)?;
        let value: serde_json::Value = serde_json::from_slice(&bytes)?;
        let had_schema = value.get("schemaVersion").is_some();
        let doc: LabelDocument = serde_json::from_value(value)?;
        if doc.schema_version != LABELS_SCHEMA_VERSION {
            return Err(LabelError::UnsupportedSchema {
                found: doc.schema_version,
                current: LABELS_SCHEMA_VERSION,
            });
        }
        if !had_schema {
            doc.save(path)?;
        }
        Ok(doc)
    }

    /// Render to bytes with sorted keys and 2-space indent.
    pub fn to_pretty_sorted_bytes(&self) -> Result<Vec<u8>, LabelError> {
        if self.schema_version != LABELS_SCHEMA_VERSION {
            return Err(LabelError::UnsupportedSchema {
                found: self.schema_version,
                current: LABELS_SCHEMA_VERSION,
            });
        }
        // Build a sorted representation per box plus the doc.
        let boxes: Vec<serde_json::Value> = self
            .boxes
            .iter()
            .map(|b| {
                let mut m: BTreeMap<&'static str, serde_json::Value> = BTreeMap::new();
                m.insert(
                    "id",
                    serde_json::Value::String(b.id.to_string().to_uppercase()),
                );
                m.insert("x", serde_json::json!(b.x));
                m.insert("y", serde_json::json!(b.y));
                m.insert("width", serde_json::json!(b.width));
                m.insert("height", serde_json::json!(b.height));
                serde_json::Value::from_iter(m)
            })
            .collect();

        let mut top: BTreeMap<&'static str, serde_json::Value> = BTreeMap::new();
        top.insert("boxes", serde_json::Value::Array(boxes));
        top.insert("image", serde_json::Value::String(self.image.clone()));
        top.insert("imageHeight", serde_json::json!(self.image_height));
        top.insert("imageWidth", serde_json::json!(self.image_width));
        top.insert(
            "labeledAt",
            serde_json::Value::String(self.labeled_at.0.clone()),
        );
        top.insert("schemaVersion", serde_json::json!(LABELS_SCHEMA_VERSION));

        let value = serde_json::Value::from_iter(top);
        let mut buf = Vec::new();
        let fmt = serde_json::ser::PrettyFormatter::with_indent(b"  ");
        let mut ser = serde_json::Serializer::with_formatter(&mut buf, fmt);
        serde::Serialize::serialize(&value, &mut ser)?;
        Ok(buf)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn published_schema_matches_runtime_version() {
        let schema: serde_json::Value =
            serde_json::from_str(include_str!("../../../../contracts/labels.schema.json")).unwrap();
        assert_eq!(
            schema["properties"]["schemaVersion"]["const"],
            LABELS_SCHEMA_VERSION
        );
    }

    #[test]
    fn iou_zero_when_disjoint() {
        let a = LabelBox::with_id(Uuid::nil(), 0.0, 0.0, 0.1, 0.1);
        let b = LabelBox::with_id(Uuid::nil(), 0.5, 0.5, 0.1, 0.1);
        assert_eq!(a.iou(&b), 0.0);
    }

    #[test]
    fn iou_one_when_identical() {
        let a = LabelBox::with_id(Uuid::nil(), 0.1, 0.2, 0.3, 0.4);
        let b = a.clone();
        assert!((a.iou(&b) - 1.0).abs() < 1e-12);
    }

    #[test]
    fn iou_known_value() {
        // Two unit boxes overlapping by half each side -> inter = 0.25, union = 1.75
        let a = LabelBox::with_id(Uuid::nil(), 0.0, 0.0, 1.0, 1.0);
        let b = LabelBox::with_id(Uuid::nil(), 0.5, 0.5, 0.5, 0.5);
        let v = a.iou(&b);
        assert!((v - 0.25 / 1.0).abs() < 1e-9, "got {v}");
    }

    #[test]
    fn label_clamps_inputs() {
        let b = LabelBox::new(-1.0, -1.0, 5.0, 5.0);
        assert_eq!(b.x, 0.0);
        assert_eq!(b.y, 0.0);
        assert!(b.width <= 1.0 && b.height <= 1.0);
    }

    #[test]
    fn iso8601_format_unix_zero() {
        let s = format_iso8601(0);
        assert_eq!(s, "1970-01-01T00:00:00Z");
    }

    #[test]
    fn iso8601_format_known_value() {
        // 2024-01-02T03:04:05Z
        // 54 years (incl 13 leap days) + 1 day + 3:04:05
        let s = format_iso8601(1_704_164_645);
        assert_eq!(s, "2024-01-02T03:04:05Z");
    }

    #[test]
    fn label_document_round_trips() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("shot.json");
        let doc = LabelDocument {
            schema_version: LABELS_SCHEMA_VERSION,
            image: "shot.png".to_string(),
            image_width: 1920,
            image_height: 1080,
            boxes: vec![LabelBox::with_id(Uuid::nil(), 0.1, 0.2, 0.3, 0.4)],
            labeled_at: Iso8601::from_unix_seconds(0),
        };
        doc.save(&path).unwrap();
        let loaded = LabelDocument::load(&path).unwrap();
        assert_eq!(loaded, doc);
    }

    #[test]
    fn saved_json_has_sorted_top_level_keys() {
        let doc = LabelDocument {
            schema_version: LABELS_SCHEMA_VERSION,
            image: "shot.png".to_string(),
            image_width: 1,
            image_height: 1,
            boxes: vec![],
            labeled_at: Iso8601::from_unix_seconds(0),
        };
        let bytes = doc.to_pretty_sorted_bytes().unwrap();
        let s = std::str::from_utf8(&bytes).unwrap();
        let boxes_pos = s.find("\"boxes\"").unwrap();
        let image_pos = s.find("\"image\"").unwrap();
        let height_pos = s.find("\"imageHeight\"").unwrap();
        let width_pos = s.find("\"imageWidth\"").unwrap();
        let labeled_pos = s.find("\"labeledAt\"").unwrap();
        let schema_pos = s.find("\"schemaVersion\"").unwrap();
        assert!(boxes_pos < image_pos);
        assert!(image_pos < height_pos);
        assert!(height_pos < width_pos);
        assert!(width_pos < labeled_pos);
        assert!(labeled_pos < schema_pos);
    }

    #[test]
    fn legacy_label_migrates_and_future_schema_is_preserved() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("shot.json");
        let legacy = r#"{"image":"shot.png","imageWidth":1,"imageHeight":1,"boxes":[],"labeledAt":"1970-01-01T00:00:00Z"}"#;
        std::fs::write(&path, legacy).unwrap();
        let loaded = LabelDocument::load(&path).unwrap();
        assert_eq!(loaded.schema_version, LABELS_SCHEMA_VERSION);
        assert!(std::fs::read_to_string(&path)
            .unwrap()
            .contains("\"schemaVersion\": 1"));

        let future = r#"{"schemaVersion":99,"image":"shot.png","imageWidth":1,"imageHeight":1,"boxes":[],"labeledAt":"1970-01-01T00:00:00Z"}"#;
        std::fs::write(&path, future).unwrap();
        let error = LabelDocument::load(&path).unwrap_err();
        assert!(matches!(
            error,
            LabelError::UnsupportedSchema { found: 99, .. }
        ));
        assert_eq!(std::fs::read_to_string(path).unwrap(), future);
    }
}
