//! X11 capture via XComposite (redirected window manager output) + XShm
//! (shared-memory pixel transfer).
//!
//! XComposite tells the server to render windows into off-screen pixmaps so
//! we can pull them without race conditions. XShm gives us a (near) zero-copy
//! shared-memory pipe for the actual pixel bytes: we allocate a SysV shared
//! segment with `shmget`/`shmat`, hand its id to the X server via
//! `XShmAttach`, then each frame `XShmGetImage` fills the segment directly and
//! we read it back as BGRX → BGRA.
//!
//! VERIFY-ON-LINUX(linux-port): the SysV shm syscalls go through `libc` and
//! cannot be exercised on this Windows dev box. The x11rb round-trip and the
//! BGRX→BGRA conversion are the parts worth re-checking against a live X
//! server (notably the server-side `ZPixmap` byte order on big-endian, which
//! we assume little-endian here as is universal on desktop Linux).

use super::{CaptureSource, FrameView};
use anyhow::{anyhow, Context, Result};
use std::sync::Arc;
use x11rb::connection::Connection;
use x11rb::protocol::composite::ConnectionExt as _;
use x11rb::protocol::shm::ConnectionExt as _;
use x11rb::protocol::xproto::{ConnectionExt as _, ImageFormat, Screen};
use x11rb::rust_connection::RustConnection;

/// RAII wrapper over a SysV shared-memory segment (shmget + shmat).
struct ShmSegment {
    /// SysV IPC id from `shmget`.
    shmid: i32,
    /// Mapped address from `shmat`.
    addr: *mut libc::c_void,
    /// Byte length of the segment.
    len: usize,
}

// The mapped pointer is only ever touched on the capture thread while a frame
// is being read; we never share it across threads concurrently.
unsafe impl Send for ShmSegment {}

impl ShmSegment {
    fn new(len: usize) -> Result<Self> {
        // 0o600 = owner rw. IPC_CREAT to allocate a fresh segment.
        let shmid = unsafe { libc::shmget(libc::IPC_PRIVATE, len, libc::IPC_CREAT | 0o600) };
        if shmid < 0 {
            return Err(anyhow!(
                "shmget({len}) failed: {}",
                std::io::Error::last_os_error()
            ));
        }
        let addr = unsafe { libc::shmat(shmid, std::ptr::null(), 0) };
        if addr == (-1isize as *mut libc::c_void) {
            // Clean up the id we just created before bailing.
            unsafe { libc::shmctl(shmid, libc::IPC_RMID, std::ptr::null_mut()) };
            return Err(anyhow!(
                "shmat failed: {}",
                std::io::Error::last_os_error()
            ));
        }
        Ok(Self { shmid, addr, len })
    }

    /// View the mapped memory as a byte slice.
    fn as_slice(&self) -> &[u8] {
        unsafe { std::slice::from_raw_parts(self.addr as *const u8, self.len) }
    }
}

impl Drop for ShmSegment {
    fn drop(&mut self) {
        unsafe {
            libc::shmdt(self.addr);
            // IPC_RMID marks the segment for deletion once detached everywhere
            // (the X server detaches when we XShmDetach / close the connection).
            libc::shmctl(self.shmid, libc::IPC_RMID, std::ptr::null_mut());
        }
    }
}

pub struct X11Capture {
    conn: RustConnection,
    root: u32,
    width: u16,
    height: u16,
    /// X server's id for the attached shm segment.
    shm_seg: u32,
    /// The shared-memory backing store. `None` if shm setup failed (we then
    /// fall back to a plain `GetImage` round-trip).
    segment: Option<ShmSegment>,
}

impl X11Capture {
    pub fn new() -> Result<Self> {
        let (conn, screen_num) = x11rb::connect(None).context("X11 connect")?;
        let setup = conn.setup();
        let screen: &Screen = &setup.roots[screen_num];
        let root = screen.root;
        let width = screen.width_in_pixels;
        let height = screen.height_in_pixels;

        // Redirect rendering of all top-level windows to off-screen storage so
        // we can read the composited result without tearing.
        conn.composite_redirect_subwindows(root, x11rb::protocol::composite::Redirect::AUTOMATIC)
            .context("composite_redirect_subwindows request")?
            .check()
            .context("composite_redirect_subwindows")?;

        // Allocate a SysV shm segment sized for one BGRA frame and attach it to
        // the X server. If anything here fails we degrade to non-shm GetImage.
        let len = width as usize * height as usize * 4;
        let (shm_seg, segment) = match Self::setup_shm(&conn, len) {
            Ok(pair) => (pair.0, Some(pair.1)),
            Err(e) => {
                tracing::warn!("XShm setup failed ({e:#}); falling back to GetImage");
                (0, None)
            }
        };

        Ok(Self {
            conn,
            root,
            width,
            height,
            shm_seg,
            segment,
        })
    }

    /// shmget/shmat a segment and XShmAttach it. Returns the X server's segment
    /// id and the RAII handle.
    fn setup_shm(conn: &RustConnection, len: usize) -> Result<(u32, ShmSegment)> {
        let segment = ShmSegment::new(len)?;
        let shm_seg = conn.generate_id().context("generate shm seg id")?;
        // read_only = false: the server writes pixels into our segment.
        conn.shm_attach(shm_seg, segment.shmid as u32, false)
            .context("shm_attach request")?
            .check()
            .context("XShmAttach")?;
        Ok((shm_seg, segment))
    }

    /// Convert a server BGRX/BGRA buffer (little-endian ZPixmap, depth 24/32)
    /// into packed opaque BGRA.
    fn finalize_bgra(src: &[u8], w: u32, h: u32) -> Vec<u8> {
        let count = (w as usize) * (h as usize);
        let mut out = vec![0u8; count * 4];
        let n = count.min(src.len() / 4);
        for i in 0..n {
            let s = i * 4;
            // ZPixmap little-endian: bytes are B, G, R, X.
            out[s] = src[s];
            out[s + 1] = src[s + 1];
            out[s + 2] = src[s + 2];
            out[s + 3] = 255; // force opaque; X often leaves the 4th byte as pad
        }
        out
    }
}

#[async_trait::async_trait]
impl CaptureSource for X11Capture {
    async fn next_frame(&mut self) -> Result<FrameView> {
        let w = self.width as u32;
        let h = self.height as u32;
        if w == 0 || h == 0 {
            return Ok(FrameView {
                pixels: Arc::from([0u8; 0]),
                width: 0,
                height: 0,
                stride: 0,
            });
        }

        // X11 has no frame-ready signal for root-window polling, so we pace the
        // grab to ~60 Hz here; the runtime loop throttles inference further.
        tokio::time::sleep(std::time::Duration::from_millis(16)).await;

        // The x11rb calls are synchronous round-trips. They run on this tokio
        // worker thread; on the multi-thread runtime that's acceptable for a
        // dedicated capture task. (A future optimization is the XDamage/XShm
        // completion event to avoid full-frame polling.)
        let bgra = if let Some(seg) = &self.segment {
            // Fast path: server DMAs the image into our shared segment.
            self.conn
                .shm_get_image(
                    self.root,
                    0,
                    0,
                    self.width,
                    self.height,
                    u32::MAX, // plane mask: all planes
                    ImageFormat::Z_PIXMAP.into(),
                    self.shm_seg,
                    0, // offset into segment
                )
                .context("shm_get_image request")?
                .reply()
                .context("XShmGetImage")?;
            Self::finalize_bgra(seg.as_slice(), w, h)
        } else {
            // Fallback path: a regular GetImage round-trip (slower, copies the
            // pixels back over the socket).
            let reply = self
                .conn
                .get_image(
                    ImageFormat::Z_PIXMAP,
                    self.root,
                    0,
                    0,
                    self.width,
                    self.height,
                    u32::MAX,
                )
                .context("get_image request")?
                .reply()
                .context("GetImage")?;
            Self::finalize_bgra(&reply.data, w, h)
        };

        Ok(FrameView {
            pixels: Arc::from(bgra.into_boxed_slice()),
            width: w,
            height: h,
            stride: w * 4,
        })
    }

    fn stop(&mut self) {
        // Detach the shm segment from the X server before the connection (and
        // the segment's Drop) tear everything down.
        if self.segment.is_some() {
            let _ = self.conn.shm_detach(self.shm_seg);
            let _ = self.conn.flush();
        }
        // `self.segment`'s Drop releases the SysV segment; closing `self.conn`
        // releases the XComposite redirection.
    }
}
