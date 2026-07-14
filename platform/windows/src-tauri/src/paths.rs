//! File-system layout for the labeling pipeline. Mirrors macOS `TrainingPaths`.
//!
//! `%APPDATA%\LiveBlock\` (typically `C:\Users\<you>\AppData\Roaming\LiveBlock\`)
//!   regions.json
//!   training\
//!     screenshots\   full-res PNGs captured for labeling
//!     labels\        one JSON sidecar per labeled screenshot
//!     exports\       YOLO-format datasets exported by tools/export_labels.py
//!     trash\         discarded screenshots
//!
//! Schema is byte-compatible with macOS so labels round-trip across platforms.

use anyhow::{Context, Result};
use chrono::Utc;
use liveblock_config::{validate_managed_file_path, ManagedPathMode};
use std::path::{Path, PathBuf};

/// `%APPDATA%\LiveBlock`. Falls back to the platform config dir if APPDATA isn't set.
pub fn appdata_root() -> PathBuf {
    if let Some(p) = std::env::var_os("APPDATA") {
        return PathBuf::from(p).join("LiveBlock");
    }
    dirs::config_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("LiveBlock")
}

pub fn regions_path() -> PathBuf {
    appdata_root().join("regions.json")
}

pub fn models_dir() -> PathBuf {
    appdata_root().join("models")
}
pub fn active_model_path() -> PathBuf {
    models_dir().join("liveblock-detector.onnx")
}
pub fn model_update_state_path() -> PathBuf {
    models_dir().join("update-state.json")
}

pub fn training_root() -> PathBuf {
    appdata_root().join("training")
}
pub fn screenshots_dir() -> PathBuf {
    training_root().join("screenshots")
}
pub fn labels_dir() -> PathBuf {
    training_root().join("labels")
}
pub fn exports_dir() -> PathBuf {
    training_root().join("exports")
}
pub fn trash_dir() -> PathBuf {
    training_root().join("trash")
}

/// Idempotent. Creates every required directory.
pub fn ensure_directories() -> Result<()> {
    for dir in [
        appdata_root(),
        models_dir(),
        training_root(),
        screenshots_dir(),
        labels_dir(),
        exports_dir(),
        trash_dir(),
    ] {
        std::fs::create_dir_all(&dir).with_context(|| format!("create {}", dir.display()))?;
    }
    Ok(())
}

/// Sortable filename stem. Same format as macOS: `yyyyMMdd-HHmmss-SSSZ`.
pub fn new_screenshot_stem() -> String {
    // macOS uses `yyyyMMdd-HHmmss-SSS` + literal "Z". We do the same.
    Utc::now().format("%Y%m%d-%H%M%S-%3f").to_string() + "Z"
}

/// Path to the JSON label sidecar for a screenshot path.
pub fn validate_screenshot_path(path: &Path) -> Result<PathBuf> {
    ensure_directories()?;
    validate_managed_file_path(
        path,
        &screenshots_dir(),
        "png",
        ManagedPathMode::ExistingRegularFile,
    )
    .context("validate managed screenshot path")
}

pub fn validate_label_path(path: &Path, must_exist: bool) -> Result<PathBuf> {
    ensure_directories()?;
    validate_managed_file_path(
        path,
        &labels_dir(),
        "json",
        if must_exist {
            ManagedPathMode::ExistingRegularFile
        } else {
            ManagedPathMode::ExistingOrNewRegularFile
        },
    )
    .context("validate managed label path")
}

pub fn label_path_for(screenshot: &Path) -> PathBuf {
    let stem = screenshot
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default();
    labels_dir().join(format!("{stem}.json"))
}
