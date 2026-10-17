"""Owned multiline receiver for isolated native manual-text acceptance."""
import gi
import json
import os
import sys
import time
from pathlib import Path

gi.require_version('Gtk', '3.0')
from gi.repository import Gtk, GLib

work = Path(sys.argv[1])
GLib.set_prgname('icu-manual-fixture')
GLib.set_application_name('ICU Manual Fixture')
window = Gtk.Window(title='ICU Manual Receiver')
window.set_default_size(470, 360)
window.move(1050, 120)
box = Gtk.Box(orientation=Gtk.Orientation.VERTICAL, spacing=12)
box.set_border_width(18)
box.pack_start(Gtk.Label(label='Manual text receiver'), False, False, 0)
draft = Gtk.TextView()
draft.get_accessible().set_name('Draft')
draft.set_wrap_mode(Gtk.WrapMode.WORD_CHAR)
buffer = draft.get_buffer()
buffer.set_text('seed')
buffer.place_cursor(buffer.get_end_iter())
box.pack_start(draft, True, True, 0)
private = Gtk.Entry()
private.set_visibility(False)
private.set_text('fixture-private-value')
private.get_accessible().set_name('Private control')
box.pack_start(private, False, False, 0)
window.add(box)

def save(*_args):
    text = buffer.get_text(buffer.get_start_iter(), buffer.get_end_iter(), True)
    record = {'text': text, 'active': window.is_active(), 'time': time.monotonic(),
              'private_unchanged': private.get_text() == 'fixture-private-value',
              'draft_selected_by_window': window.get_focus() == draft,
              'caret': buffer.get_iter_at_mark(buffer.get_insert()).get_offset()}
    temp = work / 'receiver-state.tmp'
    temp.write_text(json.dumps(record))
    temp.replace(work / 'receiver-state.json')
    with (work / 'receiver-events.jsonl').open('a') as out:
        out.write(json.dumps(record) + '\n')

buffer.connect('changed', save)
private.connect('changed', save)
window.connect('destroy', Gtk.main_quit)
window.show_all()
# Fixture setup selects the receiver field before the app owns the desktop.
# The acceptance test concerns committed text, not pixel-to-field mapping.
draft.grab_focus()
save()
(work / 'fixture-ready.json').write_text(json.dumps({'pid': os.getpid()}))
Gtk.main()
