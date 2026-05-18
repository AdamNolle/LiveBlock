//! X11 overlay: override-redirect window + empty input shape via XFIXES.
//!
//! `override_redirect = true` keeps the WM out of layout decisions.
//! `_NET_WM_WINDOW_TYPE_DOCK` + `_NET_WM_STATE_ABOVE` keeps it above other
//! windows. `XFixesSetWindowShapeRegion(ShapeInput, NULL)` makes the window
//! ignore mouse events entirely.

use anyhow::{Context, Result};
use x11rb::connection::Connection;
use x11rb::protocol::xfixes::ConnectionExt as _;
use x11rb::protocol::xproto::ConnectionExt as _;

pub fn install() -> Result<()> {
    let (conn, _screen) = x11rb::connect(None).context("X11 connect")?;
    let _ = conn.xfixes_query_version(5, 0)?;

    // TODO(linux-port): create the actual overlay window via Tauri's webview
    // HWND-equivalent — Tauri 2 exposes the GdkSurface; from there we get
    // the X11 Window id, then call xfixes_create_region(empty) and
    // xfixes_set_window_shape_region(window, ShapeInput, region) on it.
    Ok(())
}
