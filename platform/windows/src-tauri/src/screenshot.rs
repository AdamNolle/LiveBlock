//! Save the freshest captured frame as a labeling screenshot. Shared by the
//! Tauri command and the tray/hotkey actions so both produce identical PNGs.

use std::path::PathBuf;

use anyhow::Result;

use crate::paths;
use crate::state::AppState;

/// Encode the freshest captured BGRA frame to a timestamped PNG under the
/// training screenshots dir. Returns `Ok(None)` if no frame is available (i.e.
/// capture isn't running yet).
pub fn save_latest_frame(state: &AppState) -> Result<Option<PathBuf>> {
    let Some(frame) = state.latest_frame.load_full() else {
        return Ok(None);
    };
    let _ = paths::ensure_directories();
    let stem = paths::new_screenshot_stem();
    let dst = paths::screenshots_dir().join(format!("{stem}.png"));

    // BGRA -> RGBA -> PNG.
    let mut rgba = Vec::with_capacity(frame.bytes.len());
    for c in frame.bytes.chunks_exact(4) {
        rgba.extend_from_slice(&[c[2], c[1], c[0], c[3]]);
    }
    let img = image::ImageBuffer::<image::Rgba<u8>, _>::from_raw(frame.width, frame.height, rgba)
        .ok_or_else(|| anyhow::anyhow!("invalid frame buffer"))?;
    img.save(&dst)?;
    Ok(Some(dst))
}
