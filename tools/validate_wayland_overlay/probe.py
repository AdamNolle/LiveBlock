import gi, cairo

gi.require_version('Gtk', '3.0')
gi.require_version('GtkLayerShell', '0.1')
from gi.repository import Gtk, Gdk, GtkLayerShell

assert GtkLayerShell.is_supported(), 'layer-shell unavailable'
base = Gtk.Window(title='LiveBlock native layer-shell probe')
base.fullscreen()
base.connect('destroy', Gtk.main_quit)
fixed = Gtk.Fixed()
base.add(fixed)
heading = Gtk.Label(label='LiveBlock native Wayland overlay probe')
fixed.put(heading, 120, 60)
counter = Gtk.Label(label='Clicks through cover: 0')
fixed.put(counter, 120, 100)
target = Gtk.Button(label='Covered target')
target.set_size_request(200, 80)
fixed.put(target, 120, 150)
clicks = 0

def clicked(_):
    global clicks
    clicks += 1
    counter.set_text(f'Clicks through cover: {clicks}')
    print(f'covered-target-click={clicks}', flush=True)
target.connect('clicked', clicked)

layer = Gtk.Window(title='LiveBlock probe layer')
layer.set_decorated(False)
layer.set_app_paintable(True)
layer.set_visual(layer.get_screen().get_rgba_visual())
GtkLayerShell.init_for_window(layer)
GtkLayerShell.set_namespace(layer, 'liveblock-probe')
GtkLayerShell.set_layer(layer, GtkLayerShell.Layer.OVERLAY)
GtkLayerShell.set_keyboard_mode(layer, GtkLayerShell.KeyboardMode.NONE)
GtkLayerShell.set_exclusive_zone(layer, -1)
for edge in (GtkLayerShell.Edge.TOP, GtkLayerShell.Edge.BOTTOM, GtkLayerShell.Edge.LEFT, GtkLayerShell.Edge.RIGHT):
    GtkLayerShell.set_anchor(layer, edge, True)
layer.input_shape_combine_region(cairo.Region())

def draw(widget, cr):
    cr.set_operator(cairo.OPERATOR_SOURCE)
    cr.set_source_rgba(0, 0, 0, 0)
    cr.paint()
    cr.set_operator(cairo.OPERATOR_OVER)
    cr.set_source_rgba(0, 0, 0, 1)
    cr.rectangle(120, 150, 200, 80)
    cr.fill()
    return True
layer.connect('draw', draw)
layer.connect('map', lambda w: w.input_shape_combine_region(cairo.Region()))
layer.connect('map', lambda w: print(f'layer-mapped width={w.get_allocated_width()} height={w.get_allocated_height()}', flush=True))
layer.connect('unmap', lambda _: print('layer-unmapped', flush=True))

def show(_):
    layer.show_all()
    print('show-cover-request', flush=True)
def hide(_):
    layer.hide()
    print('hide-cover-request', flush=True)
for label, x, action in [('Hide cover', 400, hide), ('Show cover', 600, show)]:
    button=Gtk.Button(label=label)
    button.set_size_request(160,80)
    button.connect('clicked',action)
    fixed.put(button,x,150)
base.show_all()
layer.show_all()
print(f'layer-window={GtkLayerShell.is_layer_window(layer)}', flush=True)
Gtk.main()
