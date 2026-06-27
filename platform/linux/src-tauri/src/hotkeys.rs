//! Global hotkeys.
//!
//! Wayland: xdg-desktop-portal `org.freedesktop.portal.GlobalShortcuts` via
//! `ashpd` (the user grants access once; we persist the session). Some portal
//! backends let the COMPOSITOR pick the actual key combos, so the trigger
//! definitions are advisory.
//!
//! X11: classic `XGrabKey` on the root window + an XEvent pump thread.
//!
//! PANIC-DISABLE GUARANTEE: `PanicDisable` (Ctrl+Shift+Alt+.) must ALWAYS be
//! able to stop all overlay output, even if the portal denied the other
//! shortcuts or the grab partly failed. On X11 we grab the panic combo FIRST
//! and treat its grab as mandatory; on Wayland it is registered as the first
//! shortcut and the receiver loop in `main.rs` handles it unconditionally.

use anyhow::{anyhow, Context, Result};
use crossbeam_channel::Sender;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HotkeyAction {
    ToggleCapture,      // Ctrl+Shift+L
    ToggleEditor,       // Ctrl+Shift+B
    CaptureForLabeling, // Ctrl+Shift+S
    PanicDisable,       // Ctrl+Shift+Alt+Period
}

/// Install global hotkeys for the active display server. Spawns a background
/// listener that forwards `HotkeyAction`s on `tx`. The receiver loop lives in
/// `main.rs` (so it can touch Tauri state / emit events).
pub fn install(tx: Sender<HotkeyAction>) -> Result<()> {
    match crate::session::detect_session() {
        crate::session::SessionType::Wayland => install_wayland_portal(tx),
        crate::session::SessionType::X11 | crate::session::SessionType::Unknown => {
            install_x11_grab(tx)
        }
    }
}

// ===========================================================================
// Wayland: xdg-desktop-portal GlobalShortcuts (ashpd)
// ===========================================================================

fn install_wayland_portal(tx: Sender<HotkeyAction>) -> Result<()> {
    // ashpd is async; run a dedicated single-thread tokio runtime for the
    // portal session + signal listener so we don't depend on the Tauri runtime
    // being available at install time.
    std::thread::Builder::new()
        .name("hotkeys-portal".into())
        .spawn(move || {
            let rt = match tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
            {
                Ok(rt) => rt,
                Err(e) => {
                    tracing::error!("hotkeys portal runtime: {e:#}");
                    return;
                }
            };
            if let Err(e) = rt.block_on(wayland_portal_loop(tx)) {
                tracing::warn!("GlobalShortcuts portal unavailable: {e:#}");
            }
        })
        .context("spawn hotkeys-portal thread")?;
    Ok(())
}

async fn wayland_portal_loop(tx: Sender<HotkeyAction>) -> Result<()> {
    use ashpd::desktop::global_shortcuts::{GlobalShortcuts, NewShortcut};
    use futures_util::StreamExt;

    let shortcuts = GlobalShortcuts::new()
        .await
        .context("GlobalShortcuts proxy")?;
    let session = shortcuts
        .create_session()
        .await
        .context("create_session")?;

    // The trigger strings are portal "shortcut" syntax ("CTRL+SHIFT+l"). Backends
    // that prefer to let the user choose ignore the preferred trigger; the IDs
    // are what we map back to actions.
    // VERIFY-ON-LINUX(linux-port): `preferred_trigger` takes `Option<&str>` in
    // ashpd 0.9. The trigger syntax is the portal's ("CTRL+SHIFT+l"); some
    // backends ignore it and let the user choose, in which case the IDs are what
    // matter for dispatch.
    let defs = [
        // Panic FIRST so it's the most likely to register if the backend caps
        // the number of shortcuts.
        NewShortcut::new("panic_disable", "Panic: disable all overlays")
            .preferred_trigger(Some("CTRL+SHIFT+ALT+period")),
        NewShortcut::new("toggle_capture", "Toggle capture")
            .preferred_trigger(Some("CTRL+SHIFT+l")),
        NewShortcut::new("toggle_editor", "Toggle region editor")
            .preferred_trigger(Some("CTRL+SHIFT+b")),
        NewShortcut::new("capture_for_labeling", "Capture screenshot for labeling")
            .preferred_trigger(Some("CTRL+SHIFT+s")),
    ];

    shortcuts
        .bind_shortcuts(&session, &defs, None)
        .await
        .context("bind_shortcuts")?;

    // Listen for activations. `activated()` yields a stream of events carrying
    // the shortcut id; map id → action and forward.
    let mut activated = shortcuts
        .receive_activated()
        .await
        .context("receive_activated")?;

    tracing::info!("GlobalShortcuts portal bound; listening for activations");
    while let Some(evt) = activated.next().await {
        let id = evt.shortcut_id();
        let action = match id {
            "panic_disable" => Some(HotkeyAction::PanicDisable),
            "toggle_capture" => Some(HotkeyAction::ToggleCapture),
            "toggle_editor" => Some(HotkeyAction::ToggleEditor),
            "capture_for_labeling" => Some(HotkeyAction::CaptureForLabeling),
            other => {
                tracing::debug!("unknown shortcut id {other}");
                None
            }
        };
        if let Some(a) = action {
            if tx.send(a).is_err() {
                break; // receiver gone; app shutting down
            }
        }
    }
    Ok(())
}

// ===========================================================================
// X11: XGrabKey on the root window
// ===========================================================================

fn install_x11_grab(tx: Sender<HotkeyAction>) -> Result<()> {
    std::thread::Builder::new()
        .name("hotkeys-x11".into())
        .spawn(move || {
            if let Err(e) = x11_grab_loop(tx) {
                tracing::error!("X11 hotkeys loop: {e:#}");
            }
        })
        .context("spawn hotkeys-x11 thread")?;
    Ok(())
}

fn x11_grab_loop(tx: Sender<HotkeyAction>) -> Result<()> {
    use x11rb::connection::Connection;
    use x11rb::protocol::xproto::{ConnectionExt as _, GrabMode, ModMask};
    use x11rb::protocol::Event;

    let (conn, screen_num) = x11rb::connect(None).context("X11 connect")?;
    let screen = &conn.setup().roots[screen_num];
    let root = screen.root;

    // X11 modifier bits (raw, per the core protocol): Shift=1, Lock=2,
    // Control=4, Mod1/Alt=8, Mod2/NumLock=16. We work in raw u16 throughout to
    // avoid bitmask-newtype conversion churn across x11rb patch releases.
    const SHIFT: u16 = 1 << 0;
    const LOCK: u16 = 1 << 1;
    const CONTROL: u16 = 1 << 2;
    const MOD1: u16 = 1 << 3; // Alt
    const MOD2: u16 = 1 << 4; // NumLock

    // Map our combos to X keysyms (resolved to keycodes below).
    //   l=0x6c, b=0x62, s=0x73, period=0x2e
    let combos = [
        (HotkeyAction::PanicDisable, 0x002e_u32, CONTROL | SHIFT | MOD1),
        (HotkeyAction::ToggleCapture, 0x006c_u32, CONTROL | SHIFT),
        (HotkeyAction::ToggleEditor, 0x0062_u32, CONTROL | SHIFT),
        (HotkeyAction::CaptureForLabeling, 0x0073_u32, CONTROL | SHIFT),
    ];

    let keycode_for = build_keysym_map(&conn)?;

    // (keycode, base modifiers) → action, for dispatch in the event pump.
    let mut bound: Vec<(u8, u16, HotkeyAction)> = Vec::new();
    let mut panic_bound = false;
    for (action, keysym, base) in combos {
        let Some(&keycode) = keycode_for.get(&keysym) else {
            tracing::warn!("no keycode for keysym {keysym:#x}; skipping {action:?}");
            continue;
        };
        // Grab with the four lock-key permutations (NumLock/CapsLock) so the
        // hotkey fires regardless of lock state.
        let lock_variants = [0u16, LOCK, MOD2, LOCK | MOD2];
        let mut ok_any = false;
        for extra in lock_variants {
            let m = ModMask::from(base | extra);
            match conn.grab_key(true, root, m, keycode, GrabMode::ASYNC, GrabMode::ASYNC) {
                Ok(cookie) => match cookie.check() {
                    Ok(()) => ok_any = true,
                    Err(_) => tracing::debug!("grab_key check failed for {action:?}"),
                },
                Err(_) => tracing::debug!("grab_key request failed for {action:?}"),
            }
        }
        if ok_any {
            bound.push((keycode, base, action));
            if action == HotkeyAction::PanicDisable {
                panic_bound = true;
            }
        }
    }

    conn.flush().ok();

    if !panic_bound {
        // PANIC-DISABLE GUARANTEE: if even the panic combo couldn't be grabbed
        // (another app holds it), surface it loudly. The tray "Quit" and the
        // control window remain as the manual kill switch.
        tracing::error!(
            "could not grab panic-disable hotkey (Ctrl+Shift+Alt+.) — use the tray to disable"
        );
    }

    // Pump key events forever.
    loop {
        let event = match conn.wait_for_event() {
            Ok(e) => e,
            Err(e) => {
                tracing::warn!("X11 event wait failed: {e:#}");
                break;
            }
        };
        if let Event::KeyPress(kp) = event {
            // Strip the lock bits (CapsLock/NumLock) from the reported state
            // before matching so the combo fires regardless of lock state.
            let lock_bits = LOCK | MOD2;
            let state = u16::from(kp.state) & !lock_bits;
            if let Some((_, _, action)) = bound
                .iter()
                .find(|(kc, base, _)| *kc == kp.detail && state == *base)
            {
                if tx.send(*action).is_err() {
                    break;
                }
            }
        }
    }
    Ok(())
}

/// Build a keysym → keycode lookup from the server's keyboard mapping. We only
/// need a handful of keys, so a linear scan of the first keysym column is fine.
fn build_keysym_map(
    conn: &x11rb::rust_connection::RustConnection,
) -> Result<std::collections::HashMap<u32, u8>> {
    use x11rb::protocol::xproto::ConnectionExt as _;

    let setup = conn.setup();
    let min = setup.min_keycode;
    let max = setup.max_keycode;
    let count = max - min + 1;
    let mapping = conn
        .get_keyboard_mapping(min, count)
        .context("get_keyboard_mapping request")?
        .reply()
        .context("GetKeyboardMapping")?;

    let per = mapping.keysyms_per_keycode as usize;
    let mut map = std::collections::HashMap::new();
    for (i, chunk) in mapping.keysyms.chunks(per).enumerate() {
        let keycode = min as usize + i;
        if keycode > u8::MAX as usize {
            break;
        }
        // Column 0 is the unshifted keysym — that's what our lowercase keysyms
        // (l/b/s/period) live in.
        if let Some(&ks) = chunk.first() {
            map.entry(ks).or_insert(keycode as u8);
        }
    }
    if map.is_empty() {
        return Err(anyhow!("empty keyboard mapping"));
    }
    Ok(map)
}
