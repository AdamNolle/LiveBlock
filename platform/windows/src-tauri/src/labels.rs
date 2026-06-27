//! Re-export the shared `liveblock-labels` types so the Windows port shares
//! the on-disk format with macOS / Linux byte-for-byte. Only the
//! `ScreenshotEntry` UI type stays local — it's purely the shape the Tauri
//! frontend wants for the labeling list.

// Re-export the shared label types. `LabelDocument` is used directly by the
// command layer; `LabelBox`/`Iso8601`/`LabelClass` are part of the on-disk
// schema and kept available to callers (and future label-editing commands).
#[allow(unused_imports)]
pub use liveblock_labels::{Iso8601, LabelBox, LabelClass, LabelDocument};

use serde::Serialize;
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize)]
pub struct ScreenshotEntry {
    pub path: PathBuf,
    pub stem: String,
    pub labeled: bool,
}
