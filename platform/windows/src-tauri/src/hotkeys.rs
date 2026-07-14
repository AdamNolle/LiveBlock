//! Global hotkeys via RegisterHotKey on a HWND_MESSAGE-only window.
//! Mirrors `Sources/HotKeyMonitor.swift`.

use std::sync::atomic::{AtomicU64, Ordering};
use std::thread;
use tauri::{AppHandle, Emitter};

use windows::core::w;
use windows::Win32::Foundation::{HWND, LPARAM, WPARAM};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::Input::KeyboardAndMouse::{
    RegisterHotKey, UnregisterHotKey, MOD_ALT, MOD_CONTROL, MOD_SHIFT,
};
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW, GetMessageW,
    RegisterClassExW, TranslateMessage, HWND_MESSAGE, MSG, WINDOW_EX_STYLE, WM_HOTKEY, WNDCLASSEXW,
    WS_OVERLAPPED,
};

const HOTKEY_TOGGLE_CAPTURE: i32 = 1;
const HOTKEY_TOGGLE_EDITOR: i32 = 2;
const HOTKEY_CAPTURE_SCREENSHOT: i32 = 3;
const HOTKEY_PANIC_DISABLE: i32 = 4;
// VK_OEM_PERIOD = 0xBE — same key macOS binds Cmd+Shift+Opt+. to.
const VK_OEM_PERIOD: u32 = 0xBE;
static ACTION_SEQUENCE: AtomicU64 = AtomicU64::new(0);

pub fn next_action_sequence() -> u64 {
    ACTION_SEQUENCE
        .fetch_add(1, Ordering::SeqCst)
        .wrapping_add(1)
}

unsafe extern "system" fn wndproc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> windows::Win32::Foundation::LRESULT {
    DefWindowProcW(hwnd, msg, wparam, lparam)
}

/// Spawn a dedicated thread that owns a message-only window, requires all
/// four hotkeys to register, and forwards sequenced actions as Tauri events.
pub fn spawn(app: AppHandle) {
    thread::spawn(move || unsafe {
        let h_instance = match GetModuleHandleW(None) {
            Ok(h) => h,
            Err(e) => {
                tracing::error!("GetModuleHandleW failed: {e}");
                let _ = app.emit("hotkey-availability-changed", false);
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
            0,
            0,
            0,
            0,
            HWND_MESSAGE,
            None,
            h_instance,
            None,
        );
        let hwnd = match hwnd {
            Ok(h) => h,
            Err(e) => {
                tracing::error!("CreateWindowExW (HWND_MESSAGE) failed: {e}");
                let _ = app.emit("hotkey-availability-changed", false);
                return;
            }
        };

        let modifiers = MOD_CONTROL | MOD_SHIFT;
        let panic_modifiers = MOD_CONTROL | MOD_SHIFT | MOD_ALT;
        let bindings = [
            (HOTKEY_TOGGLE_CAPTURE, modifiers, 'L' as u32),
            (HOTKEY_TOGGLE_EDITOR, modifiers, 'B' as u32),
            (HOTKEY_CAPTURE_SCREENSHOT, modifiers, 'S' as u32),
            (HOTKEY_PANIC_DISABLE, panic_modifiers, VK_OEM_PERIOD),
        ];
        let mut registered = Vec::new();
        for (id, binding_modifiers, key) in bindings {
            match RegisterHotKey(hwnd, id, binding_modifiers, key) {
                Ok(()) => registered.push(id),
                Err(error) => {
                    tracing::error!("RegisterHotKey {id} failed: {error}");
                    for registered_id in registered {
                        let _ = UnregisterHotKey(hwnd, registered_id);
                    }
                    let _ = DestroyWindow(hwnd);
                    let _ = app.emit("hotkey-availability-changed", false);
                    return;
                }
            }
        }
        let _ = app.emit("hotkey-availability-changed", true);

        let mut msg = MSG::default();
        while GetMessageW(&mut msg, hwnd, 0, 0).as_bool() {
            if msg.message == WM_HOTKEY {
                match msg.wParam.0 as i32 {
                    HOTKEY_TOGGLE_CAPTURE => {
                        let _ = app.emit("hotkey-toggle-capture", next_action_sequence());
                    }
                    HOTKEY_TOGGLE_EDITOR => {
                        let _ = app.emit("hotkey-toggle-editor", next_action_sequence());
                    }
                    HOTKEY_CAPTURE_SCREENSHOT => {
                        let _ = app.emit("hotkey-capture-screenshot", next_action_sequence());
                    }
                    HOTKEY_PANIC_DISABLE => {
                        let _ = app.emit("hotkey-panic-disable", next_action_sequence());
                    }
                    _ => {}
                }
            }
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
        for id in registered {
            let _ = UnregisterHotKey(hwnd, id);
        }
        let _ = DestroyWindow(hwnd);
        let _ = app.emit("hotkey-availability-changed", false);
    });
}
