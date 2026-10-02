//! Per-class detection rules + global thresholds, persisted like
//! `liveblock-regions::RegionStore`.
//!
//! The on-disk JSON format is byte-compatible with Swift's
//! `JSONEncoder([.prettyPrinted, .sortedKeys])`: 2-space indentation and sorted
//! keys, produced via [`crate::pretty_two_space`]. Persistence uses the same
//! atomic-write shape as `RegionStore` (unique `pid.nanos.tmp` sibling ->
//! fsync -> rename) so concurrent writers can't clobber each other and a crash
//! can never leave a half-written settings file.

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use serde::{Deserialize, Serialize};

use crate::ConfigError;

pub const SETTINGS_SCHEMA_VERSION: u32 = 1;

fn current_settings_schema() -> u32 {
    SETTINGS_SCHEMA_VERSION
}

/// Global detector tuning. Defined here (not in `liveblock-core`) so the leaf
/// `liveblock-config` crate has no upward dependency on core; `liveblock-core`
/// re-exports this type to avoid a `core <-> config` cycle.
///
/// `detect_every` runs the detector once every N frames; other frames reuse the
/// most recent detections (cheaper steady-state).
///
/// Fields are declared in alphabetical order so that serializing the struct
/// directly (not through `serde_json::Value`) yields sorted keys while
/// preserving each `f32`'s shortest decimal representation — `0.45`, not the
/// lossy `0.44999998807907104` you'd get by widening to `f64` via `Value`.
/// That shortest form is exactly what Swift's `JSONEncoder` emits for `Float`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CoordinatorConfig {
    pub detect_every: u32,
    pub iou_threshold: f32,
    pub score_threshold: f32,
}

impl Default for CoordinatorConfig {
    fn default() -> Self {
        Self {
            score_threshold: 0.25,
            iou_threshold: 0.45,
            detect_every: 4,
        }
    }
}

/// A per-class override: whether the class is active, and an optional
/// class-specific score threshold (falls back to the global one when `None`).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ClassRule {
    pub class_id: u32,
    pub enabled: bool,
    pub score_threshold: Option<f32>,
}

/// The full detection-settings document persisted to disk. Fields are declared
/// in alphabetical order so direct serialization yields sorted keys (see
/// [`CoordinatorConfig`]).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct DetectionSettings {
    pub classes: Vec<ClassRule>,
    pub global: CoordinatorConfig,
    #[serde(rename = "schemaVersion", default = "current_settings_schema")]
    pub schema_version: u32,
    pub vocabulary_version: u32,
}

impl Default for DetectionSettings {
    fn default() -> Self {
        Self {
            global: CoordinatorConfig::default(),
            classes: Vec::new(),
            schema_version: SETTINGS_SCHEMA_VERSION,
            vocabulary_version: 1,
        }
    }
}

impl DetectionSettings {
    /// Effective score threshold for a class: its per-class override if set,
    /// otherwise the global threshold (also used for unknown class ids).
    pub fn effective_threshold(&self, class_id: u32) -> f32 {
        self.classes
            .iter()
            .find(|r| r.class_id == class_id)
            .and_then(|r| r.score_threshold)
            .unwrap_or(self.global.score_threshold)
    }
}

/// Thread-safe persistent store of detection settings.
///
/// Mirrors `liveblock-regions::RegionStore`: a `Mutex<DetectionSettings>` plus
/// an optional backing path. Mutators only touch in-memory state; call
/// [`SettingsStore::persist`] to flush to disk via an atomic write.
#[derive(Debug)]
pub struct SettingsStore {
    inner: Mutex<DetectionSettings>,
    path: Option<PathBuf>,
}

impl SettingsStore {
    /// In-memory store with no on-disk persistence.
    pub fn in_memory() -> Self {
        Self {
            inner: Mutex::new(DetectionSettings::default()),
            path: None,
        }
    }

    /// Open or create a store backed by the given JSON file. If the file exists
    /// and parses, its contents are loaded; otherwise the store starts from
    /// `DetectionSettings::default()`. Parse errors are surfaced rather than
    /// silently discarding user data.
    pub fn open<P: AsRef<Path>>(path: P) -> Result<Self, ConfigError> {
        let path = path.as_ref().to_path_buf();
        let (settings, migrate_legacy) = match fs::read(&path) {
            Ok(bytes) if bytes.is_empty() => (DetectionSettings::default(), false),
            Ok(bytes) => {
                let value: serde_json::Value = serde_json::from_slice(&bytes)?;
                let had_schema = value.get("schemaVersion").is_some();
                let settings: DetectionSettings = serde_json::from_value(value)?;
                if settings.schema_version != SETTINGS_SCHEMA_VERSION {
                    return Err(ConfigError::UnsupportedSchema {
                        document: "settings",
                        found: settings.schema_version,
                        current: SETTINGS_SCHEMA_VERSION,
                    });
                }
                (settings, !had_schema)
            }
            Err(ref e) if e.kind() == std::io::ErrorKind::NotFound => {
                (DetectionSettings::default(), false)
            }
            Err(e) => return Err(ConfigError::Io(e.to_string())),
        };
        let store = Self {
            inner: Mutex::new(settings),
            path: Some(path),
        };
        if migrate_legacy {
            store.persist()?;
        }
        Ok(store)
    }

    /// Snapshot of the current settings.
    pub fn current(&self) -> DetectionSettings {
        self.inner.lock().expect("settings mutex poisoned").clone()
    }

    /// Enable or disable a class, inserting a rule if one doesn't exist yet.
    pub fn set_class_enabled(&self, class_id: u32, enabled: bool) -> Result<(), ConfigError> {
        let mut g = self.inner.lock().expect("settings mutex poisoned");
        match g.classes.iter_mut().find(|r| r.class_id == class_id) {
            Some(rule) => rule.enabled = enabled,
            None => g.classes.push(ClassRule {
                class_id,
                enabled,
                score_threshold: None,
            }),
        }
        Ok(())
    }

    /// Set (or clear) a class's score-threshold override, inserting a rule if
    /// one doesn't exist yet.
    pub fn set_class_threshold(
        &self,
        class_id: u32,
        score_threshold: Option<f32>,
    ) -> Result<(), ConfigError> {
        let mut g = self.inner.lock().expect("settings mutex poisoned");
        match g.classes.iter_mut().find(|r| r.class_id == class_id) {
            Some(rule) => rule.score_threshold = score_threshold,
            None => g.classes.push(ClassRule {
                class_id,
                enabled: true,
                score_threshold,
            }),
        }
        Ok(())
    }

    /// Flush current settings to disk as 2-space, sorted-key JSON via an atomic
    /// `tmp -> fsync -> rename`. No-op for an in-memory store.
    pub fn persist(&self) -> Result<(), ConfigError> {
        let Some(path) = &self.path else {
            return Ok(());
        };
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(|e| ConfigError::Io(e.to_string()))?;
        }

        let snapshot = self.inner.lock().expect("settings mutex poisoned").clone();
        // Serialize the struct DIRECTLY (not through serde_json::Value). The
        // struct fields are declared in alphabetical order, so derived
        // serialization already emits sorted keys — and, unlike the Value path,
        // serde_json's serialize_f32 keeps each f32's shortest decimal form
        // (e.g. 0.45, not 0.44999998807907104), matching Swift byte-for-byte.
        let mut buf = Vec::new();
        let fmt = serde_json::ser::PrettyFormatter::with_indent(b"  ");
        let mut ser = serde_json::Serializer::with_formatter(&mut buf, fmt);
        snapshot.serialize(&mut ser)?;
        let bytes = buf;

        // Atomic write via a unique tmp file (process + nanos suffix) so two
        // writers can't clobber each other's tmp. The tmp lives in the same
        // directory as the target so the rename is atomic on the same fs.
        let stem = path
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or("detection-settings.json");
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.subsec_nanos())
            .unwrap_or(0);
        let pid = std::process::id();
        let tmp = path.with_file_name(format!("{stem}.{pid}.{nanos}.tmp"));
        {
            let mut f = fs::File::create(&tmp).map_err(|e| ConfigError::Io(e.to_string()))?;
            f.write_all(&bytes)
                .map_err(|e| ConfigError::Io(e.to_string()))?;
            f.sync_all().map_err(|e| ConfigError::Io(e.to_string()))?;
        }
        fs::rename(&tmp, path).map_err(|e| ConfigError::Io(e.to_string()))?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn published_schema_matches_runtime_version() {
        let schema: serde_json::Value =
            serde_json::from_str(include_str!("../../../../contracts/settings.schema.json"))
                .unwrap();
        assert_eq!(
            schema["properties"]["schemaVersion"]["const"],
            SETTINGS_SCHEMA_VERSION
        );
    }

    #[test]
    fn effective_threshold_prefers_class_override() {
        let s = DetectionSettings {
            global: CoordinatorConfig {
                score_threshold: 0.35,
                iou_threshold: 0.45,
                detect_every: 4,
            },
            classes: vec![
                ClassRule {
                    class_id: 1,
                    enabled: true,
                    score_threshold: Some(0.6),
                },
                ClassRule {
                    class_id: 2,
                    enabled: true,
                    score_threshold: None,
                },
            ],
            schema_version: SETTINGS_SCHEMA_VERSION,
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
        assert!(json.contains("\"schemaVersion\": 1"));
        assert!(json.starts_with("{\n  \"classes\":")); // sorted, 2-space
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn legacy_settings_are_migrated_and_future_versions_rejected() {
        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir = std::env::temp_dir().join(format!("lbcfg-schema-{}-{nonce}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("detection-settings.json");
        let legacy = r#"{"classes":[],"global":{"detect_every":4,"iou_threshold":0.45,"score_threshold":0.25},"vocabulary_version":1}"#;
        std::fs::write(&path, legacy).unwrap();
        let store = SettingsStore::open(&path).unwrap();
        assert_eq!(store.current().schema_version, SETTINGS_SCHEMA_VERSION);
        assert!(std::fs::read_to_string(&path)
            .unwrap()
            .contains("\"schemaVersion\": 1"));

        let future = r#"{"classes":[],"global":{"detect_every":4,"iou_threshold":0.45,"score_threshold":0.25},"schemaVersion":99,"vocabulary_version":1}"#;
        std::fs::write(&path, future).unwrap();
        let error = SettingsStore::open(&path).unwrap_err();
        assert!(matches!(
            error,
            ConfigError::UnsupportedSchema { found: 99, .. }
        ));
        assert_eq!(std::fs::read_to_string(&path).unwrap(), future);
        let _ = std::fs::remove_dir_all(dir);
    }
}
