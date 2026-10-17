"""Owned GTK receiver for the isolated package acceptance test."""
import gi
import json
import os
import sys
import time
from pathlib import Path

gi.require_version('Gtk', '3.0')
from gi.repository import Gtk, GLib

work = Path(sys.argv[1])
GLib.set_prgname('icu-calculator-fixture')
GLib.set_application_name('ICU Calculator Fixture')
window = Gtk.Window(title='ICU Calculator')
window.set_default_size(380, 320)
window.move(1000, 100)
grid = Gtk.Grid(column_spacing=12, row_spacing=12)
grid.set_border_width(20)
result = Gtk.Label(label='0')
result.get_accessible().set_name('0')
grid.attach(result, 0, 0, 3, 1)
state = {'display': '0', 'buttons': []}
left = None
current = '0'
fresh = True

def save():
    temp = work / 'fixture-state.tmp'
    temp.write_text(json.dumps(state))
    temp.replace(work / 'fixture-state.json')

def press(_button, label):
    global left, current, fresh
    state['buttons'].append(label)
    if label.isdigit():
        current = label if fresh or current == '0' else current + label
        fresh = False
        value = current
    elif label == '×':
        left = int(current)
        fresh = True
        value = current + ' ×'
    elif label == '=' and left is not None:
        current = str(left * int(current))
        left = None
        fresh = True
        value = current
    else:
        left = None
        current = '0'
        fresh = True
        value = current
    state['display'] = value
    result.set_text(value)
    result.get_accessible().set_name(value)
    with (work / 'receiver-events.jsonl').open('a') as f:
        f.write(json.dumps({'button': label, 'display': value, 'active': window.is_active(), 'time': time.monotonic()}) + '\n')
    save()

for index, label in enumerate(['6', '×', '7', '=', 'C']):
    button = Gtk.Button(label=label)
    button.set_size_request(90, 58)
    button.connect('clicked', press, label)
    grid.attach(button, index % 3, 1 + index // 3, 1, 1)
if os.environ.get('ICU_FIXTURE_KEYBOARD') == '1':
    Gtk.Settings.get_default().set_property('gtk-entry-select-on-focus', False)
    draft = Gtk.Entry()
    draft.set_text('seed')
    draft.set_position(-1)
    draft.get_accessible().set_name('Draft')
    grid.attach(draft, 0, 3, 3, 1)

    def save_draft(*_args):
        record = {'text': draft.get_text(), 'selection': list(draft.get_selection_bounds()),
                  'active': window.is_active(),
                  'select_on_focus': Gtk.Settings.get_default().get_property('gtk-entry-select-on-focus')}
        temp = work / 'draft-state.tmp'
        temp.write_text(json.dumps(record))
        temp.replace(work / 'draft-state.json')
        with (work / 'draft-events.jsonl').open('a') as output:
            output.write(json.dumps(dict(record, time=time.monotonic())) + '\n')

    draft.connect('changed', save_draft)
    draft.connect('notify::cursor-position', save_draft)
    draft.connect('notify::selection-bound', save_draft)
    save_draft()
    if os.environ.get('ICU_FIXTURE_PRIVATE_CONTROL') == '1':
        private = Gtk.Entry()
        private.set_visibility(False)
        private.set_text('fixture-private-value')
        private.get_accessible().set_name('Private control')
        grid.attach(private, 0, 4, 3, 1)
window.add(grid)
window.connect('destroy', Gtk.main_quit)
window.show_all()
guard = Gtk.Window(title='ICU Focus Guard')
guard.set_default_size(320, 180)
guard.move(1050, 650)
guard.add(Gtk.Label(label='Keep focus here during the calculator task.'))
guard.show_all()
save()
(work / 'fixture-ready.json').write_text(json.dumps({'pid': os.getpid()}))
Gtk.main()
