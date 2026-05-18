//! `zwlr_layer_shell_v1` overlay — works on Sway / Hyprland / river / KDE /
//! wlroots compositors. Anchors the surface to the whole output and assigns
//! it to the OVERLAY layer with an empty input region (click-through).

use anyhow::{anyhow, Result};
use smithay_client_toolkit::reexports::client::{
    globals::registry_queue_init, Connection, QueueHandle,
};

pub fn install() -> Result<()> {
    let conn = Connection::connect_to_env()
        .map_err(|e| anyhow!("connect to wayland: {e}"))?;
    let (_globals, mut _qh) = registry_queue_init::<State>(&conn)
        .map_err(|e| anyhow!("registry init: {e}"))?;

    // TODO(linux-port): bind zwlr_layer_shell_v1 + zwlr_layer_surface_v1,
    // create a layer surface with:
    //   - layer = OVERLAY
    //   - keyboard_interactivity = NONE
    //   - exclusive_zone = -1 (don't reserve, don't be reserved)
    //   - anchor = TOP|BOTTOM|LEFT|RIGHT (full-screen)
    // Then set wl_surface::set_input_region(empty) for click-through.
    Ok(())
}

struct State;
