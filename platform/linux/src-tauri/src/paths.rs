//! File-system layout for the labeling pipeline. Mirrors macOS `TrainingPaths`
//! and the Windows port. Schema is byte-compatible across platforms so labels
//! round-trip.
//!
//!   ~/.local/share/LiveBlock/
//!     regions.json
//!     training/
//!       screenshots/    full-res PNGs captured for labeling
//!       labels/         one JSON sidecar per labeled screenshot
//!       exports/        YOLO-format datasets
//!       trash/          discarded screenshots

use anyhow::{Context, Result};
use chrono::Utc;
use std::path::{Path, PathBuf};

/// XDG_DATA_HOME or ~/.local/share, then `LiveBlock`.
pub fn data_root() -> PathBuf {
    if let Some(p) = std::env::var_os("XDG_DATA_HOME") {
        let pb = PathBuf::from(p);
        if !pb.as_os_str().is_empty() {
            return pb.join("LiveBlock");
        }
    }
    dirs::data_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("LiveBlock")
}

pub fn regions_path() -> PathBuf {
    data_root().join("regions.json")
}

pub fn training_root() -> PathBuf {
    data_root().join("training")
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

pub fn ensure_directories() -> Result<()> {
    for dir in [
        data_root(),
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

pub fn new_screenshot_stem() -> String {
    Utc::now().format("%Y%m%d-%H%M%S-%3f").to_string() + "Z"
}

pub fn label_path_for(screenshot: &Path) -> PathBuf {
    let stem = screenshot
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default();
    labels_dir().join(format!("{stem}.json"))
}
