"""One override-redirect X11 window over a rectangle of the root, for the desktop-facade display probe.
usage: x11_monitor_receiver.py WORK TAG RRGGBB X Y W H

The window covers (X, Y, W, H) in root coordinates, paints a light grey background and a 60x60 square
in RRGGBB at its centre, and logs every button, motion and wheel event to WORK/events-TAG.jsonl with the
server timestamp, the root coordinates and the state mask. WORK/ready-TAG.json carries the marker
rectangle in root coordinates, so a click at the marker's centre, taken from a screenshot, can be
checked against what the server delivered.
"""
import json, os, sys, time
from pathlib import Path
from Xlib import X, display

work, TAG, COLOR = Path(sys.argv[1]), sys.argv[2], sys.argv[3]
x0, y0, w0, h0 = (int(a) for a in sys.argv[4:8])
c = display.Display()
screen = c.screen()
root = screen.root
win = root.create_window(
    x0, y0, w0, h0, 0, screen.root_depth, X.InputOutput, X.CopyFromParent,
    background_pixel=0xDCDCDC, override_redirect=1,
    event_mask=X.ExposureMask | X.ButtonPressMask | X.ButtonReleaseMask | X.PointerMotionMask)
win.map()
gc = win.create_gc(foreground=int(COLOR, 16))
MARK = (w0 // 2 - 30, h0 // 2 - 30, 60, 60)
win.fill_rectangle(gc, *MARK)
c.sync()


def log(**e):
    with (work / f'events-{TAG}.jsonl').open('a') as f:
        f.write(json.dumps({'tag': TAG, **e}) + '\n')


(work / f'ready-{TAG}.json').write_text(json.dumps({'pid': os.getpid(), 'tag': TAG, 'window': [x0, y0, w0, h0], 'marker': [x0 + MARK[0], y0 + MARK[1], 60, 60]}))
quit_file = work / 'quit'
while not quit_file.exists():
    while c.pending_events():
        ev = c.next_event()
        if ev.type == X.Expose:
            win.fill_rectangle(gc, *MARK)
        elif ev.type in (X.ButtonPress, X.ButtonRelease):
            log(k='down' if ev.type == X.ButtonPress else 'up', t=ev.time, x=ev.root_x, y=ev.root_y, b=ev.detail, s=ev.state)
        elif ev.type == X.MotionNotify:
            log(k='move', t=ev.time, x=ev.root_x, y=ev.root_y, s=ev.state)
    time.sleep(0.01)
