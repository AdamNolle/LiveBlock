# Native Wayland overlay probe

This isolated GTK3 program tests the rendering approach needed to replace
LiveBlock's current floating Wayland renderer. It is not integrated into the app.

Run it on a disposable, single-output Wayland session that implements
`zwlr_layer_shell_v1`, such as Sway. It requires Python GObject introspection,
PyCairo, GTK3 and GtkLayerShell introspection bindings. On Ubuntu:

```sh
sudo apt install python3-gi python3-cairo python3-gi-cairo gir1.2-gtk-3.0 gir1.2-gtklayershell-0.1
python3 -u tools/validate_wayland_overlay/probe.py
```

Use an output with at least 800 by 400 logical pixels. The regular test window
is fullscreen; a separate native layer window draws a black 200 by 80 rectangle
at logical position (120, 150). The rest of the layer window stays transparent.

1. Click the black rectangle. The underlying counter must increase to 1.
2. Click **Hide cover**. The **Covered target** button must become visible.
3. Click **Show cover**. The black rectangle must return at the same position.
4. Click the black rectangle again. The underlying counter must increase to 2.

The window must initialize layer-shell before realization, anchor every edge,
use the overlay layer, request no keyboard focus, reserve no screen space, and
reapply an empty input region on each map. The first version passed initial
click-through but swallowed input after a hide/show cycle; the map callback
fixed that regression.

The actual Sway 1.11, GTK layer-shell 0.10.0 test is recorded in
[the native probe report](../../validation/wayland-native-layer-probe.json).
It used a 1280 by 800 headless output, scale 1 and a software renderer. This
proves the probe's placement and remap behavior, not LiveBlock integration,
portal capture, multiple outputs, fractional scaling or hardware GPU behavior.

The GTK initialization requirement is documented in the
[GTK layer-shell reference](https://wmww.github.io/gtk-layer-shell/).
The installed Tauri API has no documented pre-realization layer-shell hook; see the
[upstream builder issue](https://github.com/tauri-apps/tauri/issues/15913).
Before integrating, observe whether the actual hidden Tauri renderer is already
realized. GTK layer-shell can initialize it only while unrealized. Otherwise,
use a separately owned native GTK window from creation. Do not transplant an
already realized webview or treat a protocol probe as a working renderer.

During app integration, a native GTK realization trace on the actual hidden
Tauri renderer reported `already_realized=0 visible=0 mapped=0`. The current
source initializes GTK layer-shell before its explicit `realize()` call.
Compilation passed; the final release build and native application acceptance
are still pending. The prototype report remains scoped to the prototype.
