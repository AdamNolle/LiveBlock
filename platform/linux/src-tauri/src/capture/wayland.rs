//! Wayland monitor capture through xdg-desktop-portal and PipeWire.
//! Portal ownership remains alive for the capture lifetime. PipeWire runs on a
//! dedicated thread because its local main loop is intentionally !Send.

use super::{CaptureSource, FrameView};
use anyhow::{anyhow, Context, Result};
use ashpd::desktop::screencast::{CursorMode, Screencast, SourceType};
use ashpd::desktop::{PersistMode, Session};
use ashpd::WindowIdentifier;
use crossbeam_channel::{bounded, Receiver, Sender, TrySendError};
use pipewire as pw;
use pw::spa::pod::Pod;
use pw::{properties::properties, spa};
use std::os::fd::OwnedFd;
use std::sync::{
    atomic::{AtomicBool, AtomicU64, Ordering},
    Arc,
};
use std::thread::{self, JoinHandle};
use std::time::Duration;

struct PipeWireWorkerGuard {
    stop_requested: Arc<AtomicBool>,
    worker: Option<JoinHandle<()>>,
}

impl Drop for PipeWireWorkerGuard {
    fn drop(&mut self) {
        self.stop_requested.store(true, Ordering::Release);
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

pub struct WaylandCapture {
    pub node_id: u32,
    _portal: Screencast<'static>,
    _session: Session<'static, Screencast<'static>>,
    frames: Receiver<Result<FrameView, String>>,
    stop_requested: Arc<AtomicBool>,
    dropped_frames: Arc<AtomicU64>,
    worker: Option<JoinHandle<()>>,
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
                false,
                None,
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
            .first()
            .ok_or_else(|| anyhow!("portal returned no PipeWire streams"))?;
        let node_id = stream.pipe_wire_node_id();
        let fd = proxy
            .open_pipe_wire_remote(&session)
            .await
            .context("open_pipe_wire_remote")?;

        let (frame_tx, frames) = bounded(1);
        let eviction_receiver = frames.clone();
        let (setup_tx, setup_rx) = bounded(1);
        let stop_requested = Arc::new(AtomicBool::new(false));
        let dropped_frames = Arc::new(AtomicU64::new(0));
        let worker_stop = stop_requested.clone();
        let worker_dropped = dropped_frames.clone();
        let worker = thread::Builder::new()
            .name("liveblock-pipewire".into())
            .spawn(move || {
                run_pipewire(
                    fd,
                    node_id,
                    frame_tx,
                    eviction_receiver,
                    setup_tx,
                    worker_stop,
                    worker_dropped,
                )
            })
            .context("spawn PipeWire capture thread")?;
        let mut worker_guard = PipeWireWorkerGuard {
            stop_requested: stop_requested.clone(),
            worker: Some(worker),
        };
        let setup =
            tokio::task::spawn_blocking(move || setup_rx.recv_timeout(Duration::from_secs(10)))
                .await
                .context("join PipeWire setup waiter")?
                .map_err(|_| anyhow!("PipeWire setup timed out"))?;
        if let Err(error) = setup {
            let _ = session.close().await;
            return Err(anyhow!(error));
        }
        let worker = worker_guard
            .worker
            .take()
            .ok_or_else(|| anyhow!("PipeWire worker disappeared during setup"))?;

        Ok(Self {
            node_id,
            _portal: proxy,
            _session: session,
            frames,
            stop_requested,
            dropped_frames,
            worker: Some(worker),
        })
    }

    fn stop_worker(&mut self) {
        self.stop_requested.store(true, Ordering::Release);
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

impl Drop for WaylandCapture {
    fn drop(&mut self) {
        self.stop_worker();
    }
}

#[async_trait::async_trait]
impl CaptureSource for WaylandCapture {
    async fn next_frame(&mut self) -> Result<FrameView> {
        loop {
            match self.frames.try_recv() {
                Ok(Ok(frame)) => return Ok(frame),
                Ok(Err(error)) => return Err(anyhow!(error)),
                Err(crossbeam_channel::TryRecvError::Disconnected) => {
                    return Err(anyhow!("PipeWire capture thread stopped"));
                }
                Err(crossbeam_channel::TryRecvError::Empty) => {
                    tokio::time::sleep(Duration::from_millis(4)).await;
                }
            }
        }
    }

    fn dropped_frames(&self) -> u64 {
        self.dropped_frames.load(Ordering::Relaxed)
    }

    async fn stop(&mut self) {
        self.stop_worker();
        let _ = self._session.close().await;
    }
}

struct PipeWireUserData {
    format: spa::param::video::VideoInfoRaw,
}

fn run_pipewire(
    fd: OwnedFd,
    node_id: u32,
    frame_tx: Sender<Result<FrameView, String>>,
    eviction_receiver: Receiver<Result<FrameView, String>>,
    setup_tx: Sender<Result<(), String>>,
    stop_requested: Arc<AtomicBool>,
    dropped_frames: Arc<AtomicU64>,
) {
    let result = (|| -> Result<()> {
        pw::init();
        let mainloop = pw::main_loop::MainLoop::new(None).context("create PipeWire main loop")?;
        let context = pw::context::Context::new(&mainloop).context("create PipeWire context")?;
        let core = context
            .connect_fd(fd, None)
            .context("connect portal PipeWire fd")?;
        let stream = pw::stream::Stream::new(
            &core,
            "liveblock-screen-capture",
            properties! {
                *pw::keys::MEDIA_TYPE => "Video",
                *pw::keys::MEDIA_CATEGORY => "Capture",
                *pw::keys::MEDIA_ROLE => "Screen",
            },
        )
        .context("create PipeWire stream")?;

        let process_tx = frame_tx.clone();
        let process_eviction = eviction_receiver.clone();
        let process_dropped = dropped_frames.clone();
        let error_tx = frame_tx.clone();
        let error_eviction = eviction_receiver.clone();
        let error_dropped = dropped_frames.clone();
        let _listener = stream
            .add_local_listener_with_user_data(PipeWireUserData {
                format: Default::default(),
            })
            .state_changed(move |_, _, _, state| {
                if let pw::stream::StreamState::Error(error) = state {
                    send_latest(
                        &error_tx,
                        &error_eviction,
                        &error_dropped,
                        Err(format!("PipeWire stream error: {error}")),
                    );
                }
            })
            .param_changed(|_, user_data, id, param| {
                let Some(param) = param else {
                    return;
                };
                if id != spa::param::ParamType::Format.as_raw() {
                    return;
                }
                let Ok((media_type, media_subtype)) = spa::param::format_utils::parse_format(param)
                else {
                    return;
                };
                if media_type != spa::param::format::MediaType::Video
                    || media_subtype != spa::param::format::MediaSubtype::Raw
                {
                    return;
                }
                let _ = user_data.format.parse(param);
            })
            .process(move |stream, user_data| {
                let Some(mut buffer) = stream.dequeue_buffer() else {
                    return;
                };
                let size = user_data.format.size();
                if size.width == 0 || size.height == 0 {
                    return;
                }
                let format = match RawVideoFormat::from_spa(user_data.format.format()) {
                    Ok(format) => format,
                    Err(error) => {
                        send_latest(
                            &process_tx,
                            &process_eviction,
                            &process_dropped,
                            Err(error.to_string()),
                        );
                        return;
                    }
                };
                let mut planes = Vec::new();
                for data in buffer.datas_mut() {
                    let (offset, size, stride) = {
                        let chunk = data.chunk();
                        (
                            chunk.offset() as usize,
                            chunk.size() as usize,
                            chunk.stride().unsigned_abs() as usize,
                        )
                    };
                    let Some(mapped) = data.data() else {
                        continue;
                    };
                    let end = offset.saturating_add(size).min(mapped.len());
                    if offset >= end {
                        continue;
                    }
                    planes.push(RawPlane {
                        bytes: mapped[offset..end].to_vec(),
                        stride,
                    });
                }
                match convert_to_bgra(format, size.width, size.height, &planes) {
                    Ok(pixels) => send_latest(
                        &process_tx,
                        &process_eviction,
                        &process_dropped,
                        Ok(FrameView {
                            pixels: Arc::from(pixels),
                            width: size.width,
                            height: size.height,
                            stride: size.width * 4,
                        }),
                    ),
                    Err(error) => send_latest(
                        &process_tx,
                        &process_eviction,
                        &process_dropped,
                        Err(error.to_string()),
                    ),
                }
            })
            .register()
            .context("register PipeWire listener")?;

        let object = spa::pod::object!(
            spa::utils::SpaTypes::ObjectParamFormat,
            spa::param::ParamType::EnumFormat,
            spa::pod::property!(
                spa::param::format::FormatProperties::MediaType,
                Id,
                spa::param::format::MediaType::Video
            ),
            spa::pod::property!(
                spa::param::format::FormatProperties::MediaSubtype,
                Id,
                spa::param::format::MediaSubtype::Raw
            ),
            spa::pod::property!(
                spa::param::format::FormatProperties::VideoFormat,
                Choice,
                Enum,
                Id,
                spa::param::video::VideoFormat::BGRx,
                spa::param::video::VideoFormat::BGRx,
                spa::param::video::VideoFormat::BGRA,
                spa::param::video::VideoFormat::NV12,
                spa::param::video::VideoFormat::YUY2
            ),
            spa::pod::property!(
                spa::param::format::FormatProperties::VideoSize,
                Choice,
                Range,
                Rectangle,
                spa::utils::Rectangle {
                    width: 1920,
                    height: 1080
                },
                spa::utils::Rectangle {
                    width: 1,
                    height: 1
                },
                spa::utils::Rectangle {
                    width: 16384,
                    height: 16384
                }
            ),
            spa::pod::property!(
                spa::param::format::FormatProperties::VideoFramerate,
                Choice,
                Range,
                Fraction,
                spa::utils::Fraction { num: 30, denom: 1 },
                spa::utils::Fraction { num: 0, denom: 1 },
                spa::utils::Fraction { num: 60, denom: 1 }
            )
        );
        let values = spa::pod::serialize::PodSerializer::serialize(
            std::io::Cursor::new(Vec::new()),
            &spa::pod::Value::Object(object),
        )
        .map_err(|error| anyhow!("serialize PipeWire format pod: {error:?}"))?
        .0
        .into_inner();
        let mut params =
            [Pod::from_bytes(&values).ok_or_else(|| anyhow!("invalid PipeWire format pod"))?];
        stream
            .connect(
                spa::utils::Direction::Input,
                Some(node_id),
                pw::stream::StreamFlags::AUTOCONNECT | pw::stream::StreamFlags::MAP_BUFFERS,
                &mut params,
            )
            .context("connect PipeWire portal stream")?;
        let _ = setup_tx.send(Ok(()));
        while !stop_requested.load(Ordering::Acquire) {
            mainloop.loop_().iterate(Duration::from_millis(50));
        }
        stream.disconnect().context("disconnect PipeWire stream")?;
        Ok(())
    })();
    if let Err(error) = result {
        let message = error.to_string();
        let _ = setup_tx.try_send(Err(message.clone()));
        send_latest(&frame_tx, &eviction_receiver, &dropped_frames, Err(message));
    }
}

fn send_latest(
    sender: &Sender<Result<FrameView, String>>,
    eviction_receiver: &Receiver<Result<FrameView, String>>,
    dropped_frames: &AtomicU64,
    item: Result<FrameView, String>,
) {
    match sender.try_send(item) {
        Ok(()) | Err(TrySendError::Disconnected(_)) => {}
        Err(TrySendError::Full(item)) => {
            if eviction_receiver.try_recv().is_ok() {
                dropped_frames.fetch_add(1, Ordering::Relaxed);
            }
            let _ = sender.try_send(item);
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RawVideoFormat {
    Bgra,
    Bgrx,
    Nv12,
    Yuy2,
}

impl RawVideoFormat {
    fn from_spa(format: spa::param::video::VideoFormat) -> Result<Self> {
        if format == spa::param::video::VideoFormat::BGRA {
            Ok(Self::Bgra)
        } else if format == spa::param::video::VideoFormat::BGRx {
            Ok(Self::Bgrx)
        } else if format == spa::param::video::VideoFormat::NV12 {
            Ok(Self::Nv12)
        } else if format == spa::param::video::VideoFormat::YUY2 {
            Ok(Self::Yuy2)
        } else {
            Err(anyhow!("unsupported PipeWire pixel format {format:?}"))
        }
    }
}

struct RawPlane {
    bytes: Vec<u8>,
    stride: usize,
}

fn convert_to_bgra(
    format: RawVideoFormat,
    width: u32,
    height: u32,
    planes: &[RawPlane],
) -> Result<Vec<u8>> {
    if width == 0 || height == 0 {
        return Err(anyhow!("empty PipeWire frame"));
    }
    match format {
        RawVideoFormat::Bgra | RawVideoFormat::Bgrx => convert_packed_bgr(width, height, planes),
        RawVideoFormat::Yuy2 => convert_yuy2(width, height, planes),
        RawVideoFormat::Nv12 => convert_nv12(width, height, planes),
    }
}

fn convert_packed_bgr(width: u32, height: u32, planes: &[RawPlane]) -> Result<Vec<u8>> {
    let plane = planes
        .first()
        .ok_or_else(|| anyhow!("PipeWire frame has no data plane"))?;
    let stride = plane.stride.max(width as usize * 4);
    copy_rows(
        &plane.bytes,
        width,
        height,
        stride,
        width as usize * 4,
        |src, dst| {
            dst.copy_from_slice(src);
            for alpha in dst[3..].iter_mut().step_by(4) {
                *alpha = 255;
            }
        },
    )
}

fn convert_yuy2(width: u32, height: u32, planes: &[RawPlane]) -> Result<Vec<u8>> {
    if width % 2 != 0 {
        return Err(anyhow!("YUY2 width must be even"));
    }
    let plane = planes
        .first()
        .ok_or_else(|| anyhow!("PipeWire frame has no YUY2 plane"))?;
    let stride = plane.stride.max(width as usize * 2);
    let required = stride
        .checked_mul(height as usize)
        .ok_or_else(|| anyhow!("YUY2 dimensions overflow"))?;
    if plane.bytes.len() < required {
        return Err(anyhow!("truncated YUY2 frame"));
    }
    let mut output = vec![0u8; width as usize * height as usize * 4];
    for y in 0..height as usize {
        let row = &plane.bytes[y * stride..y * stride + width as usize * 2];
        for x in (0..width as usize).step_by(2) {
            let offset = x * 2;
            write_yuv(
                &mut output,
                (y * width as usize + x) * 4,
                row[offset],
                row[offset + 1],
                row[offset + 3],
            );
            write_yuv(
                &mut output,
                (y * width as usize + x + 1) * 4,
                row[offset + 2],
                row[offset + 1],
                row[offset + 3],
            );
        }
    }
    Ok(output)
}

fn convert_nv12(width: u32, height: u32, planes: &[RawPlane]) -> Result<Vec<u8>> {
    if width % 2 != 0 || height % 2 != 0 {
        return Err(anyhow!("NV12 dimensions must be even"));
    }
    let y_plane = planes
        .first()
        .ok_or_else(|| anyhow!("PipeWire frame has no NV12 luma plane"))?;
    let y_stride = y_plane.stride.max(width as usize);
    let y_len = y_stride * height as usize;
    let (uv_bytes, uv_stride) = if let Some(uv) = planes.get(1) {
        (&uv.bytes[..], uv.stride.max(width as usize))
    } else {
        if y_plane.bytes.len() < y_len {
            return Err(anyhow!("truncated NV12 luma plane"));
        }
        (&y_plane.bytes[y_len..], y_stride)
    };
    if y_plane.bytes.len() < y_len || uv_bytes.len() < uv_stride * (height as usize / 2) {
        return Err(anyhow!("truncated NV12 frame"));
    }
    let mut output = vec![0u8; width as usize * height as usize * 4];
    for y in 0..height as usize {
        for x in 0..width as usize {
            let uv = (y / 2) * uv_stride + (x / 2) * 2;
            write_yuv(
                &mut output,
                (y * width as usize + x) * 4,
                y_plane.bytes[y * y_stride + x],
                uv_bytes[uv],
                uv_bytes[uv + 1],
            );
        }
    }
    Ok(output)
}

fn copy_rows<F>(
    source: &[u8],
    width: u32,
    height: u32,
    stride: usize,
    row_bytes: usize,
    mut copy: F,
) -> Result<Vec<u8>>
where
    F: FnMut(&[u8], &mut [u8]),
{
    let required = stride
        .checked_mul(height as usize)
        .ok_or_else(|| anyhow!("frame dimensions overflow"))?;
    if stride < row_bytes || source.len() < required {
        return Err(anyhow!("truncated packed PipeWire frame"));
    }
    let mut output = vec![0u8; width as usize * height as usize * 4];
    for y in 0..height as usize {
        copy(
            &source[y * stride..y * stride + row_bytes],
            &mut output[y * row_bytes..(y + 1) * row_bytes],
        );
    }
    Ok(output)
}

fn write_yuv(output: &mut [u8], offset: usize, y: u8, u: u8, v: u8) {
    let c = i32::from(y).saturating_sub(16);
    let d = i32::from(u) - 128;
    let e = i32::from(v) - 128;
    let clamp = |value: i32| ((value + 128) >> 8).clamp(0, 255) as u8;
    let r = clamp(298 * c + 409 * e);
    let g = clamp(298 * c - 100 * d - 208 * e);
    let b = clamp(298 * c + 516 * d);
    output[offset..offset + 4].copy_from_slice(&[b, g, r, 255]);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strips_bgra_row_padding_and_forces_opaque_alpha() {
        let planes = [RawPlane {
            bytes: vec![1, 2, 3, 0, 9, 9, 9, 9],
            stride: 8,
        }];
        assert_eq!(
            convert_to_bgra(RawVideoFormat::Bgra, 1, 1, &planes).unwrap(),
            [1, 2, 3, 255]
        );
    }

    #[test]
    fn converts_yuy2_black_and_white_pair() {
        let planes = [RawPlane {
            bytes: vec![16, 128, 235, 128],
            stride: 4,
        }];
        assert_eq!(
            convert_to_bgra(RawVideoFormat::Yuy2, 2, 1, &planes).unwrap(),
            [0, 0, 0, 255, 255, 255, 255, 255]
        );
    }

    #[test]
    fn converts_two_plane_nv12_neutral_luma() {
        let planes = [
            RawPlane {
                bytes: vec![16, 235, 81, 145],
                stride: 2,
            },
            RawPlane {
                bytes: vec![128, 128],
                stride: 2,
            },
        ];
        let output = convert_to_bgra(RawVideoFormat::Nv12, 2, 2, &planes).unwrap();
        assert_eq!(&output[0..4], &[0, 0, 0, 255]);
        assert_eq!(&output[4..8], &[255, 255, 255, 255]);
    }
}
