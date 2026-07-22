//! Re-export shared `liveblock-labels` types so the Linux port shares the
//! on-disk JSON format with macOS / Windows byte-for-byte.

pub use liveblock_labels::{Iso8601, LabelBox, LabelDocument};

use serde::Serialize;
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ScreenshotEntry {
    pub path: PathBuf,
    pub label_path: PathBuf,
    pub stem: String,
    pub labeled: bool,
}
