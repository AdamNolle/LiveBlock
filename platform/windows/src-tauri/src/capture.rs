//! Windows.Graphics.Capture pipeline using windows-rs.
//!
//! Sequence:
//!   1. Create D3D11 device.
//!   2. Wrap it as IDirect3DDevice (WinRT) via CreateDirect3D11DeviceFromDXGIDevice.
//!   3. Create a GraphicsCaptureItem from the chosen monitor (interop factory).
//!   4. Create Direct3D11CaptureFramePool::CreateFreeThreaded.
//!   5. Hook FrameArrived. On the WGC thread we do the **minimum**: a
//!      GPU->staging CopyResource + a fast row-packed readback into a pooled
//!      buffer, publish it via `arc_swap`, and wake the worker through a bounded
//!      channel. ALL heavy work (detect -> track -> mask -> present) runs on the
//!      worker thread, never on FrameArrived.
//!
//! Why the readback still happens here: the ONNX detector and the DRM black-out
//! probe both need CPU pixels, and the D3D11 *immediate* context is
//! single-threaded — sharing the GPU texture to another thread/device would need
//! shared-handle plumbing. So FrameArrived performs only the cheap memcpy
//! readback into a recycled staging texture + scratch buffer (no per-frame GPU
//! or Vec allocation on the hot path) and hands ownership off; the entire
//! detect -> track -> mask -> present chain is off-thread on the worker.
//!
//! TODO(windows-port): the fully zero-readback realtime path binds the WGC
//! `ID3D11Texture2D` straight into the DirectML EP as a GPU input tensor (ort
//! I/O binding), so the detector never touches CPU pixels at all; the CPU copy
//! would then exist ONLY behind the labeling screenshot path. That needs ort
//! GPU-tensor I/O binding wired against the WGC device and is a follow-up.
//!
//! Throttle to 30 Hz to match macOS `lastEmitClock`. The newest BGRA frame is
//! published into the SHARED `arc_swap::ArcSwapOption` (owned by `AppState`) so
//! the worker and the labeling screenshot path both read the freshest frame
//! without copying twice; the worker is woken by a bounded channel tick.

use anyhow::{anyhow, Context, Result};
use arc_swap::ArcSwapOption;
use crossbeam_channel::{bounded, Receiver, Sender};
use parking_lot::Mutex;
use std::sync::Arc;
use std::time::{Duration, Instant};

// NOTE: windows-rs 0.58 folded `ComInterface` into `Interface` — importing
// `ComInterface` no longer compiles. `.cast()` / `GetInterface()` come from
// `Interface`.
use windows::core::{IInspectable, Interface};
use windows::Foundation::TypedEventHandler;
use windows::Graphics::Capture::{Direct3D11CaptureFramePool, GraphicsCaptureItem, GraphicsCaptureSession};
use windows::Graphics::DirectX::Direct3D11::IDirect3DDevice;
use windows::Graphics::DirectX::DirectXPixelFormat;
use windows::Win32::Foundation::HMODULE;
use windows::Win32::Graphics::Direct3D::{D3D_DRIVER_TYPE_HARDWARE, D3D_FEATURE_LEVEL_11_0};
use windows::Win32::Graphics::Direct3D11::{
    D3D11CreateDevice, ID3D11Device, ID3D11DeviceContext, ID3D11Texture2D, D3D11_CPU_ACCESS_READ,
    D3D11_CREATE_DEVICE_BGRA_SUPPORT, D3D11_MAPPED_SUBRESOURCE, D3D11_MAP_READ, D3D11_SDK_VERSION,
    D3D11_TEXTURE2D_DESC, D3D11_USAGE_STAGING,
};
use windows::Win32::Graphics::Dxgi::Common::{DXGI_FORMAT_B8G8R8A8_UNORM, DXGI_SAMPLE_DESC};
use windows::Win32::Graphics::Dxgi::IDXGIDevice;
use windows::Win32::Graphics::Gdi::HMONITOR;
use windows::Win32::System::WinRT::Direct3D11::CreateDirect3D11DeviceFromDXGIDevice;
use windows::Win32::System::WinRT::Graphics::Capture::IGraphicsCaptureItemInterop;

/// One CPU-readable BGRA frame, tightly packed (row stride = width * 4).
#[derive(Clone)]
pub struct FrameView {
    pub width: u32,
    pub height: u32,
    /// Monotonic index since capture start.
    pub index: u64,
    pub bytes: Arc<Vec<u8>>,
}

/// Wakes the worker that a fresh frame is ready in `latest`. Carries only the
/// index so the channel stays tiny; the worker pulls the actual pixels from the
/// arc-swap (always the freshest, so a slow worker naturally drops stale frames).
pub type FrameTick = u64;

pub struct CaptureSession {
    _item: GraphicsCaptureItem,
    _frame_pool: Direct3D11CaptureFramePool,
    session: GraphicsCaptureSession,
    _d3d_device: ID3D11Device,
    _d3d_context: ID3D11DeviceContext,
    /// Worker wake channel (bounded -> backpressure: a full channel just means
    /// the worker is behind, and we drop the tick rather than queueing latency).
    tick_rx: Receiver<FrameTick>,
}

impl CaptureSession {
    /// Start capturing `monitor`, publishing each readback into the shared
    /// `latest` arc-swap. Returns a session plus a `Receiver` the worker loop
    /// selects on; each `FrameTick` means `latest` holds a new frame.
    pub fn start(monitor: HMONITOR, latest: Arc<ArcSwapOption<FrameView>>) -> Result<Self> {
        if monitor.0.is_null() {
            return Err(anyhow!("invalid HMONITOR"));
        }

        // 1. D3D11 device.
        let mut d3d_device: Option<ID3D11Device> = None;
        let mut d3d_context: Option<ID3D11DeviceContext> = None;
        let feature_levels = [D3D_FEATURE_LEVEL_11_0];
        unsafe {
            D3D11CreateDevice(
                None,
                D3D_DRIVER_TYPE_HARDWARE,
                HMODULE::default(),
                D3D11_CREATE_DEVICE_BGRA_SUPPORT,
                Some(&feature_levels),
                D3D11_SDK_VERSION,
                Some(&mut d3d_device),
                None,
                Some(&mut d3d_context),
            )
            .context("D3D11CreateDevice")?;
        }
        let d3d_device = d3d_device.context("D3D11CreateDevice returned None")?;
        let d3d_context = d3d_context.context("D3D11CreateDevice returned None context")?;

        // 2. WinRT IDirect3DDevice wrapper.
        let dxgi_device: IDXGIDevice = d3d_device.cast().context("cast to IDXGIDevice")?;
        let inspectable: IInspectable =
            unsafe { CreateDirect3D11DeviceFromDXGIDevice(&dxgi_device)? };
        let direct3d_device: IDirect3DDevice = inspectable.cast()?;

        // 3. GraphicsCaptureItem from monitor.
        let interop: IGraphicsCaptureItemInterop =
            windows::core::factory::<GraphicsCaptureItem, IGraphicsCaptureItemInterop>()?;
        let item: GraphicsCaptureItem = unsafe { interop.CreateForMonitor(monitor)? };

        // 4. Frame pool.
        let size = item.Size()?;
        let pool = Direct3D11CaptureFramePool::CreateFreeThreaded(
            &direct3d_device,
            DirectXPixelFormat::B8G8R8A8UIntNormalized,
            2,
            size,
        )?;
        let session = pool.CreateCaptureSession(&item)?;

        // 5. Frame-arrived handler — readback only; wake the worker via channel.
        // `latest` is the shared arc-swap provided by the caller.
        let last_emit = Arc::new(Mutex::new(Instant::now() - Duration::from_secs(1)));
        // A recycled staging texture + reusable readback buffer avoid per-frame
        // GPU/CPU allocations on the hot path.
        let staging: Arc<Mutex<Option<ID3D11Texture2D>>> = Arc::new(Mutex::new(None));
        let scratch: Arc<Mutex<Vec<u8>>> = Arc::new(Mutex::new(Vec::new()));
        let frame_index = Arc::new(Mutex::new(0u64));

        let (tick_tx, tick_rx): (Sender<FrameTick>, Receiver<FrameTick>) = bounded(2);

        let device_clone = d3d_device.clone();
        let context_clone = d3d_context.clone();
        let latest_clone = latest.clone();
        let last_emit_clone = last_emit.clone();
        let staging_clone = staging.clone();
        let scratch_clone = scratch.clone();
        let index_clone = frame_index.clone();

        pool.FrameArrived(&TypedEventHandler::<
            Direct3D11CaptureFramePool,
            windows::core::IInspectable,
        >::new(move |sender, _| {
            // SAFETY: this closure runs on the WGC free-threaded pool thread; the
            // D3D11 immediate context is touched only here.
            if let Some(pool_ref) = sender {
                if let Ok(frame) = pool_ref.TryGetNextFrame() {
                    // Throttle to ~30 Hz to match macOS.
                    let mut last = last_emit_clone.lock();
                    if last.elapsed() < Duration::from_millis(33) {
                        return Ok(());
                    }
                    *last = Instant::now();
                    drop(last);

                    let idx = {
                        let mut g = index_clone.lock();
                        *g = g.wrapping_add(1);
                        *g
                    };

                    if let Ok(view) = readback_frame(
                        &device_clone,
                        &context_clone,
                        &frame,
                        idx,
                        &staging_clone,
                        &scratch_clone,
                    ) {
                        latest_clone.store(Some(Arc::new(view)));
                        // Wake the worker; drop the tick if it's still busy.
                        let _ = tick_tx.try_send(idx);
                    }
                }
            }
            Ok(())
        }))?;

        session.StartCapture()?;
        Ok(Self {
            _item: item,
            _frame_pool: pool,
            session,
            _d3d_device: d3d_device,
            _d3d_context: d3d_context,
            tick_rx,
        })
    }

    /// The worker's wake channel. Each received tick means the shared `latest`
    /// arc-swap (passed to [`start`](Self::start)) holds a fresh frame. The
    /// caller reads the pixels from that arc-swap (also used by the labeling
    /// screenshot path), so there is exactly one published frame.
    pub fn ticks(&self) -> Receiver<FrameTick> {
        self.tick_rx.clone()
    }

    pub fn stop(self) {
        let _ = self.session.Close();
    }
}

/// Copy the WGC frame to a recycled CPU-readable staging texture, map it, and
/// pack BGRA into a recycled scratch buffer (honoring RowPitch). Returns a
/// `FrameView` whose `bytes` is a fresh Arc (so the publish is lock-free for
/// readers) while the scratch buffer itself is reused next frame.
fn readback_frame(
    device: &ID3D11Device,
    context: &ID3D11DeviceContext,
    frame: &windows::Graphics::Capture::Direct3D11CaptureFrame,
    index: u64,
    staging_slot: &Mutex<Option<ID3D11Texture2D>>,
    scratch: &Mutex<Vec<u8>>,
) -> Result<FrameView> {
    use windows::Win32::System::WinRT::Direct3D11::IDirect3DDxgiInterfaceAccess;

    let surface = frame.Surface()?;
    let access: IDirect3DDxgiInterfaceAccess = surface.cast()?;
    let frame_tex: ID3D11Texture2D = unsafe { access.GetInterface()? };

    let mut desc = D3D11_TEXTURE2D_DESC::default();
    unsafe { frame_tex.GetDesc(&mut desc) };
    let width = desc.Width;
    let height = desc.Height;

    // (Re)create the staging texture only when the size changed.
    let mut slot = staging_slot.lock();
    let need_new = match slot.as_ref() {
        Some(tex) => {
            let mut sd = D3D11_TEXTURE2D_DESC::default();
            unsafe { tex.GetDesc(&mut sd) };
            sd.Width != width || sd.Height != height
        }
        None => true,
    };
    if need_new {
        let staging_desc = D3D11_TEXTURE2D_DESC {
            Width: width,
            Height: height,
            MipLevels: 1,
            ArraySize: 1,
            Format: DXGI_FORMAT_B8G8R8A8_UNORM,
            SampleDesc: DXGI_SAMPLE_DESC { Count: 1, Quality: 0 },
            Usage: D3D11_USAGE_STAGING,
            BindFlags: 0,
            CPUAccessFlags: D3D11_CPU_ACCESS_READ.0 as u32,
            MiscFlags: 0,
        };
        let mut tex: Option<ID3D11Texture2D> = None;
        unsafe { device.CreateTexture2D(&staging_desc, None, Some(&mut tex))? };
        *slot = Some(tex.context("CreateTexture2D returned None")?);
    }
    let staging = slot.as_ref().expect("staging present");

    unsafe { context.CopyResource(staging, &frame_tex) };

    let mut mapped = D3D11_MAPPED_SUBRESOURCE::default();
    unsafe { context.Map(staging, 0, D3D11_MAP_READ, 0, Some(&mut mapped))? };

    let row_bytes = (width as usize) * 4;
    let total = row_bytes * height as usize;
    let mut buf = scratch.lock();
    if buf.len() != total {
        buf.resize(total, 0);
    }
    unsafe {
        let src_base = mapped.pData as *const u8;
        let row_pitch = mapped.RowPitch as usize;
        for row in 0..height as usize {
            let src = src_base.add(row * row_pitch);
            let dst = buf.as_mut_ptr().add(row * row_bytes);
            std::ptr::copy_nonoverlapping(src, dst, row_bytes);
        }
        context.Unmap(staging, 0);
    }

    // Publish a fresh Arc<Vec> snapshot (cheap clone of the reused scratch).
    let bytes = Arc::new(buf.clone());
    drop(buf);
    drop(slot);

    Ok(FrameView { width, height, index, bytes })
}

/// Enumerate monitors. Returns (HMONITOR, friendly description). Used by the
/// control panel to populate a "select display" combo box.
pub fn enumerate_monitors() -> Vec<(HMONITOR, String)> {
    use windows::Win32::Foundation::{BOOL, LPARAM, RECT};
    use windows::Win32::Graphics::Gdi::{EnumDisplayMonitors, HDC};

    extern "system" fn cb(monitor: HMONITOR, _hdc: HDC, _rect: *mut RECT, lparam: LPARAM) -> BOOL {
        let acc = unsafe { &mut *(lparam.0 as *mut Vec<(HMONITOR, String)>) };
        acc.push((monitor, format!("Display {}", acc.len() + 1)));
        BOOL(1)
    }

    let mut acc: Vec<(HMONITOR, String)> = Vec::new();
    unsafe {
        let _ = EnumDisplayMonitors(
            HDC::default(),
            None,
            Some(cb),
            LPARAM(&mut acc as *mut _ as isize),
        );
    }
    acc
}
