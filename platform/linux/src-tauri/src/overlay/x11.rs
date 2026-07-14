//! X11 render overlay: virtual-root sized, override-redirect, top-most through
//! Tauri/GTK, and empty XFixes input shape for click-through behavior.

use anyhow::{Context, Result};
use gtk::prelude::*;
use tauri::{Manager, PhysicalPosition, PhysicalSize, Position, Size};
use x11rb::connection::Connection;
use x11rb::protocol::shape::SK;
use x11rb::protocol::xfixes::ConnectionExt as _;

pub fn install(app: &tauri::AppHandle) -> Result<()> {
    let render = app
        .get_webview_window("render")
        .context("render window is unavailable")?;
    let gtk_window = render.gtk_window().context("resolve GTK render window")?;
    gtk_window.realize();
    gtk_window.set_accept_focus(false);
    gtk_window.set_focus_on_map(false);
    let gdk_window = gtk_window.window().context("realize GDK render window")?;
    gdk_window.set_override_redirect(true);
    let x11_window = gdk_window
        .downcast::<gdkx11::X11Window>()
        .map_err(|_| anyhow::anyhow!("render window is not on the X11 backend"))?;
    let xid = x11_window.xid() as u32;

    let (conn, screen_number) = x11rb::connect(None).context("connect X11 overlay control")?;
    let screen = &conn.setup().roots[screen_number];
    render
        .set_position(Position::Physical(PhysicalPosition::new(0, 0)))
        .context("position X11 overlay")?;
    render
        .set_size(Size::Physical(PhysicalSize::new(
            u32::from(screen.width_in_pixels),
            u32::from(screen.height_in_pixels),
        )))
        .context("size X11 overlay")?;
    render
        .set_ignore_cursor_events(true)
        .context("make Tauri X11 overlay click-through")?;

    conn.xfixes_query_version(5, 0)?
        .reply()
        .context("query XFixes")?;
    let empty_region = conn.generate_id().context("allocate XFixes region")?;
    conn.xfixes_create_region(empty_region, &[])?.check()?;
    conn.xfixes_set_window_shape_region(xid, SK::INPUT, 0, 0, empty_region)?
        .check()?;
    conn.xfixes_destroy_region(empty_region)?.check()?;
    conn.flush().context("flush X11 overlay shape")?;
    Ok(())
}
