"""GTK3 receiver for Wayland-session probes (works on the X11 and Wayland GDK backends).

Usage: wayland_grid_gtk.py WORK NAME

One window, a 2x3 homogeneous grid: row 0 is an entry spanning both columns,
rows 1-2 are buttons ``<NAME>-<row><col>``. Activations, entry text and window
activation changes are appended to WORK/receiver-events.jsonl. The ready file
WORK/fixture-<pid>.json records the GDK backend actually used and, on X11,
the window's XID.
"""
import json
import os
import sys
import time
from pathlib import Path

import gi
gi.require_version('Gtk', '3.0')
gi.require_version('Gdk', '3.0')
from gi.repository import Gdk, GLib, Gtk  # noqa: E402

work = Path(sys.argv[1]); name = sys.argv[2]


def log(row):
    row.update(window=name, pid=os.getpid(), time=time.monotonic())
    with (work / 'receiver-events.jsonl').open('a') as output:
        output.write(json.dumps(row) + '\n')


window = Gtk.Window(title=f'ICU Wayland {name}')
window.set_default_size(600, 360)
grid = Gtk.Grid(row_homogeneous=True, column_homogeneous=True)
entry = Gtk.Entry(hexpand=True, vexpand=True)
entry.connect('changed', lambda e: log({'kind': 'text', 'text': e.get_text()}))
grid.attach(entry, 0, 0, 2, 1)
for row in (1, 2):
    for col in (0, 1):
        label = f'{name}-{row}{col}'
        button = Gtk.Button(label=label, hexpand=True, vexpand=True)
        button.connect('clicked', lambda _b, label=label: log({'kind': 'clicked', 'button': label}))
        grid.attach(button, col, row, 1, 1)
window.add(grid)
window.connect('notify::is-active', lambda w, _p: log({'kind': 'active', 'active': w.is_active()}))
window.show_all()


def ready():
    gdk = window.get_window()
    if gdk is None:
        return True
    backend = type(Gdk.Display.get_default()).__name__
    xid = None
    if 'X11' in backend:
        gi.require_version('GdkX11', '3.0')
        from gi.repository import GdkX11  # noqa: F401
        xid = gdk.get_xid()
    (work / f'fixture-{os.getpid()}.json').write_text(json.dumps(
        {'pid': os.getpid(), 'name': name, 'backend': backend, 'xid': xid}))
    return False


GLib.timeout_add(200, ready)
Gtk.main()
