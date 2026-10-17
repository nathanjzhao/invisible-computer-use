"""GTK3 receiver for meaning-bound stale targets.

usage: stale_meaning_gtk.py LOG CMD

Buttons Alpha, Bravo, Charlie; a pool of five row buttons that shows
"Item NN" for a window into a 100-item list (the pool is rebound on scroll,
the way virtual lists recycle row widgets); one entry. Every click is logged
with the widget's creation serial and its label at the time of the click.
Commands (one per line in CMD): relabel OLD NEW | scroll N | settext TEXT
"""
import json
import os
import sys
import time

import gi

gi.require_version('Gtk', '3.0')
from gi.repository import GLib, Gtk  # noqa: E402

LOG, CMD = sys.argv[1], sys.argv[2]
serial = [0]


def log(**event):
    event.update(t=time.time(), pid=os.getpid())
    with open(LOG, 'a') as f:
        f.write(json.dumps(event) + '\n')


win = Gtk.Window(title='StaleMeaning GTK3')
win.set_default_size(320, 420)
box = Gtk.Box(orientation=Gtk.Orientation.VERTICAL, spacing=4)
win.add(box)
buttons = []


def make(label):
    serial[0] += 1
    button = Gtk.Button(label=label)
    button.serial = serial[0]
    button.connect('clicked', lambda b: log(hit='clicked', serial=b.serial, label=b.get_label()))
    buttons.append(button)
    box.pack_start(button, False, False, 0)
    return button


for name in ('Alpha', 'Bravo', 'Charlie'):
    make(name)
first_item = [0]
rows = [make(f'Item {i:02d}') for i in range(5)]
entry = Gtk.Entry()
entry.connect('changed', lambda e: log(hit='entry_changed', text=e.get_text()))
box.pack_start(entry, False, False, 0)


def by_label(label):
    return next(b for b in buttons if b.get_label() == label)


def apply(command):
    op, *args = command.split(' ', 1)
    if op == 'relabel':
        old, new = args[0].split()
        by_label(old).set_label(new)
    elif op == 'scroll':
        first_item[0] += int(args[0])
        for offset, row in enumerate(rows):
            row.set_label(f'Item {first_item[0] + offset:02d}')
    elif op == 'settext':
        entry.set_text(args[0] if args else '')
    else:
        raise ValueError(op)
    log(hit='applied', cmd=command, buttons=[(b.get_label(), b.serial) for b in buttons])


def poll():
    if os.path.exists(CMD):
        lines = [line.strip() for line in open(CMD) if line.strip()]
        os.remove(CMD)
        for line in lines:
            try:
                apply(line)
            except Exception as error:  # noqa: BLE001
                log(hit='cmd_error', cmd=line, error=repr(error))
    return True


GLib.timeout_add(50, poll)
win.connect('destroy', Gtk.main_quit)
win.show_all()
GLib.timeout_add(800, lambda: log(hit='ready') and False)
Gtk.main()
