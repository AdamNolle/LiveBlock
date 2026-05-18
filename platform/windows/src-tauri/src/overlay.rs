//! Apply click-through layered top-most styles + capture-exclusion to the
//! render-layer HWND. Mirrors macOS `RenderLayerWindow` behaviour.

use anyhow::{Context, Result};
use windows::Win32::Foundation::{COLORREF, HWND};
use windows::Win32::UI::WindowsAndMessaging::{
    GetWindowLongPtrW, SetLayeredWindowAttributes, SetWindowDisplayAffinity,
    SetWindowLongPtrW, GWL_EXSTYLE, LWA_ALPHA, WDA_EXCLUDEFROMCAPTURE,
    WS_EX_LAYERED, WS_EX_NOACTIVATE, WS_EX_TOPMOST, WS_EX_TRANSPARENT,
};

/// Make this HWND a click-through, always-on-top, layered overlay that is
/// excluded from screen captures.
///
/// Equivalent macOS bits (NSWindow.collectionBehavior + .ignoresMouseEvents +
/// .level + sharingType=.none) live in `Sources/RenderLayerWindow.swift`.
pub fn make_render_overlay(hwnd: HWND) -> Result<()> {
    if hwnd.0 == 0 {
        return Err(anyhow::anyhow!("null HWND"));
    }
    unsafe {
        let current = GetWindowLongPtrW(hwnd, GWL_EXSTYLE);
        let new_style = current
            | WS_EX_LAYERED.0 as isize
            | WS_EX_TRANSPARENT.0 as isize
            | WS_EX_NOACTIVATE.0 as isize
            | WS_EX_TOPMOST.0 as isize;
        SetWindowLongPtrW(hwnd, GWL_EXSTYLE, new_style);
        SetLayeredWindowAttributes(hwnd, COLORREF(0), 255, LWA_ALPHA)
            .context("SetLayeredWindowAttributes")?;
        SetWindowDisplayAffinity(hwnd, WDA_EXCLUDEFROMCAPTURE)
            .context("SetWindowDisplayAffinity(WDA_EXCLUDEFROMCAPTURE)")?;
    }
    Ok(())
}

/// Editor window: never steals focus from the captured app, but is clickable.
pub fn make_editor(hwnd: HWND) -> Result<()> {
    if hwnd.0 == 0 {
        return Err(anyhow::anyhow!("null HWND"));
    }
    unsafe {
        let current = GetWindowLongPtrW(hwnd, GWL_EXSTYLE);
        let new_style = current | WS_EX_NOACTIVATE.0 as isize | WS_EX_TOPMOST.0 as isize;
        SetWindowLongPtrW(hwnd, GWL_EXSTYLE, new_style);
    }
    Ok(())
}
