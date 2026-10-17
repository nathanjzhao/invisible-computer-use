"""Raw X11 receiver for the round-2 review probes (the reviewer's xrecv.py).
usage: x11_edge_receiver.py WORK TAG   (TAG names the ready/event files so two processes can run)
Windows: target T (800x600 at 40,100) and sibling S (400x300 at 900,100), same process.
Commands in WORK/TAG.cmd: recreate (destroy T and create a new 800x600 window, same process) |
destroy_sibling | quit. Every button event on any window is logged with the window id.
"""
import json, os, sys, time
from pathlib import Path
from Xlib import X, Xatom, Xutil, display
work, tag = Path(sys.argv[1]), sys.argv[2]
c = display.Display(); root = c.screen().root
pid_atom = c.intern_atom('_NET_WM_PID')
def window(title, x, y, w, h, bg):
    item = root.create_window(x, y, w, h, 0, c.screen().root_depth, X.InputOutput, X.CopyFromParent,
        background_pixel=bg, event_mask=X.ExposureMask | X.ButtonPressMask | X.ButtonReleaseMask)
    item.set_wm_name(title); item.set_wm_class('icu-edge', 'ICUEdge')
    item.set_wm_hints(flags=Xutil.InputHint | Xutil.StateHint, input=1, initial_state=Xutil.NormalState)
    item.change_property(pid_atom, Xatom.CARDINAL, 32, [os.getpid()])
    item.map(); return item
def log(**e):
    e.update(t=time.time())
    with (work/f'{tag}-events.jsonl').open('a') as f: f.write(json.dumps(e) + '\n')
wins = {'T': window(f'{tag} target', 40, 100, 800, 600, 0xffffff), 'S': window(f'{tag} sibling', 900, 100, 400, 300, 0xdddddd)}
c.sync()
def ready():
    (work/f'{tag}-ready.json').write_text(json.dumps({'pid': os.getpid(), **{k: v.id for k, v in wins.items()}}))
ready()
cmd = work/f'{tag}.cmd'
while True:
    while c.pending_events():
        ev = c.next_event()
        if ev.type in (X.ButtonPress, X.ButtonRelease):
            log(kind='down' if ev.type == X.ButtonPress else 'up', window=ev.window.id, button=ev.detail,
                x=ev.event_x, y=ev.event_y, synthetic=bool(ev.send_event))
    if cmd.exists():
        op = cmd.read_text().strip(); cmd.unlink()
        if op == 'recreate':
            old = wins['T'].id; wins['T'].destroy(); c.sync(); time.sleep(.3)
            # Hand the freed XID back to the allocator, as a client that recycles ids would.
            c.display.free_resource_id(old)
            c.display.last_resource_id = old & c.display.info.resource_id_mask
            wins['T'] = window(f'{tag} target (recreated)', 40, 100, 800, 600, 0x00ff00); c.sync()
            log(kind='applied', op=op, old=old, new=wins['T'].id); ready()
        elif op == 'destroy_sibling':
            old = wins['S'].id; wins['S'].destroy(); c.sync(); log(kind='applied', op=op, old=old)
        elif op == 'quit':
            break
    time.sleep(.02)
