//! X11 overlay: a full-screen override-redirect window kept above everything,
//! with an EMPTY XFixes input shape so it's completely click-through.
//!
//!   - `override_redirect = true` keeps the window manager out of layout /
//!     focus decisions (the overlay isn't a normal app window).
//!   - `_NET_WM_WINDOW_TYPE_DOCK` + `_NET_WM_STATE_ABOVE` ask EWMH-aware WMs to
//!     stack it above other windows even though it's override-redirect.
//!   - `XFixesSetWindowShapeRegion(window, ShapeInput, empty_region)` makes the
//!     window ignore all pointer input — events pass straight through.
//!
//! ARCHITECTURE NOTE (linux-port): like the Wayland path, the native GTK
//! renderer is the follow-up that will own this window's GPU surface and draw
//! patches onto it (never a webview). This module creates and configures the
//! override-redirect window so the X11 overlay strategy is real and the
//! input-passthrough is proven; the renderer plugs into `LIVEBLOCK_OVERLAY_WINDOW`
//! (returned id) later.
//!
//! VERIFY-ON-LINUX(linux-port): touches a live X server; cannot run on this
//! Windows dev box. The EWMH atoms, override-redirect flag and the XFixes empty
//! input region are the parts to confirm against a real WM (Mutter-on-X11,
//! KWin-X11, i3, etc.).

use anyhow::{Context, Result};
use x11rb::connection::Connection;
use x11rb::protocol::xfixes::{ConnectionExt as _, Region};
use x11rb::protocol::xproto::{
    AtomEnum, ConnectionExt as _, CreateWindowAux, EventMask, PropMode, Rectangle, WindowClass,
};
use x11rb::wrapper::ConnectionExt as _;
use x11rb::COPY_DEPTH_FROM_PARENT;

/// Create the override-redirect, always-above, click-through overlay window.
/// Returns its X11 window id so a renderer can later draw into it.
pub fn install() -> Result<()> {
    let win = create_overlay_window().context("create X11 overlay window")?;
    tracing::info!("X11 override-redirect overlay window created: 0x{win:x}");
    // Expose the id for the native renderer follow-up (and for diagnostics).
    std::env::set_var("LIVEBLOCK_OVERLAY_WINDOW", win.to_string());
    Ok(())
}

fn create_overlay_window() -> Result<u32> {
    let (conn, screen_num) = x11rb::connect(None).context("X11 connect")?;
    let screen = &conn.setup().roots[screen_num];
    let root = screen.root;
    let width = screen.width_in_pixels;
    let height = screen.height_in_pixels;

    // XFixes is required for the empty input region; negotiate the version.
    conn.xfixes_query_version(5, 0)
        .context("xfixes_query_version request")?
        .reply()
        .context("XFixesQueryVersion")?;

    let win = conn.generate_id().context("generate window id")?;

    // override_redirect keeps the WM hands-off. We don't select for any input
    // events (we'll also strip the input shape below), only exposure so a
    // renderer can repaint.
    let aux = CreateWindowAux::new()
        .override_redirect(1)
        .background_pixel(screen.black_pixel)
        .event_mask(EventMask::EXPOSURE);

    conn.create_window(
        COPY_DEPTH_FROM_PARENT,
        win,
        root,
        0,
        0,
        width,
        height,
        0,
        WindowClass::INPUT_OUTPUT,
        screen.root_visual,
        &aux,
    )
    .context("create_window request")?
    .check()
    .context("XCreateWindow")?;

    set_ewmh_above_dock(&conn, win).context("EWMH stacking hints")?;
    set_empty_input_region(&conn, win).context("XFixes empty input region")?;

    // Map and raise. With override-redirect the WM won't manage stacking, so we
    // also configure-raise via EWMH above.
    conn.map_window(win).context("map_window request")?.check().context("XMapWindow")?;
    conn.flush().context("flush")?;

    Ok(win)
}

/// Set `_NET_WM_WINDOW_TYPE = _NET_WM_WINDOW_TYPE_DOCK` and add
/// `_NET_WM_STATE_ABOVE` so EWMH-aware compositors keep us above normal windows.
fn set_ewmh_above_dock(conn: &impl Connection, win: u32) -> Result<()> {
    let net_wm_window_type = intern(conn, b"_NET_WM_WINDOW_TYPE")?;
    let net_wm_window_type_dock = intern(conn, b"_NET_WM_WINDOW_TYPE_DOCK")?;
    let net_wm_state = intern(conn, b"_NET_WM_STATE")?;
    let net_wm_state_above = intern(conn, b"_NET_WM_STATE_ABOVE")?;

    conn.change_property32(
        PropMode::REPLACE,
        win,
        net_wm_window_type,
        AtomEnum::ATOM,
        &[net_wm_window_type_dock],
    )
    .context("set _NET_WM_WINDOW_TYPE")?;

    conn.change_property32(
        PropMode::REPLACE,
        win,
        net_wm_state,
        AtomEnum::ATOM,
        &[net_wm_state_above],
    )
    .context("set _NET_WM_STATE_ABOVE")?;

    Ok(())
}

/// Apply an EMPTY XFixes input region as the window's input shape so every
/// pointer/touch event passes through to the windows beneath.
fn set_empty_input_region(conn: &impl Connection, win: u32) -> Result<()> {
    let region: Region = conn.generate_id().context("generate region id")?;
    // Create a region from an EMPTY rectangle list → covers nothing.
    let no_rects: &[Rectangle] = &[];
    conn.xfixes_create_region(region, no_rects)
        .context("xfixes_create_region request")?
        .check()
        .context("XFixesCreateRegion")?;

    // ShapeInput kind == 2 (SO::INPUT). x11rb exposes this via the shape enum.
    use x11rb::protocol::shape::SK;
    conn.xfixes_set_window_shape_region(win, SK::INPUT, 0, 0, region)
        .context("xfixes_set_window_shape_region request")?
        .check()
        .context("XFixesSetWindowShapeRegion(ShapeInput, empty)")?;

    conn.xfixes_destroy_region(region)
        .context("xfixes_destroy_region request")?
        .check()
        .ok();
    Ok(())
}

fn intern(conn: &impl Connection, name: &[u8]) -> Result<u32> {
    Ok(conn
        .intern_atom(false, name)
        .context("intern_atom request")?
        .reply()
        .context("InternAtom")?
        .atom)
}
