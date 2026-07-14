//! Windows.Graphics.Capture pipeline using windows-rs.
//!
//! Sequence:
//!   1. Create D3D11 device.
//!   2. Wrap it as IDirect3DDevice (WinRT) via CreateDirect3D11DeviceFromDXGIDevice.
//!   3. Create a GraphicsCaptureItem from the chosen monitor (interop factory).
//!   4. Create Direct3D11CaptureFramePool::CreateFreeThreaded.
//!   5. Hook FrameArrived; copy frame to a CPU-readable staging texture; map; emit.
//!
//! Throttle to 30 Hz to match macOS `lastEmitClock`. The newest BGRA frame is
//! retained in `arc_swap::ArcSwap` so `capture_screenshot_for_labeling` can read it.

use anyhow::{anyhow, Context, Result};
use arc_swap::ArcSwapOption;
use crossbeam_channel::{bounded, RecvTimeoutError};
use parking_lot::Mutex;
use std::sync::{
    atomic::{AtomicBool, AtomicU64, Ordering},
    Arc,
};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use crate::capture_policy::{
    enqueue_latest, CaptureTelemetry, CaptureTelemetrySnapshot, ConsecutiveFailureGate,
};
use windows::core::{AgileReference, IInspectable, Interface};
use windows::Foundation::{EventRegistrationToken, TypedEventHandler};
use windows::Graphics::Capture::{
    Direct3D11CaptureFramePool, GraphicsCaptureItem, GraphicsCaptureSession,
};
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

/// One CPU-readable BGRA frame.
#[derive(Clone)]
pub struct FrameView {
    pub width: u32,
    pub height: u32,
    pub bytes: Arc<Vec<u8>>, // tightly packed BGRA, row stride = width * 4
}

pub type FrameCallback = Arc<dyn Fn(&FrameView) + Send + Sync + 'static>;
pub type CaptureErrorCallback = Arc<dyn Fn(String) + Send + Sync + 'static>;

pub struct CaptureSession {
    _item: GraphicsCaptureItem,
    _frame_pool: Direct3D11CaptureFramePool,
    session: GraphicsCaptureSession,
    _d3d_device: ID3D11Device,
    _d3d_context: ID3D11DeviceContext,
    /// Latest frame; drained by `latest_frame()` for the labeling screenshot path.
    pub latest: Arc<ArcSwapOption<FrameView>>,
    /// 30-Hz throttle; mirrors macOS `lastEmitClock`.
    last_emit: Arc<Mutex<Instant>>,
    frame_token: EventRegistrationToken,
    closed_token: EventRegistrationToken,
    stop_worker: Arc<AtomicBool>,
    worker: Option<JoinHandle<()>>,
    telemetry: Arc<CaptureTelemetry>,
}

impl CaptureSession {
    pub fn start(
        monitor: HMONITOR,
        on_frame: FrameCallback,
        on_error: CaptureErrorCallback,
        telemetry: Arc<CaptureTelemetry>,
    ) -> Result<Self> {
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

        // 5. The WGC callback performs only the unavoidable GPU→CPU copy and
        // newest-frame enqueue. Detection/inpainting runs on one worker so slow
        // inference never stalls frame-pool delivery.
        let latest = Arc::new(ArcSwapOption::<FrameView>::from(None));
        let last_emit = Arc::new(Mutex::new(Instant::now() - Duration::from_secs(1)));
        let stop_worker = Arc::new(AtomicBool::new(false));
        let (frame_sender, frame_receiver) = bounded::<FrameView>(1);
        let eviction_receiver = frame_receiver.clone();
        let worker_stop = stop_worker.clone();
        let worker_telemetry = telemetry.clone();
        let worker_error = on_error.clone();
        let worker = thread::Builder::new()
            .name("liveblock-frame-processor".into())
            .spawn(move || {
                while !worker_stop.load(Ordering::Acquire) {
                    match frame_receiver.recv_timeout(Duration::from_millis(100)) {
                        Ok(frame) => {
                            let result =
                                std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                                    on_frame(&frame)
                                }));
                            if result.is_err() {
                                worker_error(
                                    "frame processor panicked; capture stopped processing".into(),
                                );
                                break;
                            }
                            worker_telemetry.processed();
                        }
                        Err(RecvTimeoutError::Timeout) => {}
                        Err(RecvTimeoutError::Disconnected) => break,
                    }
                }
            })
            .context("spawn frame processor")?;

        let device_clone = d3d_device.clone();
        let direct3d_agile = AgileReference::new(&direct3d_device)?;
        let context_clone = d3d_context.clone();
        let latest_clone = latest.clone();
        let last_emit_clone = last_emit.clone();
        let telemetry_clone = telemetry.clone();
        let error_clone = on_error.clone();
        let failure_gate = Arc::new(Mutex::new(ConsecutiveFailureGate::default()));
        let failure_gate_clone = failure_gate.clone();
        let current_size = Arc::new(AtomicU64::new(pack_size(size.Width, size.Height)));
        let current_size_clone = current_size.clone();

        let frame_token = pool.FrameArrived(&TypedEventHandler::new(
            move |sender: &Option<Direct3D11CaptureFramePool>, _: &Option<IInspectable>| {
                // SAFETY: the immediate context is touched only by this free-threaded
                // callback. The downstream worker receives packed CPU bytes.
                let Some(pool_ref) = sender else {
                    return Ok(());
                };
                let result = (|| -> Result<()> {
                    let frame = pool_ref.TryGetNextFrame()?;
                    telemetry_clone.captured();
                    let mut last = last_emit_clone.lock();
                    if last.elapsed() < Duration::from_millis(33) {
                        return Ok(());
                    }
                    *last = Instant::now();
                    drop(last);

                    let content_size = frame.ContentSize()?;
                    let view = process_frame(&device_clone, &context_clone, &frame)?;
                    latest_clone.store(Some(Arc::new(view.clone())));

                    let next_size = pack_size(content_size.Width, content_size.Height);
                    if current_size_clone.load(Ordering::Acquire) != next_size {
                        let resize_device: IDirect3DDevice = direct3d_agile.resolve()?;
                        pool_ref.Recreate(
                            &resize_device,
                            DirectXPixelFormat::B8G8R8A8UIntNormalized,
                            2,
                            content_size,
                        )?;
                        current_size_clone.store(next_size, Ordering::Release);
                    }

                    enqueue_latest(&frame_sender, &eviction_receiver, view, &telemetry_clone);
                    failure_gate_clone.lock().success();
                    Ok(())
                })();
                if let Err(error) = result {
                    telemetry_clone.copy_error();
                    if failure_gate_clone.lock().failure() {
                        error_clone(format!("three consecutive capture-frame failures: {error}"));
                    }
                }
                Ok(())
            },
        ))?;

        let closed_error = on_error.clone();
        let closed_token = item.Closed(&TypedEventHandler::new(
            move |_: &Option<GraphicsCaptureItem>, _: &Option<IInspectable>| {
                closed_error("selected display capture item closed".into());
                Ok(())
            },
        ))?;

        session.StartCapture()?;
        Ok(Self {
            _item: item,
            _frame_pool: pool,
            session,
            _d3d_device: d3d_device,
            _d3d_context: d3d_context,
            latest,
            last_emit,
            frame_token,
            closed_token,
            stop_worker,
            worker: Some(worker),
            telemetry,
        })
    }

    pub fn latest_frame(&self) -> Option<FrameView> {
        self.latest.load_full().map(|a| (*a).clone())
    }

    pub fn telemetry(&self) -> CaptureTelemetrySnapshot {
        self.telemetry.snapshot()
    }

    pub fn stop(mut self) {
        self.stop_worker.store(true, Ordering::Release);
        let _ = self._frame_pool.RemoveFrameArrived(self.frame_token);
        let _ = self._item.RemoveClosed(self.closed_token);
        let _ = self.session.Close();
        let _ = self._frame_pool.Close();
        // Do not block panic/quit on arbitrary ORT or inpainting duration.
        // Dropping JoinHandle detaches the CPU-only worker; the generation guard
        // suppresses stale output and stop_worker makes it exit after current work.
        let _ = self.worker.take();
        self.latest.store(None);
    }
}

fn pack_size(width: i32, height: i32) -> u64 {
    (width as u32 as u64) << 32 | height as u32 as u64
}

/// Copy WGC frame to a CPU-readable staging texture, map, and produce a packed
/// BGRA buffer (stride = width*4). Respects D3D11_MAPPED_SUBRESOURCE.RowPitch.
fn process_frame(
    device: &ID3D11Device,
    context: &ID3D11DeviceContext,
    frame: &windows::Graphics::Capture::Direct3D11CaptureFrame,
) -> Result<FrameView> {
    use windows::Win32::System::WinRT::Direct3D11::IDirect3DDxgiInterfaceAccess;

    let surface = frame.Surface()?;
    let access: IDirect3DDxgiInterfaceAccess = surface.cast()?;
    let frame_tex: ID3D11Texture2D = unsafe { access.GetInterface()? };

    let mut desc = D3D11_TEXTURE2D_DESC::default();
    unsafe { frame_tex.GetDesc(&mut desc) };

    // Build a staging texture of the same shape.
    let staging_desc = D3D11_TEXTURE2D_DESC {
        Width: desc.Width,
        Height: desc.Height,
        MipLevels: 1,
        ArraySize: 1,
        Format: DXGI_FORMAT_B8G8R8A8_UNORM,
        SampleDesc: DXGI_SAMPLE_DESC {
            Count: 1,
            Quality: 0,
        },
        Usage: D3D11_USAGE_STAGING,
        BindFlags: 0,
        CPUAccessFlags: D3D11_CPU_ACCESS_READ.0 as u32,
        MiscFlags: 0,
    };
    let mut staging: Option<ID3D11Texture2D> = None;
    unsafe {
        device.CreateTexture2D(&staging_desc, None, Some(&mut staging))?;
    }
    let staging = staging.context("CreateTexture2D returned None")?;

    unsafe {
        context.CopyResource(&staging, &frame_tex);
    }

    // Map.
    let mut mapped = D3D11_MAPPED_SUBRESOURCE::default();
    unsafe {
        context.Map(&staging, 0, D3D11_MAP_READ, 0, Some(&mut mapped))?;
    }

    let width = desc.Width;
    let height = desc.Height;
    let row_bytes = (width as usize) * 4;
    let mut packed = vec![0u8; row_bytes * height as usize];
    unsafe {
        let src_base = mapped.pData as *const u8;
        let row_pitch = mapped.RowPitch as usize;
        for row in 0..height as usize {
            let src = src_base.add(row * row_pitch);
            let dst = packed.as_mut_ptr().add(row * row_bytes);
            std::ptr::copy_nonoverlapping(src, dst, row_bytes);
        }
        context.Unmap(&staging, 0);
    }

    Ok(FrameView {
        width,
        height,
        bytes: Arc::new(packed),
    })
}

#[derive(Debug, Clone)]
pub struct MonitorDescriptor {
    pub handle: HMONITOR,
    pub name: String,
    pub is_primary: bool,
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
    pub scale_factor: f64,
}

/// Enumerate active monitor rectangles in physical PerMonitorV2 pixels.
pub fn enumerate_monitors() -> Vec<MonitorDescriptor> {
    use windows::Win32::Foundation::{BOOL, LPARAM, RECT};
    use windows::Win32::Graphics::Gdi::{
        EnumDisplayMonitors, GetMonitorInfoW, HDC, MONITORINFOEXW, MONITORINFOF_PRIMARY,
    };
    use windows::Win32::UI::HiDpi::{GetDpiForMonitor, MDT_EFFECTIVE_DPI};

    extern "system" fn cb(monitor: HMONITOR, _hdc: HDC, _rect: *mut RECT, lparam: LPARAM) -> BOOL {
        let acc = unsafe { &mut *(lparam.0 as *mut Vec<MonitorDescriptor>) };
        let mut info = MONITORINFOEXW::default();
        info.monitorInfo.cbSize = std::mem::size_of::<MONITORINFOEXW>() as u32;
        if unsafe { GetMonitorInfoW(monitor, &mut info as *mut _ as *mut _) }.as_bool() {
            let rect = info.monitorInfo.rcMonitor;
            let device_end = info
                .szDevice
                .iter()
                .position(|unit| *unit == 0)
                .unwrap_or(info.szDevice.len());
            let name = String::from_utf16_lossy(&info.szDevice[..device_end]);
            let mut dpi_x = 96u32;
            let mut dpi_y = 96u32;
            let _ = unsafe { GetDpiForMonitor(monitor, MDT_EFFECTIVE_DPI, &mut dpi_x, &mut dpi_y) };
            acc.push(MonitorDescriptor {
                handle: monitor,
                name: if name.is_empty() {
                    "Windows display".into()
                } else {
                    name
                },
                is_primary: info.monitorInfo.dwFlags & MONITORINFOF_PRIMARY != 0,
                x: rect.left,
                y: rect.top,
                width: rect.right.saturating_sub(rect.left) as u32,
                height: rect.bottom.saturating_sub(rect.top) as u32,
                scale_factor: f64::from(dpi_x) / 96.0,
            });
        }
        BOOL(1)
    }

    let mut monitors = Vec::new();
    unsafe {
        let _ = EnumDisplayMonitors(
            HDC::default(),
            None,
            Some(cb),
            LPARAM(&mut monitors as *mut _ as isize),
        );
    }
    sort_monitors(&mut monitors);
    monitors
}

fn sort_monitors(monitors: &mut [MonitorDescriptor]) {
    monitors.sort_by_key(|monitor| {
        (
            !monitor.is_primary,
            monitor.y,
            monitor.x,
            monitor.name.clone(),
        )
    });
}

#[cfg(test)]
mod monitor_tests {
    use super::*;

    fn monitor(name: &str, primary: bool, x: i32, y: i32) -> MonitorDescriptor {
        MonitorDescriptor {
            handle: HMONITOR::default(),
            name: name.into(),
            is_primary: primary,
            x,
            y,
            width: 1920,
            height: 1080,
            scale_factor: 1.0,
        }
    }

    #[test]
    fn monitor_order_is_primary_then_stable_geometry() {
        let mut monitors = [
            monitor("right", false, 1920, 0),
            monitor("primary", true, 0, 0),
            monitor("left", false, -1920, 0),
        ];
        sort_monitors(&mut monitors);
        assert_eq!(
            monitors
                .iter()
                .map(|monitor| monitor.name.as_str())
                .collect::<Vec<_>>(),
            ["primary", "left", "right"]
        );
    }
}
