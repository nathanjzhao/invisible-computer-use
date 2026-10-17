"""GTK3 receiver for multi-display capture/input agreement probes.

Usage: display_grid_gtk.py WORK NAME[:X:Y:W:H] [NAME:X:Y:W:H ...]

Every named window lives in this one process and has the same 3x2 grid of
buttons, so a window-local point is ambiguous across windows unless the
driver binds it to the requested window. Each button's label is
``<window>-<row><col>``. Activations and raw presses are appended to
WORK/receiver-events.jsonl; WORK/fixture-<pid>.json lists the X11 ids.
"""
import json
import os
import sys
import time
from pathlib import Path

import gi
gi.require_version('Gtk', '3.0')
gi.require_version('GdkX11', '3.0')
from gi.repository import GdkX11, GLib, Gtk  # noqa: E402,F401

work = Path(sys.argv[1])
specs = []
for arg in sys.argv[2:]:
    name, x, y, w, h = arg.split(':')
    specs.append((name, int(x), int(y), int(w), int(h)))
windows = {}


def log(row):
    window = windows.get(row['window'])
    row.update(pid=os.getpid(), scale=window.get_scale_factor() if window else None, time=time.monotonic())
    with (work / 'receiver-events.jsonl').open('a') as output:
        output.write(json.dumps(row) + '\n')


def build(name, x, y, width, height):
    window = Gtk.Window(title=f'ICU Display {name}')
    window.set_default_size(width, height)
    window.move(x, y)
    grid = Gtk.Grid(row_homogeneous=True, column_homogeneous=True)
    for row in range(2):
        for col in range(3):
            label = f'{name}-{row}{col}'
            button = Gtk.Button(label=label, hexpand=True, vexpand=True)
            button.connect('clicked', lambda _b, label=label: log({'kind': 'clicked', 'window': name, 'button': label}))
            button.connect('button-press-event', lambda _b, event, label=label: log({
                'kind': 'press', 'window': name, 'button': label, 'x': event.x, 'y': event.y,
                'root_x': event.x_root, 'root_y': event.y_root, 'synthetic': bool(event.send_event)}) or False)
            grid.attach(button, col, row, 1, 1)
    window.add(grid)
    window.connect('destroy', lambda *_: windows.pop(name, None))
    # Polling samplers cannot see a brief focus loss (e.g. a keyboard grab
    # probe); the toolkit's own active-state notifications can.
    window.connect('notify::is-active', lambda w, _p: log({'kind': 'active', 'window': name, 'button': None, 'value': w.is_active()}))
    window.show_all()
    windows[name] = window


for spec in specs:
    build(*spec)


def ready():
    ids = {}
    for name, window in windows.items():
        gdk = window.get_window()
        if gdk is None:
            return True
        ids[name] = gdk.get_xid()
    if len(ids) != len(specs):
        return True
    path = work / f'fixture-{os.getpid()}.json'
    path.write_text(json.dumps({'pid': os.getpid(), 'windows': ids,
        'scale': {n: w.get_scale_factor() for n, w in windows.items()}}))
    return False


GLib.timeout_add(200, ready)
Gtk.main()
