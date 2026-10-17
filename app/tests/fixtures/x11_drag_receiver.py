"""Owned raw X11 receiver for an interruptible background drag."""
import json
import os
import sys
import time
from pathlib import Path
from Xlib import X, Xatom, Xutil, display

work = Path(sys.argv[1])
connection = display.Display()
root = connection.screen().root
pid_atom = connection.intern_atom('_NET_WM_PID')
active_atom = connection.intern_atom('_NET_ACTIVE_WINDOW')

def window(title, x, y, width=360, height=240):
    item = root.create_window(x, y, width, height, 0, connection.screen().root_depth,
        X.InputOutput, X.CopyFromParent, background_pixel=0xffffff,
        event_mask=X.ExposureMask | X.ButtonPressMask | X.ButtonReleaseMask | X.PointerMotionMask)
    item.set_wm_name(title)
    item.set_wm_class('icu-drag-fixture', 'ICUDragFixture')
    item.set_wm_hints(flags=Xutil.InputHint | Xutil.StateHint,input=1,initial_state=Xutil.NormalState)
    item.change_property(pid_atom, Xatom.CARDINAL, 32, [os.getpid()])
    item.map()
    return item

target = (window('ICU Drag Receiver', 40, 100, 1280, 800)
    if len(sys.argv) > 2 and sys.argv[2] == 'resize' else window('ICU Drag Receiver', 1000, 100))
guard = window('ICU Focus Guard', 1050, 650)
connection.sync()
(work/'fixture-ready.json').write_text(json.dumps({'pid':os.getpid(),'target':target.id,'guard':guard.id}))
pressed = False
while True:
    event = connection.next_event()
    if getattr(event, 'window', None) != target or event.type not in (X.ButtonPress, X.MotionNotify, X.ButtonRelease):
        continue
    kind = {X.ButtonPress:'down', X.MotionNotify:'motion', X.ButtonRelease:'up'}[event.type]
    if kind == 'down': pressed = True
    if kind == 'up': pressed = False
    prop = root.get_full_property(active_atom, X.AnyPropertyType)
    active = int(prop.value[0]) if prop is not None and len(prop.value) else 0
    row = {'kind':kind,'x':event.event_x,'y':event.event_y,'pressed':pressed,
        'active':active,'synthetic':bool(event.send_event),'time':time.monotonic()}
    with (work/'receiver-events.jsonl').open('a') as output:
        output.write(json.dumps(row)+'\n')
