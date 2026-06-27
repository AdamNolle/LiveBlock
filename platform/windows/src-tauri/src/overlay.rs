//! Native GPU-presented overlay surface for LiveBlock (Windows).
//!
//! This replaces the old webview + `data:image/png;base64,...` patch path
//! (deleted): patches are now drawn onto a **native layered top-level window**
//! that is click-through, always-on-top, excluded from screen capture, and
//! positioned/sized onto the captured monitor. The window is updated with
//! `UpdateLayeredWindow` from a premultiplied-BGRA surface, so the overlay is a
//! real desktop-composited surface — never a webview, never base64.
//!
//! ## Threading
//! A top-level window must be pumped on the thread that created it, so the
//! overlay OWNS a dedicated thread: it creates the HWND, runs a message loop,
//! and handles `WM_DISPLAYCHANGE` by re-querying its bound monitor and moving
//! itself. `present` / `show` / `hide` / `position_on` post commands to that
//! thread; `present` blocks just long enough to swap the surface (it is invoked
//! from the pipeline worker at <= 30 Hz).
//!
//! ## Cover modes
//! Both [`OverlayMode::Inpaint`] (content-aware flat fill from the captured
//! frame) and [`OverlayMode::PaintOver`] (the DRM/HDCP-safe opaque cover drawn
//! WITHOUT reading protected pixels) share one draw path.
//!
//! TODO(windows-port): the perf-optimal presentation is a DirectComposition
//! `CreateSwapChainForComposition` swap-chain (no CPU copy, no DIB). That needs
//! the `Win32_Graphics_DirectComposition` feature; gated as a follow-up. The
//! `UpdateLayeredWindow` path here is correct and fully capture-excluded.

use anyhow::{anyhow, Result};
use std::sync::Arc;

use crossbeam_channel::{bounded, Sender};
use parking_lot::Mutex;
use windows::core::w;
use windows::Win32::Foundation::{COLORREF, HWND, LPARAM, LRESULT, POINT, RECT, SIZE, WPARAM};
use windows::Win32::Graphics::Gdi::{
    CreateCompatibleDC, CreateDIBSection, DeleteDC, DeleteObject, GetDC, ReleaseDC, SelectObject,
    AC_SRC_ALPHA, AC_SRC_OVER, BITMAPINFO, BITMAPINFOHEADER, BI_RGB, BLENDFUNCTION, DIB_RGB_COLORS,
    HBITMAP, HDC, HMONITOR,
};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DispatchMessageW, GetMessageW, GetWindowLongPtrW,
    PostMessageW, RegisterClassExW, SetLayeredWindowAttributes, SetWindowDisplayAffinity,
    SetWindowLongPtrW, SetWindowPos, ShowWindow, TranslateMessage, UpdateLayeredWindow,
    GWLP_USERDATA, GWL_EXSTYLE, HWND_TOPMOST, LWA_ALPHA, MSG, SWP_NOACTIVATE, SWP_NOMOVE,
    SWP_NOSIZE, SW_HIDE, SW_SHOWNOACTIVATE, ULW_ALPHA, WDA_EXCLUDEFROMCAPTURE, WM_APP,
    WM_DISPLAYCHANGE, WNDCLASSEXW, WS_EX_LAYERED, WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW, WS_EX_TOPMOST,
    WS_EX_TRANSPARENT, WS_POPUP,
};

pub use liveblock_core::{Fill, NormRect, OverlayMode, PaintPatch};

/// Custom message: the worker has a new set of cover patches ready; the LPARAM
/// is a `*mut Vec<CoverPatch>` the window thread takes ownership of and frees.
const WM_LB_PRESENT: u32 = WM_APP + 1;
/// Custom message: show / hide / reposition / destroy requests.
const WM_LB_SHOW: u32 = WM_APP + 2;
const WM_LB_HIDE: u32 = WM_APP + 3;
/// LPARAM is a `*mut MonitorRect`.
const WM_LB_REPOSITION: u32 = WM_APP + 4;
const WM_LB_DESTROY: u32 = WM_APP + 5;

/// A single cover the overlay must draw, in **normalized [0..1]** coordinates of
/// the bound monitor, plus the mode that produced it. `mode` records whether the
/// cover came from the content-aware inpaint path or the DRM-safe PaintOver path
/// (both rasterize identically today; the distinction drives diagnostics and the
/// future GPU-inpaint branch).
#[derive(Debug, Clone, Copy)]
pub struct CoverPatch {
    pub rect: NormRect,
    pub fill: Fill,
    #[allow(dead_code)]
    pub mode: OverlayMode,
}

impl From<PaintPatch> for CoverPatch {
    fn from(p: PaintPatch) -> Self {
        CoverPatch {
            rect: p.rect,
            fill: p.fill,
            mode: OverlayMode::PaintOver,
        }
    }
}

/// Geometry of the monitor the overlay is bound to (desktop pixels).
#[derive(Debug, Clone, Copy, Default)]
pub struct MonitorRect {
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub height: i32,
}

/// Handle to the overlay window thread. Cheap to clone via `Arc`.
pub struct OverlayWindow {
    hwnd: Mutex<HWND>,
    /// Keeps the thread channel alive; the window thread exits when this is
    /// dropped (it also receives an explicit `WM_LB_DESTROY`).
    _ready: Sender<()>,
    rect: Mutex<MonitorRect>,
}

// HWND is only posted to via thread-safe PostMessageW; the value never races.
unsafe impl Send for OverlayWindow {}
unsafe impl Sync for OverlayWindow {}

impl OverlayWindow {
    /// Create the overlay window on its own pump thread, bound to `rect`.
    pub fn create(rect: MonitorRect) -> Result<Arc<Self>> {
        let (hwnd_tx, hwnd_rx) = bounded::<Result<isize, String>>(1);
        let (ready_tx, ready_rx) = bounded::<()>(1);

        std::thread::Builder::new()
            .name("liveblock-overlay".into())
            .spawn(move || unsafe { overlay_thread(rect, hwnd_tx, ready_rx) })
            .map_err(|e| anyhow!("spawn overlay thread: {e}"))?;

        let raw = hwnd_rx
            .recv()
            .map_err(|_| anyhow!("overlay thread died before reporting HWND"))?
            .map_err(|e| anyhow!("overlay window creation failed: {e}"))?;

        Ok(Arc::new(Self {
            hwnd: Mutex::new(HWND(raw as *mut _)),
            _ready: ready_tx,
            rect: Mutex::new(rect),
        }))
    }

    fn post(&self, msg: u32, lparam: isize) {
        let hwnd = *self.hwnd.lock();
        if !hwnd.0.is_null() {
            unsafe {
                let _ = PostMessageW(hwnd, msg, WPARAM(0), LPARAM(lparam));
            }
        }
    }

    /// Reposition/resize the overlay onto a new monitor rect (e.g. capture
    /// switched displays). The window thread rebuilds its backing surface.
    pub fn position_on(&self, rect: MonitorRect) -> Result<()> {
        *self.rect.lock() = rect;
        let boxed = Box::into_raw(Box::new(rect));
        self.post(WM_LB_REPOSITION, boxed as isize);
        Ok(())
    }

    /// Show without activating (keeps the captured app focused); the thread
    /// re-applies click-through + capture-exclusion after showing.
    pub fn show(&self) -> Result<()> {
        self.post(WM_LB_SHOW, 0);
        Ok(())
    }

    /// Hide and clear the overlay.
    pub fn hide(&self) -> Result<()> {
        self.post(WM_LB_HIDE, 0);
        Ok(())
    }

    /// Present a fresh set of cover patches. Ownership of the boxed Vec is
    /// transferred to the window thread, which composites and frees it.
    pub fn present(&self, patches: &[CoverPatch]) -> Result<()> {
        let boxed = Box::into_raw(Box::new(patches.to_vec()));
        self.post(WM_LB_PRESENT, boxed as isize);
        Ok(())
    }

    /// The monitor rect the overlay is currently bound to.
    #[allow(dead_code)]
    pub fn rect(&self) -> MonitorRect {
        *self.rect.lock()
    }
}

impl Drop for OverlayWindow {
    fn drop(&mut self) {
        self.post(WM_LB_DESTROY, 0);
    }
}

const OVERLAY_CLASS: windows::core::PCWSTR = w!("LiveBlockOverlayClass");

/// Per-window state stashed in `GWLP_USERDATA` so the wndproc can reposition on
/// `WM_DISPLAYCHANGE` and recreate its backing surface on size change.
struct WindowState {
    rect: MonitorRect,
    /// CPU-side premultiplied BGRA surface backing the layered window.
    dib_bits: *mut u8,
    dib: HBITMAP,
    mem_dc: HDC,
}

impl WindowState {
    unsafe fn new_surface(rect: MonitorRect) -> Result<(*mut u8, HBITMAP, HDC)> {
        let w = rect.width.max(1);
        let h = rect.height.max(1);
        let bmi = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: w,
                biHeight: -h, // top-down
                biPlanes: 1,
                biBitCount: 32,
                biCompression: BI_RGB.0,
                ..Default::default()
            },
            ..Default::default()
        };
        let screen_dc = GetDC(None);
        let mem_dc = CreateCompatibleDC(screen_dc);
        let mut bits: *mut core::ffi::c_void = std::ptr::null_mut();
        let dib = CreateDIBSection(mem_dc, &bmi, DIB_RGB_COLORS, &mut bits, None, 0)
            .map_err(|e| anyhow!("CreateDIBSection: {e}"))?;
        let _ = ReleaseDC(None, screen_dc);
        if bits.is_null() {
            return Err(anyhow!("CreateDIBSection returned null bits"));
        }
        SelectObject(mem_dc, dib);
        Ok((bits as *mut u8, dib, mem_dc))
    }

    unsafe fn rebuild_surface(&mut self, rect: MonitorRect) -> Result<()> {
        let (bits, dib, mem_dc) = Self::new_surface(rect)?;
        let _ = DeleteObject(self.dib);
        let _ = DeleteDC(self.mem_dc);
        self.dib_bits = bits;
        self.dib = dib;
        self.mem_dc = mem_dc;
        self.rect = rect;
        Ok(())
    }

    unsafe fn present(&self, hwnd: HWND, patches: &[CoverPatch]) -> Result<()> {
        let w = self.rect.width.max(1) as usize;
        let h = self.rect.height.max(1) as usize;
        let stride = w * 4;
        let buf = std::slice::from_raw_parts_mut(self.dib_bits, stride * h);

        buf.fill(0); // transparent

        for p in patches {
            let Fill::Solid { r, g, b, a } = p.fill;
            let x0 = (p.rect.x.clamp(0.0, 1.0) * w as f32).floor() as usize;
            let y0 = (p.rect.y.clamp(0.0, 1.0) * h as f32).floor() as usize;
            let x1 = ((p.rect.x + p.rect.width).clamp(0.0, 1.0) * w as f32).ceil() as usize;
            let y1 = ((p.rect.y + p.rect.height).clamp(0.0, 1.0) * h as f32).ceil() as usize;
            let x1 = x1.min(w);
            let y1 = y1.min(h);
            if x1 <= x0 || y1 <= y0 {
                continue;
            }
            // Premultiply (no-op for opaque covers).
            let af = a as u32;
            let pb = ((b as u32 * af) / 255) as u8;
            let pg = ((g as u32 * af) / 255) as u8;
            let pr = ((r as u32 * af) / 255) as u8;
            for y in y0..y1 {
                let row = y * stride;
                for x in x0..x1 {
                    let o = row + x * 4;
                    buf[o] = pb;
                    buf[o + 1] = pg;
                    buf[o + 2] = pr;
                    buf[o + 3] = a;
                }
            }
        }

        let blend = BLENDFUNCTION {
            BlendOp: AC_SRC_OVER as u8,
            BlendFlags: 0,
            SourceConstantAlpha: 255,
            AlphaFormat: AC_SRC_ALPHA as u8,
        };
        let dst_pos = POINT {
            x: self.rect.x,
            y: self.rect.y,
        };
        let src_pos = POINT { x: 0, y: 0 };
        let size = SIZE {
            cx: self.rect.width.max(1),
            cy: self.rect.height.max(1),
        };
        UpdateLayeredWindow(
            hwnd,
            HDC::default(),
            Some(&dst_pos),
            Some(&size),
            self.mem_dc,
            Some(&src_pos),
            COLORREF(0),
            Some(&blend),
            ULW_ALPHA,
        )
        .map_err(|e| anyhow!("UpdateLayeredWindow: {e}"))
    }

    unsafe fn destroy(self) {
        let _ = DeleteObject(self.dib);
        let _ = DeleteDC(self.mem_dc);
    }
}

/// The overlay window thread: create the window, report its HWND, pump messages.
unsafe fn overlay_thread(
    rect: MonitorRect,
    hwnd_tx: Sender<Result<isize, String>>,
    _ready_rx: crossbeam_channel::Receiver<()>,
) {
    let hinstance = match GetModuleHandleW(None) {
        Ok(h) => h,
        Err(e) => {
            let _ = hwnd_tx.send(Err(format!("GetModuleHandleW: {e}")));
            return;
        }
    };
    let wc = WNDCLASSEXW {
        cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
        lpfnWndProc: Some(overlay_wndproc),
        hInstance: hinstance.into(),
        lpszClassName: OVERLAY_CLASS,
        ..Default::default()
    };
    let _ = RegisterClassExW(&wc);

    let ex_style = WS_EX_LAYERED
        | WS_EX_TRANSPARENT
        | WS_EX_NOACTIVATE
        | WS_EX_TOPMOST
        | WS_EX_TOOLWINDOW;

    let hwnd = match CreateWindowExW(
        ex_style,
        OVERLAY_CLASS,
        w!("LiveBlock Overlay"),
        WS_POPUP,
        rect.x,
        rect.y,
        rect.width.max(1),
        rect.height.max(1),
        None,
        None,
        hinstance,
        None,
    ) {
        Ok(h) if !h.0.is_null() => h,
        Ok(_) => {
            let _ = hwnd_tx.send(Err("CreateWindowExW returned null".into()));
            return;
        }
        Err(e) => {
            let _ = hwnd_tx.send(Err(format!("CreateWindowExW: {e}")));
            return;
        }
    };

    // Allocate the backing surface + window state, stash it in GWLP_USERDATA.
    let state = match WindowState::new_surface(rect) {
        Ok((bits, dib, mem_dc)) => Box::new(WindowState {
            rect,
            dib_bits: bits,
            dib,
            mem_dc,
        }),
        Err(e) => {
            let _ = hwnd_tx.send(Err(format!("surface: {e}")));
            return;
        }
    };
    SetWindowLongPtrW(hwnd, GWLP_USERDATA, Box::into_raw(state) as isize);

    let _ = reapply_overlay_styles(hwnd);

    // Report the HWND back to the creator.
    let _ = hwnd_tx.send(Ok(hwnd.0 as isize));

    // Message pump.
    let mut msg = MSG::default();
    while GetMessageW(&mut msg, hwnd, 0, 0).0 > 0 {
        let _ = TranslateMessage(&msg);
        DispatchMessageW(&msg);
    }
}

extern "system" fn overlay_wndproc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    unsafe {
        match msg {
            WM_LB_PRESENT => {
                let patches = Box::from_raw(lp.0 as *mut Vec<CoverPatch>);
                if let Some(state) = window_state(hwnd) {
                    if let Err(e) = state.present(hwnd, &patches) {
                        tracing::warn!("overlay present: {e}");
                    }
                }
                LRESULT(0)
            }
            WM_LB_SHOW => {
                let _ = ShowWindow(hwnd, SW_SHOWNOACTIVATE);
                let _ = reapply_overlay_styles(hwnd);
                LRESULT(0)
            }
            WM_LB_HIDE => {
                if let Some(state) = window_state(hwnd) {
                    let _ = state.present(hwnd, &[]);
                }
                let _ = ShowWindow(hwnd, SW_HIDE);
                LRESULT(0)
            }
            WM_LB_REPOSITION => {
                let rect = *Box::from_raw(lp.0 as *mut MonitorRect);
                if let Some(state) = window_state(hwnd) {
                    if let Err(e) = state.rebuild_surface(rect) {
                        tracing::error!("overlay rebuild surface: {e}");
                    }
                }
                let _ = SetWindowPos(
                    hwnd,
                    HWND_TOPMOST,
                    rect.x,
                    rect.y,
                    rect.width.max(1),
                    rect.height.max(1),
                    SWP_NOACTIVATE,
                );
                let _ = reapply_overlay_styles(hwnd);
                LRESULT(0)
            }
            WM_DISPLAYCHANGE => {
                // The display topology changed; re-query the monitor under the
                // overlay's current rect and resize to it so covers stay aligned.
                if let Some(state) = window_state(hwnd) {
                    let center = POINT {
                        x: state.rect.x + state.rect.width / 2,
                        y: state.rect.y + state.rect.height / 2,
                    };
                    if let Some(new_rect) = monitor_rect_from_point(center) {
                        if let Err(e) = state.rebuild_surface(new_rect) {
                            tracing::error!("overlay rebuild on display change: {e}");
                        }
                        let _ = SetWindowPos(
                            hwnd,
                            HWND_TOPMOST,
                            new_rect.x,
                            new_rect.y,
                            new_rect.width.max(1),
                            new_rect.height.max(1),
                            SWP_NOACTIVATE,
                        );
                        let _ = reapply_overlay_styles(hwnd);
                    }
                }
                LRESULT(0)
            }
            WM_LB_DESTROY => {
                use windows::Win32::UI::WindowsAndMessaging::DestroyWindow;
                let ptr = SetWindowLongPtrW(hwnd, GWLP_USERDATA, 0);
                if ptr != 0 {
                    let state = Box::from_raw(ptr as *mut WindowState);
                    state.destroy();
                }
                let _ = DestroyWindow(hwnd);
                // Tell the pump to exit.
                use windows::Win32::UI::WindowsAndMessaging::PostQuitMessage;
                PostQuitMessage(0);
                LRESULT(0)
            }
            _ => DefWindowProcW(hwnd, msg, wp, lp),
        }
    }
}

unsafe fn window_state<'a>(hwnd: HWND) -> Option<&'a mut WindowState> {
    let ptr = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut WindowState;
    if ptr.is_null() {
        None
    } else {
        Some(&mut *ptr)
    }
}

/// (Re)apply the overlay's defining styles. Called after creation, after each
/// show, and after each reposition, because Windows can drop
/// `WDA_EXCLUDEFROMCAPTURE` across show/parent transitions — losing it would
/// leak the overlay into the very capture we feed the detector (a feedback loop).
unsafe fn reapply_overlay_styles(hwnd: HWND) -> Result<()> {
    if hwnd.0.is_null() {
        return Err(anyhow!("null HWND"));
    }
    let current = GetWindowLongPtrW(hwnd, GWL_EXSTYLE);
    let new_style = current
        | WS_EX_LAYERED.0 as isize
        | WS_EX_TRANSPARENT.0 as isize
        | WS_EX_NOACTIVATE.0 as isize
        | WS_EX_TOPMOST.0 as isize;
    SetWindowLongPtrW(hwnd, GWL_EXSTYLE, new_style);
    let _ = SetWindowPos(hwnd, HWND_TOPMOST, 0, 0, 0, 0, SWP_NOACTIVATE | SWP_NOMOVE | SWP_NOSIZE);
    SetWindowDisplayAffinity(hwnd, WDA_EXCLUDEFROMCAPTURE)
        .map_err(|e| anyhow!("SetWindowDisplayAffinity(WDA_EXCLUDEFROMCAPTURE): {e}"))
}

// ===========================================================================
// Legacy: apply native styles to the EXISTING Tauri webview windows.
// Kept so the current Tauri shell keeps working during the native-overlay
// transition. The pointer-newtype bugs (HWND.0 as isize) are fixed: under
// windows-rs 0.58 every H* handle is a `*mut c_void` newtype.
// ===========================================================================

/// Make a Tauri webview HWND click-through, always-on-top, layered, and excluded
/// from screen capture.
pub fn make_render_overlay(hwnd: HWND) -> Result<()> {
    if hwnd.0.is_null() {
        return Err(anyhow!("null HWND"));
    }
    unsafe {
        let current = GetWindowLongPtrW(hwnd, GWL_EXSTYLE);
        let new_style = current
            | WS_EX_LAYERED.0 as isize
            | WS_EX_TRANSPARENT.0 as isize
            | WS_EX_NOACTIVATE.0 as isize
            | WS_EX_TOPMOST.0 as isize;
        SetWindowLongPtrW(hwnd, GWL_EXSTYLE, new_style);
        SetLayeredWindowAttributes(hwnd, COLORREF(0), 255, LWA_ALPHA)
            .map_err(|e| anyhow!("SetLayeredWindowAttributes: {e}"))?;
        SetWindowDisplayAffinity(hwnd, WDA_EXCLUDEFROMCAPTURE)
            .map_err(|e| anyhow!("SetWindowDisplayAffinity: {e}"))?;
    }
    Ok(())
}

/// Editor window: never steals focus from the captured app, but is clickable.
pub fn make_editor(hwnd: HWND) -> Result<()> {
    if hwnd.0.is_null() {
        return Err(anyhow!("null HWND"));
    }
    unsafe {
        let current = GetWindowLongPtrW(hwnd, GWL_EXSTYLE);
        let new_style = current | WS_EX_NOACTIVATE.0 as isize | WS_EX_TOPMOST.0 as isize;
        SetWindowLongPtrW(hwnd, GWL_EXSTYLE, new_style);
    }
    Ok(())
}

/// Look up a monitor's desktop-pixel rect from its `HMONITOR`.
pub fn monitor_rect(monitor: HMONITOR) -> Option<MonitorRect> {
    use windows::Win32::Graphics::Gdi::{GetMonitorInfoW, MONITORINFO};
    if monitor.0.is_null() {
        return None;
    }
    let mut mi = MONITORINFO {
        cbSize: std::mem::size_of::<MONITORINFO>() as u32,
        ..Default::default()
    };
    let ok = unsafe { GetMonitorInfoW(monitor, &mut mi) };
    if !ok.as_bool() {
        return None;
    }
    rect_from(mi.rcMonitor)
}

/// Monitor rect containing `pt` (used to react to `WM_DISPLAYCHANGE`).
fn monitor_rect_from_point(pt: POINT) -> Option<MonitorRect> {
    use windows::Win32::Graphics::Gdi::{MonitorFromPoint, MONITOR_DEFAULTTONEAREST};
    let h = unsafe { MonitorFromPoint(pt, MONITOR_DEFAULTTONEAREST) };
    monitor_rect(h)
}

fn rect_from(r: RECT) -> Option<MonitorRect> {
    Some(MonitorRect {
        x: r.left,
        y: r.top,
        width: r.right - r.left,
        height: r.bottom - r.top,
    })
}
