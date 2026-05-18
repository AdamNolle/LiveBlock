//! Global hotkeys via RegisterHotKey on a HWND_MESSAGE-only window.
//! Mirrors `Sources/HotKeyMonitor.swift`.

use std::thread;
use tauri::{AppHandle, Emitter};

use windows::core::w;
use windows::Win32::Foundation::{HWND, LPARAM, WPARAM};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::Input::KeyboardAndMouse::{
    RegisterHotKey, MOD_ALT, MOD_CONTROL, MOD_SHIFT,
};
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DispatchMessageW, GetMessageW, RegisterClassExW,
    TranslateMessage, HWND_MESSAGE, MSG, WINDOW_EX_STYLE, WINDOW_STYLE, WM_HOTKEY, WNDCLASSEXW,
    WS_OVERLAPPED,
};

const HOTKEY_TOGGLE_CAPTURE: i32 = 1;
const HOTKEY_TOGGLE_EDITOR: i32 = 2;
const HOTKEY_CAPTURE_SCREENSHOT: i32 = 3;
const HOTKEY_PANIC_DISABLE: i32 = 4;
// VK_OEM_PERIOD = 0xBE — same key macOS binds Cmd+Shift+Opt+. to.
const VK_OEM_PERIOD: u32 = 0xBE;

unsafe extern "system" fn wndproc(
    hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM,
) -> windows::Win32::Foundation::LRESULT {
    DefWindowProcW(hwnd, msg, wparam, lparam)
}

/// Spawn a dedicated thread that owns a message-only window, registers the
/// three hotkeys, and forwards them as Tauri events.
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

        let hwnd = CreateWindowExW(
            WINDOW_EX_STYLE::default(),
            class_name,
            w!("LiveBlockHotkeyWindow"),
            WS_OVERLAPPED,
            0, 0, 0, 0,
            HWND_MESSAGE,
            None,
            h_instance,
            None,
        );
        let hwnd = match hwnd {
            Ok(h) => h,
            Err(e) => {
                tracing::error!("CreateWindowExW (HWND_MESSAGE) failed: {e}");
                return;
            }
        };

        let modifiers = MOD_CONTROL | MOD_SHIFT;
        let panic_modifiers = MOD_CONTROL | MOD_SHIFT | MOD_ALT;
        let _ = RegisterHotKey(hwnd, HOTKEY_TOGGLE_CAPTURE, modifiers, 'L' as u32);
        let _ = RegisterHotKey(hwnd, HOTKEY_TOGGLE_EDITOR, modifiers, 'B' as u32);
        let _ = RegisterHotKey(hwnd, HOTKEY_CAPTURE_SCREENSHOT, modifiers, 'S' as u32);
        // Panic-disable matches macOS Cmd+Shift+Opt+. — closes editor + shows panel.
        let _ = RegisterHotKey(hwnd, HOTKEY_PANIC_DISABLE, panic_modifiers, VK_OEM_PERIOD);

        let mut msg = MSG::default();
        while GetMessageW(&mut msg, hwnd, 0, 0).as_bool() {
            if msg.message == WM_HOTKEY {
                match msg.wParam.0 as i32 {
                    HOTKEY_TOGGLE_CAPTURE => {
                        let _ = app.emit("hotkey-toggle-capture", ());
                    }
                    HOTKEY_TOGGLE_EDITOR => {
                        let _ = app.emit("hotkey-toggle-editor", ());
                    }
                    HOTKEY_CAPTURE_SCREENSHOT => {
                        let _ = app.emit("hotkey-capture-screenshot", ());
                    }
                    HOTKEY_PANIC_DISABLE => {
                        let _ = app.emit("hotkey-panic-disable", ());
                    }
                    _ => {}
                }
            }
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
    });
}
