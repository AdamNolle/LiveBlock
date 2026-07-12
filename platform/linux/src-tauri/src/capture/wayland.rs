//! Wayland capture via xdg-desktop-portal (`org.freedesktop.portal.ScreenCast`)
//! and the resulting PipeWire stream.
//!
//! Flow:
//!   1. ashpd::desktop::screencast::Screencast::create_session
//!   2. select_sources(types=Monitor, multiple=false, persist_mode=Application)
//!   3. start  → user gets the system permission dialog (first run)
//!   4. open_pipe_wire_remote → PipeWire fd
//!   5. negotiate format on the PipeWire stream (BGRA preferred)
//!   6. on each on_process callback, copy DMA-BUF / shm buffer into a FrameView
//!
//! TODO(linux-port): the actual PipeWire frame loop (steps 5–6) requires
//! pipewire-rs's `Stream` API. This module wires steps 1–4 with real ashpd
//! calls; the streaming bit is stubbed with a clear extension point.

use super::{CaptureSource, FrameView};
use anyhow::{anyhow, Context, Result};
use ashpd::desktop::screencast::{CursorMode, Screencast, SourceType};
use ashpd::desktop::PersistMode;
use ashpd::WindowIdentifier;
use std::sync::Arc;
use std::time::Duration;

pub struct WaylandCapture {
    /// PipeWire node id we're streaming from.
    pub node_id: u32,
    /// PipeWire connection fd (kept open for the duration of capture).
    _fd: std::os::fd::OwnedFd,
}

impl WaylandCapture {
    pub async fn new() -> Result<Self> {
        let proxy = Screencast::new()
            .await
            .context("create xdg-desktop-portal Screencast proxy")?;

        let session = proxy
            .create_session()
            .await
            .context("create_session")?;

        proxy
            .select_sources(
                &session,
                CursorMode::Embedded,
                SourceType::Monitor.into(),
                false, // multiple
                None,  // restore_token
                PersistMode::ExplicitlyRevoked,
            )
            .await
            .context("select_sources")?;

        let response = proxy
            .start(&session, &WindowIdentifier::default())
            .await
            .context("start")?
            .response()
            .context("start response")?;

        let stream = response
            .streams()
            .iter()
            .next()
            .ok_or_else(|| anyhow!("portal returned no PipeWire streams"))?;

        let node_id = stream.pipe_wire_node_id();

        let fd = proxy
            .open_pipe_wire_remote(&session)
            .await
            .context("open_pipe_wire_remote")?;

        Ok(Self { node_id, _fd: fd })
    }
}

#[async_trait::async_trait]
impl CaptureSource for WaylandCapture {
    async fn next_frame(&mut self) -> Result<FrameView> {
        // TODO(linux-port): pump the PipeWire stream and produce real frames.
        // The pipewire crate exposes Stream::add_listener with on_process to
        // receive Pod-formatted buffers. Format negotiation is required:
        // BGRA8888 is the preferred path; some compositors only offer YUY2
        // or NV12 (needs colorspace conversion).
        tokio::time::sleep(Duration::from_millis(16)).await;
        Ok(FrameView {
            pixels: Arc::from([0u8; 0]),
            width: 0,
            height: 0,
            stride: 0,
        })
    }

    fn stop(&mut self) {
        // Dropping `_fd` closes the PipeWire remote. The portal session is
        // released automatically when the proxy goes out of scope.
    }
}
