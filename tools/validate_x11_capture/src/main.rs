//! Exercise the port's actual XShm capture against a private test display.
use anyhow::{bail, ensure, Context, Result};
#[path = "../../../platform/linux/src-tauri/src/capture/types.rs"]
mod capture_types;
pub use capture_types::{CaptureSource, FrameView};
#[path = "../../../platform/linux/src-tauri/src/capture/x11.rs"]
mod x11;
#[path = "../../../platform/linux/src-tauri/src/capture/x11_pixels.rs"]
mod x11_pixels;
use x11rb::connection::Connection;
use x11rb::protocol::xproto::{ConnectionExt, CreateGCAux, Rectangle};

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<()> {
    if std::env::var("LIVEBLOCK_X11_TEST_DISPLAY").as_deref() != Ok("1") {
        bail!("Use xvfb-run and set LIVEBLOCK_X11_TEST_DISPLAY=1; this harness paints a private test display");
    }
    let (conn, index) = x11rb::connect(None)?;
    let screen = &conn.setup().roots[index];
    let visual = screen
        .allowed_depths
        .iter()
        .flat_map(|d| &d.visuals)
        .find(|v| v.visual_id == screen.root_visual)
        .context("root visual missing")?;
    let width = screen.width_in_pixels;
    let height = screen.height_in_pixels;
    ensure!(width >= 30 && height >= 10, "Test display too small");
    let colors = [visual.red_mask, visual.green_mask, visual.blue_mask];
    let gc = conn.generate_id()?;
    conn.create_gc(gc, screen.root, &CreateGCAux::new())?
        .check()?;
    for (i, color) in colors.iter().enumerate() {
        conn.change_gc(
            gc,
            &x11rb::protocol::xproto::ChangeGCAux::new().foreground(*color),
        )?
        .check()?;
        conn.poly_fill_rectangle(
            screen.root,
            gc,
            &[Rectangle {
                x: (i as u16 * width / 3) as i16,
                y: 0,
                width: width / 3,
                height,
            }],
        )?
        .check()?;
    }
    conn.flush()?;
    let mut capture = x11::X11Capture::new()?;
    ensure!(
        capture.uses_shared_memory(),
        "XShm is required for this integration check"
    );
    let frame = capture.next_frame().await?;
    ensure!(
        frame.width == width as u32 && frame.height == height as u32,
        "Incorrect captured dimensions"
    );
    ensure!(
        frame.stride == width as u32 * 4,
        "Capture is not packed BGRA8"
    );
    for (i, expected) in [[0, 0, 255, 255], [0, 255, 0, 255], [255, 0, 0, 255]]
        .iter()
        .enumerate()
    {
        let x = (i * 2 + 1) * width as usize / 6;
        let offset = (height as usize / 2 * width as usize + x) * 4;
        ensure!(
            &frame.pixels[offset..offset + 4] == expected,
            "Wrong pixel at stripe {i}: {:?}, expected {expected:?}; depth {}",
            &frame.pixels[offset..offset + 4],
            screen.root_depth
        );
    }
    capture.stop();
    conn.free_gc(gc)?.check()?;
    println!(
        "XShm capture passed: {width}×{height}, depth {}, opaque RGB stripes",
        screen.root_depth
    );
    Ok(())
}
