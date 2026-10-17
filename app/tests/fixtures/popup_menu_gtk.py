"""GTK3 receiver with a real popup menu, for transient-surface capture probes (X11).

Usage: popup_menu_gtk.py WORK NAME X Y [--overlay X Y]

One window ``ICU Popup <NAME>`` at (X, Y) with a 2x2 button grid and a
Gtk.Menu (Copy / Paste / Delete) whose items are painted in distinct solid
colors. The menu is a GDK temp window, i.e. an override-redirect X window,
exactly what GTK3 maps for a context or menu-button menu. ``--overlay`` adds
an override-redirect Gtk POPUP window at root (X, Y) owned by this process,
used as another application's popup.

Commands are read from WORK/cmd-<NAME> (one word, then the file is removed):
``popup`` opens the menu at window-local (40, 40), ``popdown`` closes it,
``overlay``/``unoverlay`` map and unmap the overlay window.
Events (clicks, menu activations, menu map/unmap, is-active changes) go to
WORK/receiver-events.jsonl; WORK/fixture-<pid>.json lists the X11 ids.
"""
import json
import os
import sys
import time
from pathlib import Path

import gi
gi.require_version('Gtk', '3.0')
gi.require_version('Gdk', '3.0')
gi.require_version('GdkX11', '3.0')
from gi.repository import Gdk, GdkX11, GLib, Gtk  # noqa: E402,F401

work = Path(sys.argv[1]); name = sys.argv[2]; x, y = int(sys.argv[3]), int(sys.argv[4])
overlay_at = (int(sys.argv[6]), int(sys.argv[7])) if len(sys.argv) > 5 and sys.argv[5] == '--overlay' else None
COLORS = {'Copy': '#d03030', 'Paste': '#30a040', 'Delete': '#3050d0'}


def log(row):
    row.update(window=name, pid=os.getpid(), time=time.monotonic())
    with (work / 'receiver-events.jsonl').open('a') as output:
        output.write(json.dumps(row) + '\n')


css = Gtk.CssProvider()
css.load_from_data(''.join(f'#item-{k} {{ background: {v}; color: white; }}' for k, v in COLORS.items()).encode()
    + b'#overlay { background: #f0c020; }')
Gtk.StyleContext.add_provider_for_screen(Gdk.Screen.get_default(), css, Gtk.STYLE_PROVIDER_PRIORITY_USER)

window = Gtk.Window(title=f'ICU Popup {name}')
window.set_default_size(520, 320)
window.move(x, y)
grid = Gtk.Grid(row_homogeneous=True, column_homogeneous=True)
for row in range(2):
    for col in range(2):
        label = f'{name}-{row}{col}'
        button = Gtk.Button(label=label, hexpand=True, vexpand=True)
        button.connect('clicked', lambda _b, label=label: log({'kind': 'clicked', 'button': label}))
        grid.attach(button, col, row, 1, 1)
window.add(grid)
window.connect('notify::is-active', lambda w, _p: log({'kind': 'active', 'value': w.is_active()}))

menu = Gtk.Menu()
for item_name in COLORS:
    item = Gtk.MenuItem(label=item_name)
    item.set_name(f'item-{item_name}')
    item.connect('activate', lambda _i, n=item_name: log({'kind': 'menu_activate', 'item': n}))
    menu.append(item)
menu.show_all()
menu.connect('map', lambda *_: log({'kind': 'menu_map'}))
menu.connect('unmap', lambda *_: log({'kind': 'menu_unmap'}))

overlay = None
if overlay_at:
    overlay = Gtk.Window(type=Gtk.WindowType.POPUP)
    overlay.set_name('overlay')
    overlay.set_default_size(160, 90)
    overlay.move(*overlay_at)
    overlay.add(Gtk.Label(label=f'{name} overlay'))

window.show_all()


def menu_xid():
    toplevel = menu.get_toplevel().get_window()
    return toplevel.get_xid() if toplevel is not None else None


def poll():
    path = work / f'cmd-{name}'
    if path.exists():
        command = path.read_text().strip(); path.unlink()
        if command == 'popup':
            rect = Gdk.Rectangle(); rect.x, rect.y, rect.width, rect.height = 40, 40, 1, 1
            menu.popup_at_rect(window.get_window(), rect, Gdk.Gravity.NORTH_WEST, Gdk.Gravity.NORTH_WEST, None)
        elif command == 'popdown':
            menu.popdown()
        elif command == 'overlay' and overlay:
            overlay.show_all()
        elif command == 'unoverlay' and overlay:
            overlay.hide()
        GLib.timeout_add(150, report_ids)
    return True


def report_ids():
    gdk = window.get_window()
    if gdk is None:
        return True
    ids = {'pid': os.getpid(), 'name': name, 'window': gdk.get_xid(), 'menu': menu_xid(),
        'menu_visible': menu.get_visible(),
        'overlay': overlay.get_window().get_xid() if overlay and overlay.get_window() else None}
    (work / f'fixture-{os.getpid()}.json').write_text(json.dumps(ids))
    return False


GLib.timeout_add(200, report_ids)
GLib.timeout_add(100, poll)
Gtk.main()
