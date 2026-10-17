"""Owned raw X11 receivers for a tile wheel gesture: a bound target and a decoy, both scrollable.

    x11_scroll_receiver.py WORK [overlap]

Two top-level windows, `ICU Scroll Target` and `ICU Scroll Decoy`. Each keeps its own scroll offset: a
wheel-up press (button 4) moves it by -1, a wheel-down press (button 5) by +1, and it draws the offset as
text so a screenshot shows it. Every wheel press is appended to WORK/receiver-events.jsonl with the window it
reached, the offset after it, the pointer position in that window and whether it was a SendEvent.
With `overlap` the decoy is mapped at the same screen place as the target and stacked in front of it
(the Mac proof's arrangement: a decoy where a screen-position wheel would land); without it the two sit apart.
WORK/fixture-ready.json holds the window ids once both are mapped.
"""
import json
import os
import sys
import time
from pathlib import Path
from Xlib import X, Xatom, Xutil, display

work = Path(sys.argv[1])
overlap = len(sys.argv) > 2 and sys.argv[2] == 'overlap'
connection = display.Display()
root = connection.screen().root
pid_atom = connection.intern_atom('_NET_WM_PID')
font = connection.open_font('fixed')
WIDTH, HEIGHT = 640, 420
offsets = {}


def window(title, x, y, background):
    item = root.create_window(x, y, WIDTH, HEIGHT, 0, connection.screen().root_depth,
        X.InputOutput, X.CopyFromParent, background_pixel=background,
        event_mask=X.ExposureMask | X.ButtonPressMask | X.ButtonReleaseMask | X.PointerMotionMask)
    item.set_wm_name(title)
    item.set_wm_class('icu-scroll-fixture', 'ICUScrollFixture')
    item.set_wm_hints(flags=Xutil.InputHint | Xutil.StateHint, input=1, initial_state=Xutil.NormalState)
    item.set_wm_normal_hints(flags=Xutil.PPosition | Xutil.PSize, min_width=WIDTH, min_height=HEIGHT, max_width=WIDTH, max_height=HEIGHT)
    item.change_property(pid_atom, Xatom.CARDINAL, 32, [os.getpid()])
    item.gc = item.create_gc(foreground=0x000000, background=background, font=font)
    offsets[item.id] = 0
    item.map()
    return item


def draw(item, name):
    item.clear_area(0, 0, WIDTH, HEIGHT)
    item.image_text(item.gc, 24, 60, f'{name}  scroll offset {offsets[item.id]}')
    for row in range(8):  # content that visibly moves with the offset
        y = 120 + row * 36 - (offsets[item.id] % 36)
        item.image_text(item.gc, 24, y, f'line {row + offsets[item.id] // 36:04d} of {name}')


target = window('ICU Scroll Target', 40, 100, 0xdff0ff)
decoy = window('ICU Scroll Decoy', 40 if overlap else 760, 100 if overlap else 100, 0xffe4d0)
names = {target.id: 'target', decoy.id: 'decoy'}
if overlap:
    decoy.configure(stack_mode=X.Above)
connection.sync()
(work / 'fixture-ready.json').write_text(json.dumps({'pid': os.getpid(), 'target': target.id, 'decoy': decoy.id, 'overlap': overlap}))
while True:
    event = connection.next_event()
    item = getattr(event, 'window', None)
    if item is None or getattr(item, 'id', None) not in names:
        continue
    if event.type == X.Expose:
        draw(target if item.id == target.id else decoy, names[item.id])
    elif event.type == X.ButtonPress and event.detail in (4, 5):
        offsets[item.id] += 1 if event.detail == 5 else -1
        win = target if item.id == target.id else decoy
        row = {'window': names[item.id], 'button': event.detail, 'offset': offsets[item.id],
               'x': event.event_x, 'y': event.event_y, 'synthetic': bool(event.send_event), 'time': time.time()}
        with (work / 'receiver-events.jsonl').open('a') as output:
            output.write(json.dumps(row) + '\n')
        draw(win, names[item.id])
        connection.flush()
