# Native X11 capture integration check

This imports the Linux port's actual capture implementation and frame contract.
It draws RGB stripes only on an isolated Xvfb display, reads them through real
SysV shared memory and XShm, and checks dimensions, stride, colors and opacity.
It refuses to run without an explicit test-display flag. Do not point it at a
personal desktop.

On Linux with Xvfb installed:

```sh
cargo build --manifest-path tools/validate_x11_capture/Cargo.toml
xvfb-run -a -s '-screen 0 320x240x24 +extension COMPOSITE +extension MIT-SHM' \
  env LIVEBLOCK_X11_TEST_DISPLAY=1 tools/validate_x11_capture/target/debug/liveblock-validate-x11-capture
```

Repeat with `320x240x16` to check a 16-bit visual. This tests capture, not native
overlay placement, self-exclusion, Wayland portals or hardware acceleration.
