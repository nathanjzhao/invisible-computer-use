"""Whole-output Wayland receiver for the full-desktop facade probe (GTK3 on the Wayland GDK backend).
usage: wayland_desktop_receiver.py WORK

A fullscreen window with a light grey background and one 60x60 red square at (700, 500). Under a
headless sway the window covers the output at (0, 0), so event coordinates are desktop coordinates.
Every button, motion and key event is appended to WORK/events.jsonl with the event timestamp (ms),
the coordinates and the state mask, the same record the X11 receiver writes.
"""
import json, os, sys, time
from pathlib import Path

import gi
gi.require_version('Gtk', '3.0')
gi.require_version('Gdk', '3.0')
from gi.repository import Gdk, GLib, Gtk  # noqa: E402

work = Path(sys.argv[1])
MARK = (700, 500, 60, 60)


def log(**e):
    with (work / 'events.jsonl').open('a') as f:
        f.write(json.dumps(e) + '\n')


win = Gtk.Window(title='icu desktop receiver')
area = Gtk.DrawingArea()
area.set_can_focus(True)
area.add_events(Gdk.EventMask.BUTTON_PRESS_MASK | Gdk.EventMask.BUTTON_RELEASE_MASK | Gdk.EventMask.POINTER_MOTION_MASK
                | Gdk.EventMask.KEY_PRESS_MASK | Gdk.EventMask.KEY_RELEASE_MASK | Gdk.EventMask.ENTER_NOTIFY_MASK
                | Gdk.EventMask.SCROLL_MASK | Gdk.EventMask.SMOOTH_SCROLL_MASK)


def draw(_w, cr):
    cr.set_source_rgb(0xDC / 255, 0xDC / 255, 0xDC / 255); cr.paint()
    cr.set_source_rgb(1, 0, 0); cr.rectangle(*MARK); cr.fill()
    return False


area.connect('draw', draw)
# GTK3 also delivers GDK_2BUTTON_PRESS / GDK_3BUTTON_PRESS as button-press-event for the same server press; count only the real one
area.connect('button-press-event', lambda _w, e: (log(k='down', t=e.time, x=e.x, y=e.y, b=e.button, s=int(e.state)) if e.type == Gdk.EventType.BUTTON_PRESS else None) or True)
area.connect('button-release-event', lambda _w, e: log(k='up', t=e.time, x=e.x, y=e.y, b=e.button, s=int(e.state)) or True)
area.connect('motion-notify-event', lambda _w, e: log(k='move', t=e.time, x=e.x, y=e.y, s=int(e.state)) or True)
def keychar(e):
    u = Gdk.keyval_to_unicode(e.keyval)
    return chr(u) if u >= 0x20 else ''


area.connect('key-press-event', lambda _w, e: log(k='kdown', t=e.time, sym=Gdk.keyval_name(e.keyval), ch=keychar(e), s=int(e.state)) or True)
area.connect('key-release-event', lambda _w, e: log(k='kup', t=e.time, sym=Gdk.keyval_name(e.keyval), ch=keychar(e), s=int(e.state)) or True)
# a wheel turn: dy > 0 is down, dx > 0 is right; a discrete wheel gives whole steps, a smooth one fractions
area.connect('scroll-event', lambda _w, e: log(k='scroll', t=e.time, x=e.x, y=e.y, dir=e.direction.value_nick if e.direction != Gdk.ScrollDirection.SMOOTH else 'smooth',
                                               dx=e.delta_x if e.direction == Gdk.ScrollDirection.SMOOTH else 0, dy=e.delta_y if e.direction == Gdk.ScrollDirection.SMOOTH else 0, s=int(e.state)) or True)
area.connect('enter-notify-event', lambda _w, e: log(k='enter', t=e.time, x=e.x, y=e.y, s=int(e.state)) or True)
win.add(area)
win.fullscreen()
win.show_all()
area.grab_focus()


def ready():
    gdk = win.get_window()
    if gdk is None or win.get_allocated_width() < 100:
        return True
    (work / 'ready.json').write_text(json.dumps({'pid': os.getpid(), 'width': win.get_allocated_width(), 'height': win.get_allocated_height(),
                                                 'marker': MARK, 'backend': type(Gdk.Display.get_default()).__name__}))
    return False


GLib.timeout_add(100, ready)
GLib.timeout_add(100, lambda: (Gtk.main_quit(), False)[1] if (work / 'quit').exists() else True)
Gtk.main()
