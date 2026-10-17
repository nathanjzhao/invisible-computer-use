"""Two real editable windows in one process for native scope acceptance."""
import gi
import json
import os
import sys
import time
from pathlib import Path

gi.require_version('Gtk', '3.0')
from gi.repository import Gtk, GLib

work = Path(sys.argv[1])
GLib.set_prgname('icu-scope-fixture')
GLib.set_application_name('ICU Scope Fixture')
fields = {}

def save(changed=None):
    state = {
        label: {'text': item['buffer'].get_text(item['buffer'].get_start_iter(), item['buffer'].get_end_iter(), True),
                'active': item['window'].is_active(),
                'selected_by_window': item['window'].get_focus() == item['view']}
        for label, item in fields.items()
    }
    temp = work / 'receiver-state.tmp'
    temp.write_text(json.dumps(state)); temp.replace(work / 'receiver-state.json')
    if changed is not None:
        with (work / 'receiver-events.jsonl').open('a') as output:
            output.write(json.dumps({'changed':changed, 'state':state, 'time':time.monotonic()}) + '\n')

for label, y in [('Alpha', 90), ('Beta', 500)]:
    window = Gtk.Window(title='ICU Scope ' + label)
    window.set_default_size(260, 280); window.move(1320, y)
    box = Gtk.Box(orientation=Gtk.Orientation.VERTICAL, spacing=10)
    box.set_border_width(14)
    box.pack_start(Gtk.Label(label='Document ' + label), False, False, 0)
    view = Gtk.TextView(); view.get_accessible().set_name('Field ' + label)
    buffer = view.get_buffer(); buffer.set_text(label.lower())
    buffer.place_cursor(buffer.get_end_iter())
    box.pack_start(view, True, True, 0); window.add(box)
    fields[label] = {'window':window, 'view':view, 'buffer':buffer}
    buffer.connect('changed', lambda _buffer, name=label: save(name))
    window.show_all(); view.grab_focus()
if os.environ.get('ICU_SCOPE_EMPTY') == '1':
    empty = Gtk.Window(title='ICU Scope Empty')
    empty.set_default_size(300, 180); empty.move(500, 0)
    empty.add(Gtk.Label(label='No editable field in this window'))
    empty.show_all()
    # Controlled fixture setup, before the app owns foreground for the probe.
    fields['Beta']['window'].present(); fields['Beta']['view'].grab_focus()
save()
(work / 'fixture-ready.json').write_text(json.dumps({'pid':os.getpid(), 'titles':['ICU Scope Alpha','ICU Scope Beta']}))
Gtk.main()
