//! Re-export the shared `liveblock-labels` types so the Windows port shares
//! the on-disk format with macOS / Linux byte-for-byte. Only the
//! `ScreenshotEntry` UI type stays local — it's purely the shape the Tauri
//! frontend wants for the labeling list.

pub use liveblock_labels::{Iso8601, LabelBox, LabelDocument};

use serde::Serialize;
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize)]
pub struct ScreenshotEntry {
    pub path: PathBuf,
    pub stem: String,
    pub labeled: bool,
}
