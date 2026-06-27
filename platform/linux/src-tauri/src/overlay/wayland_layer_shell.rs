//! `zwlr_layer_shell_v1` overlay — works on Sway / Hyprland / river / KDE /
//! wlroots compositors. Anchors a surface to the whole output, assigns it to
//! the OVERLAY layer, makes it keyboard-inert and click-through (empty input
//! region), and never reserves screen space (exclusive_zone = -1).
//!
//! ARCHITECTURE NOTE (linux-port): the long-term plan (per the locked decision)
//! is a NATIVE GTK overlay that owns this layer surface directly and renders the
//! inpaint patches onto its own GPU surface — NEVER a webview/base64 path. This
//! module implements the layer-shell setup as a self-contained
//! smithay-client-toolkit client so the protocol wiring is real and testable;
//! the GTK rewrite (a follow-up) replaces the `draw`/buffer plumbing while
//! keeping this anchoring/input-region logic verbatim. Until then the patches
//! continue to be presented through the existing Tauri `render` window; this
//! function proves the layer surface negotiates correctly and is the seam the
//! native renderer plugs into.
//!
//! VERIFY-ON-LINUX(linux-port): exercises live Wayland globals; cannot run on
//! this Windows dev box. The bind list, layer/anchor/exclusive-zone config and
//! the empty-input-region call are the load-bearing bits to confirm against a
//! real wlroots/KWin compositor.

use anyhow::{anyhow, Result};
// Use the wayland-client / wayland-protocols-wlr crates DIRECTLY (both pinned in
// Cargo.toml at the versions smithay-client-toolkit 0.19 depends on, so the
// proxy types are identical). This avoids guessing sctk's reexport module paths.
use wayland_client::{
    globals::registry_queue_init,
    protocol::{wl_compositor::WlCompositor, wl_region::WlRegion, wl_surface::WlSurface},
    Connection, Dispatch, Proxy, QueueHandle,
};
use wayland_protocols_wlr::layer_shell::v1::client::{
    zwlr_layer_shell_v1::{Layer, ZwlrLayerShellV1},
    zwlr_layer_surface_v1::{Anchor, Event as LayerSurfaceEvent, KeyboardInteractivity, ZwlrLayerSurfaceV1},
};

/// Drives the layer surface to a configured, mapped state. Returns once the
/// surface has been created, anchored, and committed with an empty input
/// region. The caller (today: the Tauri setup hook) treats success as "overlay
/// strategy is live"; the native GTK renderer will later own the event loop and
/// keep this surface alive for the app's lifetime.
pub fn install() -> Result<()> {
    let conn = Connection::connect_to_env().map_err(|e| anyhow!("connect to wayland: {e}"))?;
    let (globals, mut queue) =
        registry_queue_init::<State>(&conn).map_err(|e| anyhow!("registry init: {e}"))?;
    let qh = queue.handle();

    // Bind the two globals we need: wl_compositor (to make a surface + region)
    // and zwlr_layer_shell_v1 (to promote the surface to the overlay layer).
    let compositor: WlCompositor = globals
        .bind(&qh, 1..=6, ())
        .map_err(|e| anyhow!("bind wl_compositor: {e}"))?;
    let layer_shell: ZwlrLayerShellV1 = globals
        .bind(&qh, 1..=4, ())
        .map_err(|_| anyhow!("compositor does not export zwlr_layer_shell_v1"))?;

    let surface = compositor.create_surface(&qh, ());

    // Promote to a layer surface on the OVERLAY layer (above normal windows).
    // `None` output = let the compositor place it on the current/default output.
    let layer_surface = layer_shell.get_layer_surface(
        &surface,
        None::<&wayland_client::protocol::wl_output::WlOutput>,
        Layer::Overlay,
        "liveblock-overlay".to_string(),
        &qh,
        (),
    );

    // Full-screen: anchor to all four edges so size follows the output.
    layer_surface.set_anchor(Anchor::Top | Anchor::Bottom | Anchor::Left | Anchor::Right);
    // 0,0 size + all anchors → compositor sizes us to the whole output.
    layer_surface.set_size(0, 0);
    // We are a pure overlay: take no keyboard focus.
    layer_surface.set_keyboard_interactivity(KeyboardInteractivity::None);
    // -1 → neither reserve space for ourselves nor get pushed by others.
    layer_surface.set_exclusive_zone(-1);

    // Click-through: an EMPTY input region means pointer/touch events fall
    // through to whatever is underneath. This is what makes the overlay
    // non-interactive so the user keeps using the app behind it.
    let empty_region: WlRegion = compositor.create_region(&qh, ());
    // (No `add` calls → the region covers nothing → no input is captured.)
    surface.set_input_region(Some(&empty_region));

    // Commit so the compositor sends the initial `configure`.
    surface.commit();

    let mut state = State {
        configured: false,
        compositor,
        surface,
        layer_surface,
        empty_region,
    };

    // Round-trip until the first configure arrives (or we give up). On the real
    // native renderer this becomes the persistent event loop; here we just prove
    // negotiation completes.
    for _ in 0..20 {
        queue
            .blocking_dispatch(&mut state)
            .map_err(|e| anyhow!("wayland dispatch: {e}"))?;
        if state.configured {
            break;
        }
    }
    if !state.configured {
        return Err(anyhow!("layer surface never configured"));
    }
    tracing::info!("wlr-layer-shell overlay configured (OVERLAY layer, click-through)");

    // NOTE: we intentionally keep `state` (and thus the surface) alive only for
    // the duration of negotiation in this seam implementation. The native GTK
    // renderer owns the long-lived surface + frame callbacks; wiring that is the
    // documented follow-up. Returning Ok here means "layer-shell path is viable".
    Ok(())
}

/// Client state for the layer-shell dispatch. Holds the protocol objects alive.
struct State {
    configured: bool,
    #[allow(dead_code)]
    compositor: WlCompositor,
    surface: WlSurface,
    layer_surface: ZwlrLayerSurfaceV1,
    #[allow(dead_code)]
    empty_region: WlRegion,
}

// --- Dispatch impls. Only the layer_surface configure carries state we act on.

impl Dispatch<ZwlrLayerSurfaceV1, ()> for State {
    fn event(
        state: &mut Self,
        layer_surface: &ZwlrLayerSurfaceV1,
        event: <ZwlrLayerSurfaceV1 as Proxy>::Event,
        _data: &(),
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
    ) {
        match event {
            LayerSurfaceEvent::Configure { serial, .. } => {
                // Ack the configure and commit; the surface is now mapped.
                layer_surface.ack_configure(serial);
                state.surface.commit();
                state.configured = true;
            }
            LayerSurfaceEvent::Closed => {
                // Compositor asked us to go away (e.g. output removed).
                state.configured = false;
            }
            _ => {}
        }
    }
}

// The remaining globals produce no events we care about; empty impls satisfy
// the wayland-client Dispatch requirement.
impl Dispatch<ZwlrLayerShellV1, ()> for State {
    fn event(
        _: &mut Self,
        _: &ZwlrLayerShellV1,
        _: <ZwlrLayerShellV1 as Proxy>::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}

impl Dispatch<WlCompositor, ()> for State {
    fn event(
        _: &mut Self,
        _: &WlCompositor,
        _: <WlCompositor as Proxy>::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}

impl Dispatch<WlSurface, ()> for State {
    fn event(
        _: &mut Self,
        _: &WlSurface,
        _: <WlSurface as Proxy>::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}

impl Dispatch<WlRegion, ()> for State {
    fn event(
        _: &mut Self,
        _: &WlRegion,
        _: <WlRegion as Proxy>::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}
