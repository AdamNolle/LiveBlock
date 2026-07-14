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
use liveblock_config::{validate_managed_file_path, ManagedPathMode};
use std::fs::File;
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
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

pub fn models_dir() -> PathBuf {
    data_root().join("models")
}
pub fn active_model_path() -> PathBuf {
    models_dir().join("liveblock-detector.onnx")
}
pub fn model_update_state_path() -> PathBuf {
    models_dir().join("update-state.json")
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
        models_dir(),
        training_root(),
        screenshots_dir(),
        labels_dir(),
        exports_dir(),
        trash_dir(),
    ] {
        ensure_private_directory(&dir)?;
    }
    Ok(())
}

fn ensure_private_directory(path: &Path) -> Result<()> {
    std::fs::create_dir_all(path).with_context(|| format!("create {}", path.display()))?;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o700))
        .with_context(|| format!("protect {}", path.display()))
}

pub fn create_private_file(path: &Path) -> Result<File> {
    std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(path)
        .with_context(|| format!("create private file {}", path.display()))
}

pub fn new_screenshot_stem() -> String {
    Utc::now().format("%Y%m%d-%H%M%S-%3f").to_string() + "Z"
}

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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn private_capture_paths_reject_group_and_other_access() {
        let root = std::env::temp_dir().join(format!("liveblock-private-{}", uuid::Uuid::new_v4()));
        ensure_private_directory(&root).unwrap();
        let file_path = root.join("frame.png");
        drop(create_private_file(&file_path).unwrap());
        assert_eq!(
            std::fs::metadata(&root).unwrap().permissions().mode() & 0o777,
            0o700
        );
        assert_eq!(
            std::fs::metadata(&file_path).unwrap().permissions().mode() & 0o777,
            0o600
        );
        std::fs::remove_dir_all(root).unwrap();
    }
}
