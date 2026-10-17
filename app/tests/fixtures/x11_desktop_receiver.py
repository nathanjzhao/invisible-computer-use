"""Whole-screen X11 receiver for the full-desktop facade probe.
usage: x11_desktop_receiver.py WORK

One override-redirect window covers the screen at (0, 0), so a desktop coordinate is a window
coordinate and every pointer or key event the server delivers lands here. It paints a light grey
background with one 60x60 red square at (700, 500), the marker the probe finds in a screenshot to
prove screenshot pixels and click pixels are the same pixels. Every button, motion and key event is
appended to WORK/events.jsonl with the server timestamp (ms), the root coordinates and the modifier
and button state mask, so a hold time, a path and a held key can be read back from what the X server
really delivered, not from what the driver says it sent.
"""
import json, os, sys, time
from pathlib import Path
from Xlib import X, display

work = Path(sys.argv[1])
c = display.Display()
screen = c.screen()
root = screen.root
geo = root.get_geometry()
W, H = geo.width, geo.height
win = root.create_window(
    0, 0, W, H, 0, screen.root_depth, X.InputOutput, X.CopyFromParent,
    background_pixel=0xDCDCDC, override_redirect=1,
    event_mask=X.ExposureMask | X.ButtonPressMask | X.ButtonReleaseMask | X.PointerMotionMask | X.KeyPressMask | X.KeyReleaseMask)
win.map()
gc = win.create_gc(foreground=0xFF0000)
MARK = (700, 500, 60, 60)
win.fill_rectangle(gc, *MARK)
c.set_input_focus(win, X.RevertToParent, X.CurrentTime)
c.sync()


def log(**e):
    with (work / 'events.jsonl').open('a') as f:
        f.write(json.dumps(e) + '\n')


(work / 'ready.json').write_text(json.dumps({'pid': os.getpid(), 'width': W, 'height': H, 'marker': MARK}))
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
        elif ev.type in (X.KeyPress, X.KeyRelease):
            # the character this key produces under the modifier state it was pressed with (shift level only)
            sym = c.keycode_to_keysym(ev.detail, 1 if ev.state & X.ShiftMask else 0)
            ch = chr(sym) if 0x20 <= sym < 0x100 else chr(sym - 0x1000000) if 0x1000100 <= sym < 0x1110000 else ''
            log(k='kdown' if ev.type == X.KeyPress else 'kup', t=ev.time, code=ev.detail, sym=c.keycode_to_keysym(ev.detail, 0), ch=ch, s=ev.state)
    time.sleep(0.01)
