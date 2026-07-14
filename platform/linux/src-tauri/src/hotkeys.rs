//! Linux global shortcuts.
//! Wayland uses the permissioned GlobalShortcuts portal. X11 uses passive
//! XGrabKey registrations on the root and releases them when the connection
//! closes. Runtime availability is emitted separately from static capability.

use anyhow::{anyhow, Context, Result};
use ashpd::desktop::global_shortcuts::{GlobalShortcuts, NewShortcut};
use ashpd::WindowIdentifier;
use crossbeam_channel::Sender;
use futures_util::StreamExt;
use std::sync::atomic::Ordering;
use std::sync::Arc;
use tauri::{AppHandle, Emitter};

use crate::state::AppState;
use x11rb::connection::Connection;
use x11rb::protocol::xproto::{ConnectionExt as _, GrabMode, ModMask};
use x11rb::protocol::Event;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HotkeyAction {
    ToggleCapture,
    ToggleEditor,
    CaptureForLabeling,
    PanicDisable,
}

const TOGGLE_CAPTURE_ID: &str = "toggle_capture";
const TOGGLE_EDITOR_ID: &str = "toggle_editor";
const CAPTURE_LABEL_ID: &str = "capture_for_labeling";
const PANIC_ID: &str = "panic_disable";

pub fn spawn(app: AppHandle, actions: Sender<HotkeyAction>, state: Arc<AppState>) {
    match crate::session::detect_session() {
        crate::session::SessionType::Wayland => {
            tauri::async_runtime::spawn(async move {
                if let Err(error) = run_wayland_portal(actions, app.clone(), state.clone()).await {
                    report_unavailable(&app, &state, error);
                }
            });
        }
        crate::session::SessionType::X11 => {
            std::thread::spawn(move || {
                let availability_app = app.clone();
                let availability_state = state.clone();
                if let Err(error) = run_x11_grab(
                    actions,
                    || false,
                    move || set_availability(&availability_app, &availability_state, true),
                ) {
                    report_unavailable(&app, &state, error);
                }
            });
        }
        crate::session::SessionType::Unknown => {
            report_unavailable(&app, &state, anyhow!("unknown Linux display server"));
        }
    }
}

fn set_availability(app: &AppHandle, state: &AppState, available: bool) {
    state.hotkeys_available.store(available, Ordering::SeqCst);
    state.hotkeys_initialized.store(true, Ordering::SeqCst);
    let _ = app.emit("hotkey-availability-changed", available);
}

fn report_unavailable(app: &AppHandle, state: &AppState, error: anyhow::Error) {
    tracing::warn!("Linux global shortcuts unavailable: {error}");
    set_availability(app, state, false);
    let _ = app.emit("hotkey-runtime-error", error.to_string());
}

async fn run_wayland_portal(
    actions: Sender<HotkeyAction>,
    app: AppHandle,
    state: Arc<AppState>,
) -> Result<()> {
    let proxy = GlobalShortcuts::new()
        .await
        .context("create GlobalShortcuts portal proxy")?;
    let session = proxy
        .create_session()
        .await
        .context("create GlobalShortcuts portal session")?;
    let shortcuts = [
        NewShortcut::new(TOGGLE_CAPTURE_ID, "Toggle LiveBlock capture")
            .preferred_trigger("CTRL+SHIFT+l"),
        NewShortcut::new(TOGGLE_EDITOR_ID, "Toggle LiveBlock region editor")
            .preferred_trigger("CTRL+SHIFT+b"),
        NewShortcut::new(CAPTURE_LABEL_ID, "Capture a LiveBlock labeling frame")
            .preferred_trigger("CTRL+SHIFT+s"),
        NewShortcut::new(PANIC_ID, "Immediately disable LiveBlock")
            .preferred_trigger("CTRL+SHIFT+ALT+period"),
    ];
    let response = proxy
        .bind_shortcuts(&session, &shortcuts, &WindowIdentifier::default())
        .await
        .context("request GlobalShortcuts bindings")?
        .response()
        .context("GlobalShortcuts binding response")?;
    if response.shortcuts().len() != shortcuts.len() {
        let _ = session.close().await;
        return Err(anyhow!("portal did not bind every required shortcut"));
    }

    let mut activated = proxy
        .receive_activated()
        .await
        .context("listen for GlobalShortcuts activation")?;
    set_availability(&app, &state, true);
    // Availability means all bindings succeeded and the activation stream is
    // live. This loop owns proxy + session for the rest of app lifetime.
    while let Some(event) = activated.next().await {
        if let Some(action) = action_for_id(event.shortcut_id()) {
            if actions.send(action).is_err() {
                break;
            }
        }
    }
    let _ = session.close().await;
    Err(anyhow!("GlobalShortcuts activation stream ended"))
}

fn run_x11_grab(
    actions: Sender<HotkeyAction>,
    should_stop: impl Fn() -> bool,
    on_registered: impl FnOnce(),
) -> Result<()> {
    let (conn, screen_number) = x11rb::connect(None).context("connect X11 for global shortcuts")?;
    let root = conn.setup().roots[screen_number].root;
    let keys = resolve_x11_keycodes(&conn)?;
    let base = ModMask::CONTROL | ModMask::SHIFT;
    let panic = base | ModMask::M1;
    for (keycode, modifiers) in [
        (keys.toggle_capture, base),
        (keys.toggle_editor, base),
        (keys.capture_label, base),
        (keys.panic, panic),
    ] {
        // Passive grabs must include common lock combinations or shortcuts
        // silently stop working while CapsLock/NumLock is enabled.
        for locks in [
            ModMask::default(),
            ModMask::LOCK,
            ModMask::M2,
            ModMask::LOCK | ModMask::M2,
        ] {
            conn.grab_key(
                false,
                root,
                modifiers | locks,
                keycode,
                GrabMode::ASYNC,
                GrabMode::ASYNC,
            )?
            .check()
            .with_context(|| format!("XGrabKey conflict for keycode {keycode}"))?;
        }
    }
    conn.flush().context("flush X11 hotkey grabs")?;

    // Report availability before entering the blocking event loop.
    // The caller emits it when this function eventually returns, so send a
    // dedicated sentinel through tracing for real-time diagnostics.
    tracing::info!("X11 global shortcuts registered");
    on_registered();
    while !should_stop() {
        match conn.wait_for_event().context("wait for X11 hotkey event")? {
            Event::KeyPress(event) => {
                let action = if event.detail == keys.toggle_capture {
                    Some(HotkeyAction::ToggleCapture)
                } else if event.detail == keys.toggle_editor {
                    Some(HotkeyAction::ToggleEditor)
                } else if event.detail == keys.capture_label {
                    Some(HotkeyAction::CaptureForLabeling)
                } else if event.detail == keys.panic {
                    Some(HotkeyAction::PanicDisable)
                } else {
                    None
                };
                if let Some(action) = action {
                    if actions.send(action).is_err() {
                        return Ok(());
                    }
                }
            }
            _ => {}
        }
    }
    Ok(())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct X11Keycodes {
    toggle_capture: u8,
    toggle_editor: u8,
    capture_label: u8,
    panic: u8,
}

fn resolve_x11_keycodes<C: Connection>(conn: &C) -> Result<X11Keycodes> {
    let setup = conn.setup();
    let first = setup.min_keycode;
    let count = setup.max_keycode.saturating_sub(first).saturating_add(1);
    let reply = conn
        .get_keyboard_mapping(first, count)?
        .reply()
        .context("read X11 keyboard mapping")?;
    let find = |keysym: u32| {
        find_keycode(first, reply.keysyms_per_keycode, &reply.keysyms, keysym)
            .ok_or_else(|| anyhow!("X11 keyboard mapping has no keysym 0x{keysym:x}"))
    };
    Ok(X11Keycodes {
        toggle_capture: find('l' as u32)?,
        toggle_editor: find('b' as u32)?,
        capture_label: find('s' as u32)?,
        panic: find('.' as u32)?,
    })
}

fn find_keycode(first: u8, per_keycode: u8, keysyms: &[u32], target: u32) -> Option<u8> {
    if per_keycode == 0 {
        return None;
    }
    keysyms
        .chunks(usize::from(per_keycode))
        .position(|symbols| symbols.iter().any(|symbol| *symbol == target))
        .and_then(|index| first.checked_add(index as u8))
}

fn action_for_id(id: &str) -> Option<HotkeyAction> {
    match id {
        TOGGLE_CAPTURE_ID => Some(HotkeyAction::ToggleCapture),
        TOGGLE_EDITOR_ID => Some(HotkeyAction::ToggleEditor),
        CAPTURE_LABEL_ID => Some(HotkeyAction::CaptureForLabeling),
        PANIC_ID => Some(HotkeyAction::PanicDisable),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn portal_ids_map_only_to_locked_actions() {
        assert_eq!(
            action_for_id(TOGGLE_CAPTURE_ID),
            Some(HotkeyAction::ToggleCapture)
        );
        assert_eq!(action_for_id(PANIC_ID), Some(HotkeyAction::PanicDisable));
        assert_eq!(action_for_id("unknown"), None);
    }

    #[test]
    fn x11_mapping_resolves_keycodes_across_layout_levels() {
        let symbols = [0u32, 'l' as u32, 'b' as u32, 0, 's' as u32, '.' as u32];
        assert_eq!(find_keycode(20, 2, &symbols, 'l' as u32), Some(20));
        assert_eq!(find_keycode(20, 2, &symbols, 'b' as u32), Some(21));
        assert_eq!(find_keycode(20, 2, &symbols, '.' as u32), Some(22));
        assert_eq!(find_keycode(20, 2, &symbols, 'x' as u32), None);
    }
}
