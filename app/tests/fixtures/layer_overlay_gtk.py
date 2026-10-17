"""One process, two surfaces: a normal toplevel (a sway container) and a magenta
overlay layer-shell surface at (150,150) 300x150 (the round-2 reviewer's layer_and_window.py).
usage: layer_overlay_gtk.py READYFILE [nowindow]"""
import json, os, sys
import gi
gi.require_version('Gtk', '3.0'); gi.require_version('GtkLayerShell', '0.1')
from gi.repository import Gtk, GtkLayerShell, GLib, Gdk
if len(sys.argv) < 3 or sys.argv[2] != 'nowindow':
    top = Gtk.Window(title='decoy toplevel'); top.set_default_size(200, 100); top.add(Gtk.Label(label='decoy')); top.show_all()
layer = Gtk.Window(); GtkLayerShell.init_for_window(layer)
GtkLayerShell.set_layer(layer, GtkLayerShell.Layer.OVERLAY)
GtkLayerShell.set_anchor(layer, GtkLayerShell.Edge.TOP, True); GtkLayerShell.set_anchor(layer, GtkLayerShell.Edge.LEFT, True)
GtkLayerShell.set_margin(layer, GtkLayerShell.Edge.TOP, 150); GtkLayerShell.set_margin(layer, GtkLayerShell.Edge.LEFT, 150)
layer.set_size_request(300, 150)
css = Gtk.CssProvider(); css.load_from_data(b'window { background-color: #ff00ff; }')
Gtk.StyleContext.add_provider_for_screen(Gdk.Screen.get_default(), css, Gtk.STYLE_PROVIDER_PRIORITY_APPLICATION)
layer.show_all()
GLib.timeout_add(700, lambda: open(sys.argv[1], 'w').write(json.dumps({'pid': os.getpid()})) and False)
Gtk.main()
