//! Global hotkeys via RegisterHotKey on a HWND_MESSAGE-only window.
//! Mirrors `Sources/HotKeyMonitor.swift`.
//!
//! WM_HOTKEY drives REAL backend actions (toggle capture, panic-disable, capture
//! screenshot, open editor) via `crate::actions`, not dead Tauri events. The
//! panic-disable hotkey is registered with `MOD_NOREPEAT` and its failure is
//! logged loudly (it is the safety kill-switch — silently dropping it is unsafe).

use std::thread;
use tauri::AppHandle;

use windows::core::w;
use windows::Win32::Foundation::{HWND, LPARAM, WPARAM};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::Input::KeyboardAndMouse::{
    RegisterHotKey, HOT_KEY_MODIFIERS, MOD_ALT, MOD_CONTROL, MOD_NOREPEAT, MOD_SHIFT,
};
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DispatchMessageW, GetMessageW, RegisterClassExW,
    TranslateMessage, HWND_MESSAGE, MSG, WINDOW_EX_STYLE, WM_HOTKEY, WNDCLASSEXW, WS_OVERLAPPED,
};

const HOTKEY_TOGGLE_CAPTURE: i32 = 1;
const HOTKEY_TOGGLE_EDITOR: i32 = 2;
const HOTKEY_CAPTURE_SCREENSHOT: i32 = 3;
const HOTKEY_PANIC_DISABLE: i32 = 4;
// VK_OEM_PERIOD = 0xBE — same key macOS binds Cmd+Shift+Opt+. to.
const VK_OEM_PERIOD: u32 = 0xBE;

unsafe extern "system" fn wndproc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> windows::Win32::Foundation::LRESULT {
    DefWindowProcW(hwnd, msg, wparam, lparam)
}

/// Spawn a dedicated thread that owns a message-only window, registers the
/// hotkeys, and dispatches them to real backend actions.
pub fn spawn(app: AppHandle) {
    thread::spawn(move || unsafe {
        let h_instance = match GetModuleHandleW(None) {
            Ok(h) => h,
            Err(e) => {
                tracing::error!("GetModuleHandleW failed: {e}");
                return;
            }
        };
        let class_name = w!("LiveBlockHotkeyClass");
        let wc = WNDCLASSEXW {
            cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
            lpfnWndProc: Some(wndproc),
            hInstance: h_instance.into(),
            lpszClassName: class_name,
            ..Default::default()
        };
        let _ = RegisterClassExW(&wc);

        let hwnd = match CreateWindowExW(
            WINDOW_EX_STYLE::default(),
            class_name,
            w!("LiveBlockHotkeyWindow"),
            WS_OVERLAPPED,
            0,
            0,
            0,
            0,
            HWND_MESSAGE,
            None,
            h_instance,
            None,
        ) {
            Ok(h) => h,
            Err(e) => {
                tracing::error!("CreateWindowExW (HWND_MESSAGE) failed: {e}");
                return;
            }
        };

        // MOD_NOREPEAT so a held key fires once. Register each hotkey and LOG any
        // failure (previously these `let _ =` swallowed errors — a registration
        // clash would silently disable a hotkey, including the panic kill-switch).
        let modifiers = MOD_CONTROL | MOD_SHIFT | MOD_NOREPEAT;
        let panic_modifiers = MOD_CONTROL | MOD_SHIFT | MOD_ALT | MOD_NOREPEAT;
        register(hwnd, HOTKEY_TOGGLE_CAPTURE, modifiers, 'L' as u32, "toggle-capture");
        register(hwnd, HOTKEY_TOGGLE_EDITOR, modifiers, 'B' as u32, "toggle-editor");
        register(hwnd, HOTKEY_CAPTURE_SCREENSHOT, modifiers, 'S' as u32, "capture-screenshot");
        // Panic-disable is the safety kill-switch; a failure here is loud + fatal
        // to the hotkey thread's usefulness, so surface it at error level.
        if !register(hwnd, HOTKEY_PANIC_DISABLE, panic_modifiers, VK_OEM_PERIOD, "panic-disable") {
            tracing::error!(
                "PANIC-DISABLE hotkey (Ctrl+Shift+Alt+.) failed to register — \
                 the kill-switch is unavailable; another app may own this combo"
            );
        }

        let mut msg = MSG::default();
        // GetMessageW returns -1 on error, 0 on WM_QUIT, >0 otherwise.
        while GetMessageW(&mut msg, hwnd, 0, 0).0 > 0 {
            if msg.message == WM_HOTKEY {
                match msg.wParam.0 as i32 {
                    HOTKEY_TOGGLE_CAPTURE => crate::actions::toggle_capture(&app),
                    HOTKEY_TOGGLE_EDITOR => crate::actions::show_window_action(&app, "editor"),
                    HOTKEY_CAPTURE_SCREENSHOT => crate::actions::capture_screenshot_action(&app),
                    HOTKEY_PANIC_DISABLE => crate::actions::panic_disable_action(&app),
                    _ => {}
                }
            }
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
    });
}

/// Register one hotkey; returns whether it succeeded (and logs failures).
unsafe fn register(
    hwnd: HWND,
    id: i32,
    modifiers: HOT_KEY_MODIFIERS,
    vk: u32,
    name: &str,
) -> bool {
    match RegisterHotKey(hwnd, id, modifiers, vk) {
        Ok(()) => true,
        Err(e) => {
            tracing::warn!("RegisterHotKey({name}) failed: {e}");
            false
        }
    }
}
