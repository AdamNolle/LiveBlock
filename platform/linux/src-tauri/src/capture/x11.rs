//! X11 capture via XComposite (redirected window manager output) + XShm
//! (shared-memory pixel transfer).
//!
//! XComposite tells the server to render windows into off-screen pixmaps so
//! we can pull them without race conditions. XShm gives us a zero-copy
//! shared-memory pipe for the actual pixel bytes.

use super::{CaptureSource, FrameView};
use anyhow::{anyhow, Context, Result};
use std::sync::Arc;
use x11rb::connection::Connection;
use x11rb::protocol::composite::ConnectionExt as _;
use x11rb::protocol::shm::ConnectionExt as _;
use x11rb::protocol::xproto::{ConnectionExt as _, Screen};
use x11rb::rust_connection::RustConnection;

pub struct X11Capture {
    conn: RustConnection,
    root: u32,
    width: u16,
    height: u16,
    shm_seg: u32,
    pixmap: u32,
}

impl X11Capture {
    pub fn new() -> Result<Self> {
        let (conn, screen_num) = x11rb::connect(None).context("X11 connect")?;
        let setup = conn.setup();
        let screen: &Screen = &setup.roots[screen_num];
        let root = screen.root;
        let width = screen.width_in_pixels;
        let height = screen.height_in_pixels;

        // Tell the X server to redirect rendering of all top-level windows to
        // off-screen pixmaps so we can read them without tearing.
        conn.composite_redirect_subwindows(root, x11rb::protocol::composite::Redirect::AUTOMATIC)?
            .check()
            .context("composite_redirect_subwindows")?;

        // TODO(linux-port): allocate XShm segment + create the destination
        // pixmap. x11rb exposes shm::create_segment and xproto::create_pixmap
        // but the actual handshake also needs sysv shmget which lives in libc.
        // For now we record placeholders so the rest of the flow can compile.
        let shm_seg = 0;
        let pixmap = 0;

        Ok(Self {
            conn,
            root,
            width,
            height,
            shm_seg,
            pixmap,
        })
    }
}

#[async_trait::async_trait]
impl CaptureSource for X11Capture {
    async fn next_frame(&mut self) -> Result<FrameView> {
        // TODO(linux-port): perform shm_get_image on self.root and ship the
        // resulting BGRA bytes into a FrameView. x11rb's shm extension makes
        // the round-trip; the libc shm segment is what backs the data.
        Ok(FrameView {
            pixels: Arc::from([0u8; 0]),
            width: self.width as u32,
            height: self.height as u32,
            stride: (self.width as u32) * 4,
        })
    }

    fn stop(&mut self) {
        // XComposite redirection persists per-connection. Closing `self.conn`
        // releases everything.
    }
}
