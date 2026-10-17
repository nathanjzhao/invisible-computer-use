"""GTK3 fixture for the round-3 O1 attack: two unnamed entries that show a
sorted list of records, like an editable list that keeps itself sorted and
recycles its row widgets. An edit renames that row's record and, on the next
idle, the list re-sorts and rebinds each entry to the record now at its
position. Enter in an entry commits and loads the next record into the same
entry (a form that advances). A third entry, with the accessible name "Notes",
is not part of the list: its label does not change when its text does.

usage: resort_rows_gtk.py LOG
Every entry change is logged with the record the entry is bound to.
"""
import json, os, sys, time
import gi
gi.require_version('Gtk', '3.0')
from gi.repository import GLib, Gtk
LOG = sys.argv[1]
def log(**e):
    e.update(t=time.time(), pid=os.getpid())
    with open(LOG, 'a') as f: f.write(json.dumps(e) + '\n')
records = {1: 'carol', 2: 'carol2', 3: 'mallory'}   # record id -> text
queue = [3]                                        # records Enter advances to
win = Gtk.Window(title='Sorted rows'); win.set_default_size(360, 200)
box = Gtk.Box(orientation=Gtk.Orientation.VERTICAL, spacing=4); win.add(box)
entries = []; state = {'binding': False, 'pending': False}
def bind(e, rid):
    state['binding'] = True; e.rid = rid; e.set_text(records[rid]); state['binding'] = False
def rows():
    return [(e.rid, records[e.rid], e.get_text()) for e in entries]
def resort():
    state['pending'] = False
    order = sorted((e.rid for e in entries), key=lambda rid: records[rid])
    for e, rid in zip(entries, order):
        if e.rid != rid: bind(e, rid)
    log(hit='resorted', rows=rows()); return False
def changed(e):
    if state['binding']: return
    records[e.rid] = e.get_text()
    log(hit='edit', record=e.rid, text=e.get_text(), rows=rows())
    if not state['pending']:
        state['pending'] = True; GLib.idle_add(resort)
def activate(e):
    if not queue: return
    old = e.rid; bind(e, queue.pop(0)); queue.append(old)
    log(hit='advanced', rows=rows())
for rid in (1, 2):
    e = Gtk.Entry(); e.rid = rid; e.set_text(records[rid])
    e.connect('changed', changed); e.connect('activate', activate)
    entries.append(e); box.pack_start(e, False, False, 0)
notes = Gtk.Entry(); notes.get_accessible().set_name('Notes')
notes.connect('changed', lambda e: log(hit='notes', text=e.get_text()))
box.pack_start(notes, False, False, 0)
win.connect('destroy', Gtk.main_quit); win.show_all()
GLib.timeout_add(800, lambda: log(hit='ready', rows=rows()) and False); Gtk.main()
