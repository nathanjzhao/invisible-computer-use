"""One fullscreen window on one named output, for the desktop-facade display probe (GTK3 on the Wayland GDK backend).
usage: wayland_output_receiver.py WORK TAG RRGGBB

The window is titled `icu receiver TAG` so the probe can pin it to an output with sway, paints a light
grey background and a 60x60 square in RRGGBB at the centre of the window, and logs every button,
motion, enter and wheel event to WORK/events-TAG.jsonl in the window's own (logical) coordinates with
the event time and state. WORK/ready-TAG.json says what the window got: its logical size, the GDK
scale factor of its monitor and the marker rectangle, so the probe can compare what the compositor
says with what the driver captured and delivered.
"""
import json, os, sys
from pathlib import Path

import gi
gi.require_version('Gtk', '3.0')
gi.require_version('Gdk', '3.0')
from gi.repository import Gdk, GLib, Gtk  # noqa: E402

work, TAG, COLOR = Path(sys.argv[1]), sys.argv[2], sys.argv[3]
RGB = tuple(int(COLOR[i:i + 2], 16) / 255 for i in (0, 2, 4))
mark = [0, 0, 60, 60]


def log(**e):
    with (work / f'events-{TAG}.jsonl').open('a') as f:
        f.write(json.dumps({'tag': TAG, **e}) + '\n')


win = Gtk.Window(title=f'icu receiver {TAG}')
area = Gtk.DrawingArea()
area.set_can_focus(True)
area.add_events(Gdk.EventMask.BUTTON_PRESS_MASK | Gdk.EventMask.BUTTON_RELEASE_MASK | Gdk.EventMask.POINTER_MOTION_MASK
                | Gdk.EventMask.ENTER_NOTIFY_MASK | Gdk.EventMask.SCROLL_MASK | Gdk.EventMask.SMOOTH_SCROLL_MASK)


def draw(w, cr):
    mark[0], mark[1] = w.get_allocated_width() // 2 - 30, w.get_allocated_height() // 2 - 30
    cr.set_source_rgb(0xDC / 255, 0xDC / 255, 0xDC / 255); cr.paint()
    cr.set_source_rgb(*RGB); cr.rectangle(*mark); cr.fill()
    return False


area.connect('draw', draw)
area.connect('button-press-event', lambda _w, e: (log(k='down', t=e.time, x=e.x, y=e.y, b=e.button, s=int(e.state)) if e.type == Gdk.EventType.BUTTON_PRESS else None) or True)
area.connect('button-release-event', lambda _w, e: log(k='up', t=e.time, x=e.x, y=e.y, b=e.button, s=int(e.state)) or True)
area.connect('motion-notify-event', lambda _w, e: log(k='move', t=e.time, x=e.x, y=e.y, s=int(e.state)) or True)
area.connect('enter-notify-event', lambda _w, e: log(k='enter', t=e.time, x=e.x, y=e.y, s=int(e.state)) or True)
area.connect('scroll-event', lambda _w, e: log(k='scroll', t=e.time, x=e.x, y=e.y, dir=e.direction.value_nick, dy=e.delta_y if e.direction == Gdk.ScrollDirection.SMOOTH else 0, s=int(e.state)) or True)
win.add(area)
win.fullscreen()
win.show_all()


def ready():
    gdk = win.get_window()
    if gdk is None or win.get_allocated_width() < 100:
        return True
    area.queue_draw()
    mon = Gdk.Display.get_default().get_monitor_at_window(gdk)
    geo = mon.get_geometry() if mon else None
    (work / f'ready-{TAG}.json').write_text(json.dumps({
        'pid': os.getpid(), 'tag': TAG, 'width': win.get_allocated_width(), 'height': win.get_allocated_height(), 'marker': mark,
        'scale_factor': win.get_scale_factor(), 'monitor_scale': mon.get_scale_factor() if mon else None,
        'monitor_geometry': [geo.x, geo.y, geo.width, geo.height] if geo else None, 'monitor_model': mon.get_model() if mon else None}))
    return True  # keep rewriting: the compositor may move and resize the window after it maps


GLib.timeout_add(500, ready)
GLib.timeout_add(100, lambda: (Gtk.main_quit(), False)[1] if (work / 'quit').exists() else True)
Gtk.main()
