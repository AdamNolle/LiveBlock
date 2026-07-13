//! Library entry for the Windows Tauri port. The runnable binary lives in
//! `main.rs`; this lib.rs exists so `cargo` can satisfy the `[lib]` manifest
//! entry that other tooling (e.g. tauri-build, mobile entry-point patterns)
//! expects.

#[cfg(windows)]
pub mod capture;
pub mod capture_policy;
pub mod detection;
#[cfg(windows)]
pub mod hotkeys;
pub mod inpainting;
pub mod labels;
#[cfg(windows)]
pub mod overlay;
pub mod paths;
pub mod regions;
#[cfg(windows)]
pub mod state;
pub mod training;
#[cfg(windows)]
pub mod tray;
