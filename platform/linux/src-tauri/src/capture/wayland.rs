//! Wayland capture via xdg-desktop-portal (`org.freedesktop.portal.ScreenCast`)
//! and the resulting PipeWire stream.
//!
//! Flow:
//!   1. ashpd::desktop::screencast::Screencast::create_session
//!   2. select_sources(types=Monitor, multiple=false, persist_mode=…)
//!   3. start  → user gets the system permission dialog (first run)
//!   4. open_pipe_wire_remote → PipeWire fd
//!   5. spin up a PipeWire `MainLoop` on a dedicated thread, connect the remote
//!      via that fd, create a `Stream` on `node_id`, negotiate BGRA/RGBA/YUY2/
//!      NV12, and in `on_process` copy the buffer (honoring per-plane stride)
//!      into a `FrameView`.
//!   6. bridge frames to the async `next_frame()` via a bounded channel.
//!
//! PipeWire's `Stream`/`MainLoop` are single-threaded and not `Send`, so they
//! live entirely on the worker thread. The async side only ever sees finished
//! `FrameView`s through the channel — no PipeWire types cross the boundary.
//!
//! VERIFY-ON-LINUX(linux-port): the pipewire-rs SPA pod construction and the
//! buffer plane layout are coded to the documented 0.8 API but cannot be
//! exercised on this Windows dev box. The negotiation list, stride handling and
//! YUY2/NV12 → BGRA conversion are the parts most worth re-checking against a
//! live compositor.

use super::{CaptureSource, FrameView};
use anyhow::{anyhow, Context, Result};
use ashpd::desktop::screencast::{CursorMode, PersistMode, Screencast, SourceType};
use crossbeam_channel::{Receiver, Sender};
use std::sync::Arc;

/// A decoded frame handed from the PipeWire worker thread to the async side.
/// Always tightly-packed BGRA8 (stride == width*4); any source stride padding
/// and any YUY2/NV12 source is normalized away on the worker thread.
type FrameMsg = FrameView;

pub struct WaylandCapture {
    /// PipeWire node id we're streaming from (kept for diagnostics).
    pub node_id: u32,
    /// Receives decoded frames from the PipeWire worker thread.
    rx: Receiver<FrameMsg>,
    /// Signals the worker thread to quit (it watches for disconnect).
    _stop_tx: Sender<()>,
    /// Worker thread join handle (joined on `stop`).
    worker: Option<std::thread::JoinHandle<()>>,
    /// Last good frame — returned if the channel momentarily has nothing (so
    /// the detection loop sees steady frames rather than 0x0 gaps).
    last: Option<FrameView>,
}

impl WaylandCapture {
    pub async fn new() -> Result<Self> {
        let proxy = Screencast::new()
            .await
            .context("create xdg-desktop-portal Screencast proxy")?;

        let session = proxy.create_session().await.context("create_session")?;

        proxy
            .select_sources(
                &session,
                CursorMode::Embedded,
                SourceType::Monitor.into(),
                false, // multiple
                None,  // restore_token
                // Persist the grant for this app so the dialog is one-time.
                PersistMode::Application,
            )
            .await
            .context("select_sources")?;

        let response = proxy
            .start(&session, None)
            .await
            .context("start")?
            .response()
            .context("start response")?;

        // Read the node id off the first stream by reference — ashpd's `Stream`
        // is not `Clone`, and the node id is all we need to connect PipeWire.
        let node_id = {
            let streams = response.streams();
            let stream = streams
                .iter()
                .next()
                .ok_or_else(|| anyhow!("portal returned no PipeWire streams"))?;
            stream.pipe_wire_node_id()
        };

        let fd = proxy
            .open_pipe_wire_remote(&session)
            .await
            .context("open_pipe_wire_remote")?;

        // Hand the owned fd to the PipeWire worker thread. The portal session is
        // held alive by `proxy`/`session` being moved into the worker closure's
        // outer scope via the leaked guard below.
        let (tx, rx) = crossbeam_channel::bounded::<FrameMsg>(2);
        let (stop_tx, stop_rx) = crossbeam_channel::bounded::<()>(1);

        // Keep the portal session alive for the lifetime of the stream. ashpd
        // releases the session on drop, which would tear down the PipeWire node,
        // so we leak it intentionally (process-lifetime). It is reclaimed when
        // the app exits. Holding it in the struct would force `Self: !Send`
        // because the proxy isn't `Send` across the worker boundary cleanly.
        std::mem::forget(session);
        std::mem::forget(proxy);

        let worker = std::thread::Builder::new()
            .name("pipewire-capture".into())
            .spawn(move || {
                if let Err(e) = run_pipewire_loop(fd, node_id, tx, stop_rx) {
                    tracing::error!("pipewire capture loop exited: {e:#}");
                }
            })
            .context("spawn pipewire worker")?;

        Ok(Self {
            node_id,
            rx,
            _stop_tx: stop_tx,
            worker: Some(worker),
            last: None,
        })
    }
}

#[async_trait::async_trait]
impl CaptureSource for WaylandCapture {
    async fn next_frame(&mut self) -> Result<FrameView> {
        // The PipeWire worker produces frames on its own clock. Pull the freshest
        // one: drain the bounded(2) channel keeping only the newest frame; if the
        // channel is momentarily empty, yield the last good frame so detection
        // stays warm rather than stalling on a 0x0 gap.
        let mut latest = None;
        loop {
            match self.rx.try_recv() {
                Ok(f) => latest = Some(f),
                Err(crossbeam_channel::TryRecvError::Empty) => break,
                Err(crossbeam_channel::TryRecvError::Disconnected) => {
                    if latest.is_none() && self.last.is_none() {
                        return Err(anyhow!("pipewire worker disconnected"));
                    }
                    break;
                }
            }
        }
        if let Some(f) = latest {
            self.last = Some(f.clone());
            return Ok(f);
        }
        if let Some(f) = &self.last {
            // No new frame this tick — pace to ~capture rate and reuse the last.
            tokio::time::sleep(std::time::Duration::from_millis(8)).await;
            return Ok(f.clone());
        }

        // First call(s) before format negotiation completes: block-await one
        // frame with a timeout (multi-thread runtime required for block_in_place).
        match tokio::task::block_in_place(|| {
            self.rx.recv_timeout(std::time::Duration::from_millis(250))
        }) {
            Ok(f) => {
                self.last = Some(f.clone());
                Ok(f)
            }
            Err(crossbeam_channel::RecvTimeoutError::Timeout) => Ok(FrameView {
                pixels: Arc::from([0u8; 0]),
                width: 0,
                height: 0,
                stride: 0,
            }),
            Err(crossbeam_channel::RecvTimeoutError::Disconnected) => {
                Err(anyhow!("pipewire worker disconnected"))
            }
        }
    }

    fn stop(&mut self) {
        // Dropping the stop channel sender signals the worker; joining ensures
        // the PipeWire main loop is fully torn down before we return.
        let _ = self._stop_tx.try_send(());
        if let Some(h) = self.worker.take() {
            // The worker watches the stop channel inside its main-loop timer.
            let _ = h.join();
        }
    }
}

// ===========================================================================
// PipeWire worker thread
// ===========================================================================

/// Runs the PipeWire main loop, negotiating a video format and pumping frames
/// into `tx` until the remote disconnects or `stop_rx` fires. All PipeWire
/// objects are created and dropped on THIS thread (they are not `Send`).
fn run_pipewire_loop(
    fd: std::os::fd::OwnedFd,
    node_id: u32,
    tx: Sender<FrameMsg>,
    stop_rx: Receiver<()>,
) -> Result<()> {
    use pipewire as pw;
    use pw::{properties::properties, spa};

    pw::init();

    let mainloop = pw::main_loop::MainLoop::new(None).context("pw MainLoop")?;
    let context = pw::context::Context::new(&mainloop).context("pw Context")?;

    // Connect to the portal-provided remote via the inherited fd.
    let core = context
        .connect_fd(fd, None)
        .context("pw connect_fd (portal remote)")?;

    // Poll the stop channel from inside the loop. PipeWire's loop drives a timer
    // we use to check for shutdown cooperatively.
    let main_for_timer = mainloop.clone();
    let timer = mainloop.loop_().add_timer(move |_| {
        if stop_rx.try_recv().is_ok() {
            main_for_timer.quit();
        }
    });
    // Fire every 100ms.
    timer
        .update_timer(
            Some(std::time::Duration::from_millis(100)),
            Some(std::time::Duration::from_millis(100)),
        )
        .into_result()
        .ok();

    // Per-stream negotiated state shared with the `process` callback.
    let format: Arc<parking_lot::Mutex<Option<VideoFormat>>> =
        Arc::new(parking_lot::Mutex::new(None));

    let stream = pw::stream::Stream::new(
        &core,
        "liveblock-capture",
        properties! {
            *pw::keys::MEDIA_TYPE => "Video",
            *pw::keys::MEDIA_CATEGORY => "Capture",
            *pw::keys::MEDIA_ROLE => "Screen",
        },
    )
    .context("pw Stream::new")?;

    let format_for_param = format.clone();
    let format_for_process = format.clone();
    let tx_for_process = tx.clone();

    let _listener = stream
        .add_local_listener_with_user_data(())
        .state_changed(|_stream, _ud, old, new| {
            tracing::debug!("pipewire stream state {old:?} -> {new:?}");
        })
        .param_changed(move |_stream, _ud, id, param| {
            // We only care about the negotiated Format param.
            let Some(param) = param else { return };
            if id != spa::param::ParamType::Format.as_raw() {
                return;
            }
            match parse_video_format(param) {
                Ok(vf) => {
                    tracing::info!(
                        "pipewire negotiated {}x{} fourcc={:?}",
                        vf.width,
                        vf.height,
                        vf.fourcc
                    );
                    *format_for_param.lock() = Some(vf);
                }
                Err(e) => tracing::warn!("parse format param failed: {e:#}"),
            }
        })
        .process(move |stream, _ud| {
            let Some(mut buffer) = stream.dequeue_buffer() else {
                return;
            };
            let vf = match *format_for_process.lock() {
                Some(vf) => vf,
                None => return, // format not negotiated yet
            };
            let datas = buffer.datas_mut();
            if datas.is_empty() {
                return;
            }
            if let Some(frame) = decode_buffer(&mut datas[0], vf) {
                // Drop the frame if the consumer is behind (bounded channel) —
                // we want the freshest frame, never a backlog.
                let _ = tx_for_process.try_send(frame);
            }
        })
        .register()
        .context("register stream listener")?;

    // Build the EnumFormat param we accept and serialize it into a SPA Pod byte
    // buffer (the form `Stream::connect` consumes). pipewire-rs builds params as
    // `spa::pod::Value::Object`, serializes to bytes, then reborrows as `&Pod`.
    let format_obj = build_format_params();
    let pod_bytes = {
        use spa::pod::serialize::PodSerializer;
        let cursor = std::io::Cursor::new(Vec::new());
        PodSerializer::serialize(cursor, &spa::pod::Value::Object(format_obj))
            .map_err(|e| anyhow!("serialize format pod: {e:?}"))?
            .0
            .into_inner()
    };
    let pod = spa::pod::Pod::from_bytes(&pod_bytes)
        .ok_or_else(|| anyhow!("invalid serialized format pod"))?;

    stream
        .connect(
            spa::utils::Direction::Input,
            Some(node_id),
            pw::stream::StreamFlags::AUTOCONNECT
                | pw::stream::StreamFlags::MAP_BUFFERS
                | pw::stream::StreamFlags::RT_PROCESS,
            &mut [pod],
        )
        .context("stream.connect")?;

    // Blocks on this thread until `quit()` (stop signal or remote disconnect).
    mainloop.run();
    Ok(())
}

/// Negotiated video format details we need to decode buffers.
#[derive(Debug, Clone, Copy)]
struct VideoFormat {
    width: u32,
    height: u32,
    fourcc: SourceFourcc,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SourceFourcc {
    Bgrx,
    Bgra,
    Rgbx,
    Rgba,
    Yuy2,
    Nv12,
}

/// The video formats we advertise to the compositor, most-preferred first.
/// BGRA/BGRx need no conversion (frontend/detector are BGRA); YUY2 and NV12 are
/// accepted because some compositors only offer packed/planar YUV.
fn build_format_params() -> pipewire::spa::pod::Object {
    use pipewire::spa;
    use spa::param::video::VideoFormat as SpaVideoFormat;
    use spa::pod::{Object, Property, PropertyFlags, Value};

    // We build one EnumFormat object listing an enumeration of pixel formats and
    // a size/framerate range. The compositor picks one and replies via
    // `param_changed`.
    let formats = [
        SpaVideoFormat::BGRx,
        SpaVideoFormat::BGRA,
        SpaVideoFormat::RGBx,
        SpaVideoFormat::RGBA,
        SpaVideoFormat::YUY2,
        SpaVideoFormat::NV12,
    ];

    // VERIFY-ON-LINUX(linux-port): pod builder ergonomics differ slightly across
    // pipewire-rs patch releases. The intent is an EnumFormat object with:
    //   mediaType = video, mediaSubtype = raw,
    //   format    = Enum(formats…),
    //   size      = Range(default 1920x1080, min 1x1, max 8192x8192),
    //   framerate = Range(default 60/1, min 0/1, max 240/1).
    let obj = Object {
        type_: spa::utils::SpaTypes::ObjectParamFormat.as_raw(),
        id: spa::param::ParamType::EnumFormat.as_raw(),
        properties: vec![
            Property {
                key: spa::param::format::FormatProperties::MediaType.as_raw(),
                flags: PropertyFlags::empty(),
                value: Value::Id(spa::utils::Id(
                    spa::param::format::MediaType::Video.as_raw(),
                )),
            },
            Property {
                key: spa::param::format::FormatProperties::MediaSubtype.as_raw(),
                flags: PropertyFlags::empty(),
                value: Value::Id(spa::utils::Id(
                    spa::param::format::MediaSubtype::Raw.as_raw(),
                )),
            },
            Property {
                key: spa::param::format::FormatProperties::VideoFormat.as_raw(),
                flags: PropertyFlags::empty(),
                value: Value::Choice(spa::pod::ChoiceValue::Id(spa::utils::Choice(
                    spa::utils::ChoiceFlags::empty(),
                    spa::utils::ChoiceEnum::Enum {
                        default: spa::utils::Id(formats[0].as_raw()),
                        alternatives: formats
                            .iter()
                            .map(|f| spa::utils::Id(f.as_raw()))
                            .collect(),
                    },
                ))),
            },
            Property {
                key: spa::param::format::FormatProperties::VideoSize.as_raw(),
                flags: PropertyFlags::empty(),
                value: Value::Choice(spa::pod::ChoiceValue::Rectangle(spa::utils::Choice(
                    spa::utils::ChoiceFlags::empty(),
                    spa::utils::ChoiceEnum::Range {
                        default: spa::utils::Rectangle {
                            width: 1920,
                            height: 1080,
                        },
                        min: spa::utils::Rectangle {
                            width: 1,
                            height: 1,
                        },
                        max: spa::utils::Rectangle {
                            width: 8192,
                            height: 8192,
                        },
                    },
                ))),
            },
            Property {
                key: spa::param::format::FormatProperties::VideoFramerate.as_raw(),
                flags: PropertyFlags::empty(),
                value: Value::Choice(spa::pod::ChoiceValue::Fraction(spa::utils::Choice(
                    spa::utils::ChoiceFlags::empty(),
                    spa::utils::ChoiceEnum::Range {
                        default: spa::utils::Fraction { num: 60, denom: 1 },
                        min: spa::utils::Fraction { num: 0, denom: 1 },
                        max: spa::utils::Fraction { num: 240, denom: 1 },
                    },
                ))),
            },
        ],
    };

    obj
}

/// Parse the compositor's chosen Format param back into our `VideoFormat`.
fn parse_video_format(param: &pipewire::spa::pod::Pod) -> Result<VideoFormat> {
    use pipewire::spa;
    use spa::param::video::VideoInfoRaw;

    let mut info = VideoInfoRaw::new();
    info.parse(param)
        .map_err(|e| anyhow!("VideoInfoRaw::parse: {e:?}"))?;

    let size = info.size();
    let fourcc = map_spa_format(info.format())
        .ok_or_else(|| anyhow!("unsupported negotiated format {:?}", info.format()))?;

    Ok(VideoFormat {
        width: size.width,
        height: size.height,
        fourcc,
    })
}

fn map_spa_format(f: pipewire::spa::param::video::VideoFormat) -> Option<SourceFourcc> {
    use pipewire::spa::param::video::VideoFormat as F;
    Some(match f {
        F::BGRx => SourceFourcc::Bgrx,
        F::BGRA => SourceFourcc::Bgra,
        F::RGBx => SourceFourcc::Rgbx,
        F::RGBA => SourceFourcc::Rgba,
        F::YUY2 => SourceFourcc::Yuy2,
        F::NV12 => SourceFourcc::Nv12,
        _ => return None,
    })
}

/// Copy/convert one PipeWire buffer plane into a packed BGRA `FrameView`,
/// honoring the source row stride (PipeWire planes are frequently padded).
fn decode_buffer(
    data: &mut pipewire::buffer::Data,
    vf: VideoFormat,
) -> Option<FrameView> {
    let w = vf.width as usize;
    let h = vf.height as usize;
    if w == 0 || h == 0 {
        return None;
    }
    // Read the chunk stride into a plain value first so the immutable borrow of
    // `data` ends before we take the mutable `data.data()` slice.
    let stride = data.chunk().stride().max(0) as usize;
    let src = data.data()?;
    if src.is_empty() {
        return None;
    }

    let mut out = vec![0u8; w * h * 4];

    // `src` is `&mut [u8]`; the conversion helpers take `&[u8]` (reborrowed).
    let src: &[u8] = src;
    match vf.fourcc {
        SourceFourcc::Bgrx | SourceFourcc::Bgra => {
            let row_stride = if stride >= w * 4 { stride } else { w * 4 };
            copy_packed_4(src, &mut out, w, h, row_stride, false, true);
        }
        SourceFourcc::Rgbx | SourceFourcc::Rgba => {
            let row_stride = if stride >= w * 4 { stride } else { w * 4 };
            // RGBA → BGRA: swap R/B per pixel.
            copy_packed_4(src, &mut out, w, h, row_stride, true, true);
        }
        SourceFourcc::Yuy2 => {
            let row_stride = if stride >= w * 2 { stride } else { w * 2 };
            yuy2_to_bgra(src, &mut out, w, h, row_stride);
        }
        SourceFourcc::Nv12 => {
            // NV12: Y plane (w*h) then interleaved CbCr plane (w*h/2). Stride
            // applies to the Y plane; chroma stride matches by convention.
            let y_stride = if stride >= w { stride } else { w };
            nv12_to_bgra(src, &mut out, w, h, y_stride);
        }
    }

    Some(FrameView {
        pixels: Arc::from(out.into_boxed_slice()),
        width: vf.width,
        height: vf.height,
        stride: (w * 4) as u32,
    })
}

/// Copy a 4-byte-per-pixel source into a packed BGRA destination. `swap_rb`
/// swaps channels 0/2 (RGBA→BGRA). `force_opaque` writes alpha=255 (for the
/// x-variants whose 4th byte is undefined).
fn copy_packed_4(
    src: &[u8],
    dst: &mut [u8],
    w: usize,
    h: usize,
    src_stride: usize,
    swap_rb: bool,
    force_opaque: bool,
) {
    for y in 0..h {
        let s_row = y * src_stride;
        let d_row = y * w * 4;
        if s_row + w * 4 > src.len() {
            break;
        }
        for x in 0..w {
            let s = s_row + x * 4;
            let d = d_row + x * 4;
            let (b, g, r) = if swap_rb {
                (src[s + 2], src[s + 1], src[s])
            } else {
                (src[s], src[s + 1], src[s + 2])
            };
            dst[d] = b;
            dst[d + 1] = g;
            dst[d + 2] = r;
            dst[d + 3] = if force_opaque { 255 } else { src[s + 3] };
        }
    }
}

/// YUY2 (a.k.a. YUYV): 4 bytes encode 2 pixels = Y0 U Y1 V. BT.601 limited.
fn yuy2_to_bgra(src: &[u8], dst: &mut [u8], w: usize, h: usize, src_stride: usize) {
    for y in 0..h {
        let s_row = y * src_stride;
        let d_row = y * w * 4;
        if s_row + (w / 2) * 4 > src.len() {
            break;
        }
        let mut x = 0;
        while x + 1 < w {
            let s = s_row + (x / 2) * 4;
            let y0 = src[s] as f32;
            let u = src[s + 1] as f32 - 128.0;
            let y1 = src[s + 2] as f32;
            let v = src[s + 3] as f32 - 128.0;
            write_yuv_bgra(dst, d_row + x * 4, y0, u, v);
            write_yuv_bgra(dst, d_row + (x + 1) * 4, y1, u, v);
            x += 2;
        }
    }
}

/// NV12: full-res Y plane, then half-res interleaved CbCr plane. BT.601.
fn nv12_to_bgra(src: &[u8], dst: &mut [u8], w: usize, h: usize, y_stride: usize) {
    let y_plane_len = y_stride * h;
    let c_stride = y_stride; // CbCr interleaved → same byte stride as Y
    for y in 0..h {
        let y_row = y * y_stride;
        let c_row = y_plane_len + (y / 2) * c_stride;
        let d_row = y * w * 4;
        if c_row + (w & !1) > src.len() {
            break;
        }
        for x in 0..w {
            let yv = src[y_row + x] as f32;
            let ci = c_row + (x & !1);
            let u = src[ci] as f32 - 128.0;
            let v = src[ci + 1] as f32 - 128.0;
            write_yuv_bgra(dst, d_row + x * 4, yv, u, v);
        }
    }
}

#[inline]
fn write_yuv_bgra(dst: &mut [u8], off: usize, yv: f32, u: f32, v: f32) {
    if off + 4 > dst.len() {
        return;
    }
    // BT.601 full-range-ish; good enough for ad detection + cover.
    let r = (yv + 1.402 * v).clamp(0.0, 255.0) as u8;
    let g = (yv - 0.344136 * u - 0.714136 * v).clamp(0.0, 255.0) as u8;
    let b = (yv + 1.772 * u).clamp(0.0, 255.0) as u8;
    dst[off] = b;
    dst[off + 1] = g;
    dst[off + 2] = r;
    dst[off + 3] = 255;
}
