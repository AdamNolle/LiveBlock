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
use parking_lot::Mutex;
use std::sync::Arc;
use std::time::{Duration, Instant};

use windows::core::{IInspectable, Interface};
use windows::Foundation::TypedEventHandler;
use windows::Graphics::Capture::{
    Direct3D11CaptureFramePool, GraphicsCaptureItem, GraphicsCaptureSession,
};
use windows::Graphics::DirectX::Direct3D11::IDirect3DDevice;
use windows::Graphics::DirectX::DirectXPixelFormat;
use windows::Win32::Foundation::HMODULE;
use windows::Win32::Graphics::Direct3D::{D3D_DRIVER_TYPE_HARDWARE, D3D_FEATURE_LEVEL_11_0};
use windows::Win32::Graphics::Direct3D11::{
    D3D11CreateDevice, ID3D11Device, ID3D11DeviceContext, ID3D11Texture2D,
    D3D11_CPU_ACCESS_READ, D3D11_CREATE_DEVICE_BGRA_SUPPORT, D3D11_MAPPED_SUBRESOURCE,
    D3D11_MAP_READ, D3D11_SDK_VERSION, D3D11_TEXTURE2D_DESC, D3D11_USAGE_STAGING,
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
}

impl CaptureSession {
    pub fn start(monitor: HMONITOR, on_frame: FrameCallback) -> Result<Self> {
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

        // 5. Frame arrived handler.
        let latest = Arc::new(ArcSwapOption::<FrameView>::from(None));
        let last_emit = Arc::new(Mutex::new(Instant::now() - Duration::from_secs(1)));
        let on_frame_clone = on_frame.clone();
        let device_clone = d3d_device.clone();
        let context_clone = d3d_context.clone();
        let latest_clone = latest.clone();
        let last_emit_clone = last_emit.clone();

        pool.FrameArrived(&TypedEventHandler::new(move |sender, _| {
            // SAFETY: closure runs on the WGC thread; D3D11 immediate context
            // is single-threaded — we only touch it here. If we ever switch
            // to multi-frame parallelism, switch to a deferred context.
            if let Some(pool_ref) = sender {
                if let Ok(frame) = pool_ref.TryGetNextFrame() {
                    if let Ok(view) = process_frame(&device_clone, &context_clone, &frame) {
                        latest_clone.store(Some(Arc::new(view.clone())));
                        // Throttle emits to 30 Hz.
                        let mut last = last_emit_clone.lock();
                        if last.elapsed() >= Duration::from_millis(33) {
                            *last = Instant::now();
                            drop(last);
                            on_frame_clone(&view);
                        }
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
            latest,
            last_emit,
        })
    }

    pub fn latest_frame(&self) -> Option<FrameView> {
        self.latest.load_full().map(|a| (*a).clone())
    }

    pub fn stop(self) {
        let _ = self.session.Close();
    }
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
        SampleDesc: DXGI_SAMPLE_DESC { Count: 1, Quality: 0 },
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
