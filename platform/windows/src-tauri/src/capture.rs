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
use windows::Win32::Foundation::{BOOL, HMODULE};
use windows::Win32::Graphics::Direct3D::{D3D_DRIVER_TYPE_HARDWARE, D3D_FEATURE_LEVEL_11_0};
use windows::Win32::Graphics::Direct3D11::{
    D3D11CreateDevice, ID3D11Device, ID3D11DeviceContext, ID3D11Multithread, ID3D11Texture2D,
    D3D11_BIND_SHADER_RESOURCE, D3D11_CPU_ACCESS_READ, D3D11_CREATE_DEVICE_BGRA_SUPPORT,
    D3D11_MAPPED_SUBRESOURCE, D3D11_MAP_READ, D3D11_SDK_VERSION, D3D11_TEXTURE2D_DESC,
    D3D11_USAGE_DEFAULT, D3D11_USAGE_STAGING,
};
use windows::Win32::Graphics::Dxgi::Common::{DXGI_FORMAT_B8G8R8A8_UNORM, DXGI_SAMPLE_DESC};
use windows::Win32::Graphics::Dxgi::IDXGIDevice;
use windows::Win32::Graphics::Gdi::HMONITOR;
use windows::Win32::System::WinRT::Direct3D11::CreateDirect3D11DeviceFromDXGIDevice;
use windows::Win32::System::WinRT::Graphics::Capture::IGraphicsCaptureItemInterop;

/// An application-owned D3D11 copy of a WGC surface. The frame-pool-owned
/// texture is never retained after its callback returns.
#[derive(Clone)]
pub struct GpuFrameResource {
    pub texture: ID3D11Texture2D,
    pub device: ID3D11Device,
    pub context: ID3D11DeviceContext,
}

/// One packed CPU BGRA frame plus its optional application-owned GPU source.
/// CPU bytes remain authoritative for protected-frame checks, labeling, and
/// fallback; the GPU resource allows compute work without re-uploading pixels.
#[derive(Clone)]
pub struct FrameView {
    pub width: u32,
    pub height: u32,
    pub bytes: Arc<Vec<u8>>, // tightly packed BGRA, row stride = width * 4
    pub gpu: Option<Arc<GpuFrameResource>>,
}

struct PendingGpuFrame {
    width: u32,
    height: u32,
    resource: Arc<GpuFrameResource>,
}

struct ReadbackStaging {
    width: u32,
    height: u32,
    texture: ID3D11Texture2D,
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
    frame_token: EventRegistrationToken,
    closed_token: EventRegistrationToken,
    stop_worker: Arc<AtomicBool>,
    first_frame_seen: Arc<AtomicBool>,
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
        // The callback submits a fast copy while the frame worker performs the
        // staging readback and optional compute work. Protect the immediate
        // context because those calls can occur on different threads.
        let multithread: ID3D11Multithread = d3d_context.cast()?;
        unsafe {
            let _ = multithread.SetMultithreadProtected(BOOL(1));
        }

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

        // 5. The WGC callback copies the pool-owned surface into an
        // application-owned shader-readable texture and enqueues only the
        // newest texture. GPU→CPU staging, protected checks, detection, and
        // inpainting all run on the worker, so Map/readback cannot stall WGC.
        let latest = Arc::new(ArcSwapOption::<FrameView>::from(None));
        let last_emit = Arc::new(Mutex::new(Instant::now() - Duration::from_secs(1)));
        let stop_worker = Arc::new(AtomicBool::new(false));
        let first_frame_seen = Arc::new(AtomicBool::new(false));
        let (frame_sender, frame_receiver) = bounded::<PendingGpuFrame>(1);
        let eviction_receiver = frame_receiver.clone();
        let worker_stop = stop_worker.clone();
        let worker_telemetry = telemetry.clone();
        let worker_error = on_error.clone();
        let worker_latest = latest.clone();
        let worker_first_frame = first_frame_seen.clone();
        let worker_failure_gate = Arc::new(Mutex::new(ConsecutiveFailureGate::default()));
        let failure_gate_for_worker = worker_failure_gate.clone();
        let worker = thread::Builder::new()
            .name("liveblock-frame-processor".into())
            .spawn(move || {
                let mut staging = None;
                while !worker_stop.load(Ordering::Acquire) {
                    match frame_receiver.recv_timeout(Duration::from_millis(100)) {
                        Ok(pending) => {
                            let result = readback_frame(&pending, &mut staging).and_then(|frame| {
                                worker_first_frame.store(true, Ordering::Release);
                                worker_latest.store(Some(Arc::new(frame.clone())));
                                std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                                    on_frame(&frame)
                                }))
                                .map_err(|_| anyhow!("frame processor panicked"))?;
                                Ok(())
                            });
                            match result {
                                Ok(()) => {
                                    failure_gate_for_worker.lock().success();
                                    worker_telemetry.processed();
                                }
                                Err(error) => {
                                    worker_telemetry.copy_error();
                                    if failure_gate_for_worker.lock().failure() {
                                        worker_error(format!(
                                            "three consecutive capture-frame failures: {error}"
                                        ));
                                    }
                                }
                            }
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
        let last_emit_clone = last_emit.clone();
        let telemetry_clone = telemetry.clone();
        let error_clone = on_error.clone();
        let failure_gate_clone = worker_failure_gate.clone();
        let current_size = Arc::new(AtomicU64::new(pack_size(size.Width, size.Height)));
        let current_size_clone = current_size.clone();

        let frame_token = pool.FrameArrived(&TypedEventHandler::new(
            move |sender: &Option<Direct3D11CaptureFramePool>, _: &Option<IInspectable>| {
                // The immediate context is multithread-protected: this callback
                // submits only the owned-texture copy while the worker stages it.
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
                    let pending = copy_frame_to_owned(&device_clone, &context_clone, &frame)?;

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

                    enqueue_latest(&frame_sender, &eviction_receiver, pending, &telemetry_clone);
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
            frame_token,
            closed_token,
            stop_worker,
            first_frame_seen,
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

    pub fn first_frame_signal(&self) -> Arc<AtomicBool> {
        self.first_frame_seen.clone()
    }

    pub fn has_first_frame(&self) -> bool {
        self.first_frame_seen.load(Ordering::Acquire)
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

/// Copy a frame-pool-owned WGC texture into an application-owned texture. This
/// deliberately avoids Map/readback in the WGC callback.
fn copy_frame_to_owned(
    device: &ID3D11Device,
    context: &ID3D11DeviceContext,
    frame: &windows::Graphics::Capture::Direct3D11CaptureFrame,
) -> Result<PendingGpuFrame> {
    use windows::Win32::System::WinRT::Direct3D11::IDirect3DDxgiInterfaceAccess;

    let surface = frame.Surface()?;
    let access: IDirect3DDxgiInterfaceAccess = surface.cast()?;
    let frame_tex: ID3D11Texture2D = unsafe { access.GetInterface()? };

    let mut desc = D3D11_TEXTURE2D_DESC::default();
    unsafe { frame_tex.GetDesc(&mut desc) };

    if desc.Width == 0
        || desc.Height == 0
        || desc.Format != DXGI_FORMAT_B8G8R8A8_UNORM
        || desc.SampleDesc.Count != 1
    {
        return Err(anyhow!("unsupported WGC texture descriptor"));
    }

    let owned_desc = D3D11_TEXTURE2D_DESC {
        Width: desc.Width,
        Height: desc.Height,
        MipLevels: 1,
        ArraySize: 1,
        Format: DXGI_FORMAT_B8G8R8A8_UNORM,
        SampleDesc: DXGI_SAMPLE_DESC {
            Count: 1,
            Quality: 0,
        },
        Usage: D3D11_USAGE_DEFAULT,
        BindFlags: D3D11_BIND_SHADER_RESOURCE.0 as u32,
        CPUAccessFlags: 0,
        MiscFlags: 0,
    };
    let mut owned = None;
    unsafe {
        device.CreateTexture2D(&owned_desc, None, Some(&mut owned))?;
    }
    let owned = owned.context("CreateTexture2D returned no owned frame")?;
    unsafe {
        context.CopyResource(&owned, &frame_tex);
    }

    Ok(PendingGpuFrame {
        width: desc.Width,
        height: desc.Height,
        resource: Arc::new(GpuFrameResource {
            texture: owned,
            device: device.clone(),
            context: context.clone(),
        }),
    })
}

/// Stage and map one application-owned texture on the processor worker. The
/// staging allocation is reused while dimensions remain stable.
fn readback_frame(
    pending: &PendingGpuFrame,
    staging: &mut Option<ReadbackStaging>,
) -> Result<FrameView> {
    let device = &pending.resource.device;
    let context = &pending.resource.context;
    let needs_staging = staging
        .as_ref()
        .map(|value| value.width != pending.width || value.height != pending.height)
        .unwrap_or(true);
    if needs_staging {
        let staging_desc = D3D11_TEXTURE2D_DESC {
            Width: pending.width,
            Height: pending.height,
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
        let mut texture = None;
        unsafe {
            device.CreateTexture2D(&staging_desc, None, Some(&mut texture))?;
        }
        *staging = Some(ReadbackStaging {
            width: pending.width,
            height: pending.height,
            texture: texture.context("CreateTexture2D returned no staging frame")?,
        });
    }
    let staging_texture = &staging.as_ref().context("staging texture missing")?.texture;

    unsafe {
        context.CopyResource(staging_texture, &pending.resource.texture);
    }

    // Map only on the worker.
    let mut mapped = D3D11_MAPPED_SUBRESOURCE::default();
    unsafe {
        context.Map(staging_texture, 0, D3D11_MAP_READ, 0, Some(&mut mapped))?;
    }

    let width = pending.width;
    let height = pending.height;
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
        context.Unmap(staging_texture, 0);
    }

    Ok(FrameView {
        width,
        height,
        bytes: Arc::new(packed),
        gpu: Some(pending.resource.clone()),
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
        EnumDisplayMonitors, GetMonitorInfoW, HDC, MONITORINFOEXW,
    };
    use windows::Win32::UI::HiDpi::{GetDpiForMonitor, MDT_EFFECTIVE_DPI};
    use windows::Win32::UI::WindowsAndMessaging::MONITORINFOF_PRIMARY;

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
