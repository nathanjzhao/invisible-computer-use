"""GTK3 receiver for round-2 meaning-token edges (the reviewer's meaning_gtk.py): four
buttons, a toggle, a check button and two unnamed entries whose rows can be rebound.
usage: text_label_rows_gtk.py LOG CMD
Commands (JSON per line): {"op":"relabel","old":..,"new":..} | {"op":"rebind","texts":[..]}"""
import json, os, sys, time
import gi
gi.require_version('Gtk', '3.0')
from gi.repository import GLib, Gtk
LOG, CMD = sys.argv[1], sys.argv[2]
def log(**e):
    e.update(t=time.time(), pid=os.getpid())
    with open(LOG, 'a') as f: f.write(json.dumps(e) + '\n')
win = Gtk.Window(title='Meaning edges'); win.set_default_size(360, 520)
box = Gtk.Box(orientation=Gtk.Orientation.VERTICAL, spacing=4); win.add(box)
buttons = []; serial = [0]
def make(label):
    serial[0] += 1; b = Gtk.Button(label=label); b.serial = serial[0]
    b.connect('clicked', lambda b: log(hit='clicked', serial=b.serial, label=b.get_label()))
    buttons.append(b); box.pack_start(b, False, False, 0)
for name in ('Bravo', 'Delta', 'Echo', 'Foxtrot'): make(name)
play = Gtk.ToggleButton(label='Play'); play.serial = 100
def on_play(b):
    log(hit='clicked', serial=b.serial, label=b.get_label(), active=b.get_active())
    b.set_label('Pause' if b.get_active() else 'Play')
play.connect('toggled', on_play); buttons.append(play); box.pack_start(play, False, False, 0)
mute = Gtk.CheckButton(label='Mute'); mute.serial = 101
mute.connect('toggled', lambda b: log(hit='clicked', serial=b.serial, label=b.get_label(), active=b.get_active()))
buttons.append(mute); box.pack_start(mute, False, False, 0)
entries = []
for i, text in enumerate(('alice', 'bob')):
    e = Gtk.Entry(); e.set_text(text); e.serial = 200 + i; e.record = text
    e.connect('changed', lambda e: log(hit='entry_changed', serial=e.serial, record=e.record, text=e.get_text()))
    entries.append(e); box.pack_start(e, False, False, 0)
def apply(cmd):
    if cmd['op'] == 'relabel':
        next(b for b in buttons if b.get_label() == cmd['old']).set_label(cmd['new'])
    elif cmd['op'] == 'rebind':
        for e, text in zip(entries, cmd['texts']):
            e.record = text; e.handler_block_by_func  # keep logging the rebinding as a change of record
            e.set_text(text)
    log(hit='applied', cmd=cmd, buttons=[(b.get_label(), b.serial) for b in buttons], entries=[(e.serial, e.record, e.get_text()) for e in entries])
def poll():
    if os.path.exists(CMD):
        lines = [l for l in open(CMD) if l.strip()]; os.remove(CMD)
        for l in lines:
            try: apply(json.loads(l))
            except Exception as err: log(hit='cmd_error', cmd=l, error=repr(err))
    return True
GLib.timeout_add(50, poll); win.connect('destroy', Gtk.main_quit); win.show_all()
GLib.timeout_add(800, lambda: log(hit='ready') and False); Gtk.main()
