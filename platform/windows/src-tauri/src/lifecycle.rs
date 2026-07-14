//! Native Windows power, session-lock, and display-topology observation.

use std::ffi::c_void;
use std::thread;
use tauri::{AppHandle, Emitter};
use windows::core::w;
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::System::RemoteDesktop::{
    WTSRegisterSessionNotification, WTSUnRegisterSessionNotification, NOTIFY_FOR_THIS_SESSION,
};
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DispatchMessageW, GetMessageW, GetWindowLongPtrW,
    RegisterClassExW, SetWindowLongPtrW, TranslateMessage, CREATESTRUCTW, GWLP_USERDATA, MSG,
    PBT_APMRESUMEAUTOMATIC, PBT_APMSUSPEND, WINDOW_EX_STYLE, WM_DISPLAYCHANGE, WM_NCCREATE,
    WM_POWERBROADCAST, WM_WTSSESSION_CHANGE, WNDCLASSEXW, WS_OVERLAPPED, WTS_SESSION_LOCK,
    WTS_SESSION_UNLOCK,
};

struct LifecycleContext {
    app: AppHandle,
}

unsafe extern "system" fn wndproc(
    hwnd: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    if message == WM_NCCREATE {
        let create = &*(lparam.0 as *const CREATESTRUCTW);
        SetWindowLongPtrW(hwnd, GWLP_USERDATA, create.lpCreateParams as isize);
    }
    let context = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *const LifecycleContext;
    if !context.is_null() {
        let app = &(*context).app;
        match message {
            WM_POWERBROADCAST if wparam.0 as u32 == PBT_APMSUSPEND => {
                let _ = app.emit("windows-power-suspension-changed", true);
            }
            WM_POWERBROADCAST if wparam.0 as u32 == PBT_APMRESUMEAUTOMATIC => {
                let _ = app.emit("windows-power-suspension-changed", false);
            }
            WM_WTSSESSION_CHANGE if wparam.0 as u32 == WTS_SESSION_LOCK => {
                let _ = app.emit("windows-session-lock-changed", true);
            }
            WM_WTSSESSION_CHANGE if wparam.0 as u32 == WTS_SESSION_UNLOCK => {
                let _ = app.emit("windows-session-lock-changed", false);
            }
            WM_DISPLAYCHANGE => {
                let _ = app.emit("windows-display-topology-changed", ());
            }
            _ => {}
        }
    }
    DefWindowProcW(hwnd, message, wparam, lparam)
}

/// Own an invisible top-level window. Message-only windows do not receive all
/// broadcast power/display notifications, so this must remain a non-visible
/// overlapped window.
pub fn spawn(app: AppHandle) {
    thread::spawn(move || unsafe {
        let instance = match GetModuleHandleW(None) {
            Ok(instance) => instance,
            Err(error) => {
                tracing::error!("lifecycle GetModuleHandleW failed: {error}");
                let _ = app.emit("windows-lifecycle-availability-changed", false);
                return;
            }
        };
        let class_name = w!("LiveBlockLifecycleObserver");
        let class = WNDCLASSEXW {
            cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
            lpfnWndProc: Some(wndproc),
            hInstance: instance.into(),
            lpszClassName: class_name,
            ..Default::default()
        };
        let _ = RegisterClassExW(&class);
        let context = Box::new(LifecycleContext { app });
        let context_pointer = (&*context as *const LifecycleContext).cast::<c_void>();
        let hwnd = match CreateWindowExW(
            WINDOW_EX_STYLE::default(),
            class_name,
            w!("LiveBlock lifecycle observer"),
            WS_OVERLAPPED,
            0,
            0,
            0,
            0,
            HWND::default(),
            None,
            instance,
            Some(context_pointer),
        ) {
            Ok(hwnd) => hwnd,
            Err(error) => {
                tracing::error!("create lifecycle observer window failed: {error}");
                let _ = context
                    .app
                    .emit("windows-lifecycle-availability-changed", false);
                return;
            }
        };
        let session_registered = WTSRegisterSessionNotification(hwnd, NOTIFY_FOR_THIS_SESSION)
            .map(|()| true)
            .unwrap_or_else(|error| {
                tracing::error!("WTS session notification registration failed: {error}");
                false
            });
        let _ = context
            .app
            .emit("windows-lifecycle-availability-changed", session_registered);
        let mut message = MSG::default();
        loop {
            let result = GetMessageW(&mut message, hwnd, 0, 0).0;
            if result == -1 {
                tracing::error!(
                    "lifecycle GetMessageW failed: {}",
                    std::io::Error::last_os_error()
                );
                break;
            }
            if result == 0 {
                break;
            }
            let _ = TranslateMessage(&message);
            DispatchMessageW(&message);
        }
        let _ = context
            .app
            .emit("windows-lifecycle-availability-changed", false);
        if session_registered {
            let _ = WTSUnRegisterSessionNotification(hwnd);
        }
        drop(context);
    });
}
