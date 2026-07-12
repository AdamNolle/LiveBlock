//! `zwlr_layer_shell_v1` overlay — works on Sway / Hyprland / river / KDE /
//! wlroots compositors. The production binding is not implemented yet, so
//! this adapter fails closed rather than claiming overlay protection.

use anyhow::{anyhow, Result};
use smithay_client_toolkit::reexports::client::Connection;

pub fn install() -> Result<()> {
    let _connection = Connection::connect_to_env()
        .map_err(|e| anyhow!("connect to wayland: {e}"))?;
    Err(anyhow!(
        "zwlr_layer_shell_v1 overlay installation is not implemented"
    ))
}
