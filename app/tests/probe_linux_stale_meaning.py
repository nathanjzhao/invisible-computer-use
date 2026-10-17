"""Element tokens must refuse when the control changed meaning, on GTK3 and Qt5.

Usage: probe_linux_stale_meaning.py OUTDIR FIXTURE_DIR

Starts Xvfb, openbox and the AT-SPI bus, the two receivers
(fixtures/stale_meaning_gtk.py and fixtures/stale_meaning_qt.py) and
``$CUA_DRIVER serve``, then drives the driver through ``cua-driver call``.
Each case takes a fresh snapshot, reads the token for one label, changes the
UI, and acts with that token. The receiver's own click log is the oracle.

Expected (exit 0 only when all hold):
  fresh_click            token for Charlie clicks the widget labelled Charlie
  relabel                token for Bravo, Bravo relabelled Xray: refused
                         stale_element_target, no click
  recycled_row           token for "Item 03", rows rebound by a scroll so the
                         same widget shows "Item 08": refused, no click
  recycled_row_fresh     a new snapshot's token for "Item 08" clicks it
  other_widget_changed   token for Alpha, Charlie relabelled: Alpha clicked
  entry_typed_twice      an unnamed entry is labelled by its text, so this driver's own
                         write stales the token (round-3 O1): the first type_text
                         lands, the second through the same token refuses
                         stale_element_target, and a re-observed token types the rest
"""
import json
import os
import subprocess
import sys
import time

OUT, FIXTURES = sys.argv[1], sys.argv[2]
os.makedirs(OUT, exist_ok=True)
DRV = os.environ.get('CUA_DRIVER', '/usr/bin/cua-driver')
report = {'cases': {}, 'failures': []}


def sh(command):
    subprocess.run(command, shell=True, check=False)


def call(tool, args):
    result = subprocess.run([DRV, 'call', tool, json.dumps(args)], capture_output=True, text=True, timeout=60)
    try:
        return json.loads(result.stdout)
    except Exception:  # noqa: BLE001
        return {'_raw': (result.stdout + result.stderr)[:2000]}


os.environ['DISPLAY'] = ':99'
sh('Xvfb :99 -screen 0 1280x800x24 >/tmp/xvfb.log 2>&1 &')
time.sleep(2)
for line in subprocess.run('dbus-launch --sh-syntax', shell=True, capture_output=True, text=True).stdout.splitlines():
    if line.startswith('DBUS_SESSION_BUS_ADDRESS='):
        os.environ['DBUS_SESSION_BUS_ADDRESS'] = line.split('=', 1)[1].rstrip(';').strip("'")
sh('openbox >/tmp/openbox.log 2>&1 &')
time.sleep(1)
sh('/usr/libexec/at-spi-bus-launcher --launch-immediately >/tmp/atspi.log 2>&1 &')
time.sleep(1)


class Receiver:
    def __init__(self, tag, script, env=''):
        self.log, self.cmd = f'{OUT}/{tag}-events.jsonl', f'/tmp/{tag}.cmd'
        for path in (self.log, self.cmd):
            if os.path.exists(path):
                os.remove(path)
        sh(f'{env} python3 {FIXTURES}/{script} {self.log} {self.cmd} >{OUT}/{tag}.out 2>&1 &')
        for _ in range(150):
            if any(e.get('hit') == 'ready' for e in self.events()):
                break
            time.sleep(0.1)
        else:
            raise SystemExit(f'{tag} not ready')
        self.pid = next(e for e in self.events() if e.get('hit') == 'ready')['pid']

    def events(self):
        return [json.loads(line) for line in open(self.log)] if os.path.exists(self.log) else []

    def send(self, command):
        done = len([e for e in self.events() if e.get('hit') in ('applied', 'cmd_error')])
        with open(self.cmd, 'w') as f:
            f.write(command + '\n')
        for _ in range(100):
            if len([e for e in self.events() if e.get('hit') in ('applied', 'cmd_error')]) > done:
                break
            time.sleep(0.05)
        time.sleep(0.8)


receivers = {
    'gtk3': Receiver('gtk3', 'stale_meaning_gtk.py', 'GTK_MODULES=gail:atk-bridge NO_AT_BRIDGE=0'),
    'qt5': Receiver('qt5', 'stale_meaning_qt.py', 'QT_ACCESSIBILITY=1 QT_LINUX_ACCESSIBILITY_ALWAYS_ON=1'),
}
sh(f'{DRV} serve --no-permissions-gate >{OUT}/driver.log 2>&1 &')
time.sleep(4)
report['driver_sha256'] = subprocess.run(['sha256sum', DRV], capture_output=True, text=True).stdout.split()[0]


def window_of(receiver):
    for _ in range(30):
        windows = [w for w in call('list_windows', {}).get('windows', []) if w.get('pid') == receiver.pid]
        if windows:
            return windows[0]
        time.sleep(0.2)
    raise SystemExit(f'no window for pid {receiver.pid}')


def snapshot(receiver, window):
    return call('get_window_state', {'pid': receiver.pid, 'window_id': window['window_id'], 'include_screenshot': False})


def token(state, label=None, role=None):
    found = [e for e in state.get('elements', []) if e.get('element_token')
             and (label is None or e.get('label') == label) and (role is None or role in (e.get('role') or ''))]
    return found[0]['element_token'] if len(found) == 1 else None


def refusal_code(result):
    return (result.get('refusal') or {}).get('code') or result.get('code')


def run(tk, name, label, change, expect):
    receiver = receivers[tk]
    window = window_of(receiver)
    state = snapshot(receiver, window)
    tok = token(state, label=label)
    entry = {'label': label, 'change': change, 'token': tok}
    if tok is None:
        entry['error'] = f'no unique token for {label!r}'
    else:
        if change:
            receiver.send(change)
        before = len(receiver.events())
        result = call('click', {'pid': receiver.pid, 'window_id': window['window_id'], 'element_token': tok})
        time.sleep(0.8)
        clicks = [f"{e['serial']}:{e['label']}" for e in receiver.events()[before:] if e.get('hit') == 'clicked']
        entry.update(result=result, clicks=clicks, refusal=refusal_code(result))
    if expect == 'refuse':
        entry['ok'] = entry.get('refusal') == 'stale_element_target' and entry.get('clicks') == []
    else:
        entry['ok'] = entry.get('refusal') is None and len(entry.get('clicks') or []) == 1 \
            and entry['clicks'][0].split(':', 1)[1] == expect
    report['cases'][f'{tk}_{name}'] = entry
    print(f'{tk}_{name}', 'ok' if entry['ok'] else 'FAIL', entry.get('refusal'), entry.get('clicks'), entry.get('error', ''))


def typed_twice(tk):
    receiver = receivers[tk]
    window = window_of(receiver)
    receiver.send('settext ')
    state = snapshot(receiver, window)
    tok = token(state, role='text')
    entry = {'token': tok, 'results': []}
    if tok is None:
        entry['error'] = 'no unique editable token'
    else:
        for text in ('ab', 'cd'):
            entry['results'].append(call('type_text', {'pid': receiver.pid, 'window_id': window['window_id'],
                                                       'element_token': tok, 'text': text}))
            time.sleep(0.8)
        texts = [e['text'] for e in receiver.events() if e.get('hit') == 'entry_changed']
        entry['text_after_two_calls'] = texts[-1] if texts else None
        again = token(snapshot(receiver, window), label='ab', role='text')
        entry['reobserved_token'] = again
        if again:
            entry['results'].append(call('type_text', {'pid': receiver.pid, 'window_id': window['window_id'],
                                                       'element_token': again, 'text': 'cd'}))
            time.sleep(0.8)
        texts = [e['text'] for e in receiver.events() if e.get('hit') == 'entry_changed']
        entry['final_text'] = texts[-1] if texts else None
    entry['refusals'] = [refusal_code(r) for r in entry['results']]
    entry['ok'] = (entry.get('text_after_two_calls') == 'ab' and entry.get('final_text') == 'abcd'
                   and entry['refusals'] == [None, 'stale_element_target', None])
    report['cases'][f'{tk}_entry_typed_twice'] = entry
    print(f'{tk}_entry_typed_twice', 'ok' if entry['ok'] else 'FAIL', entry.get('final_text'), entry['refusals'], entry.get('error', ''))


for tk in ('gtk3', 'qt5'):
    run(tk, 'fresh_click', 'Charlie', None, 'Charlie')
    run(tk, 'relabel', 'Bravo', 'relabel Bravo Xray', 'refuse')
    run(tk, 'recycled_row', 'Item 03', 'scroll 5', 'refuse')
    run(tk, 'recycled_row_fresh', 'Item 08', None, 'Item 08')
    run(tk, 'other_widget_changed', 'Alpha', 'relabel Charlie Zulu', 'Alpha')
    typed_twice(tk)

report['failures'] = [name for name, case in report['cases'].items() if not case.get('ok')]
report['ok'] = not report['failures']
with open(f'{OUT}/summary.json', 'w') as f:
    json.dump(report, f, indent=1)
print('STALE_MEANING', 'PASS' if report['ok'] else 'FAIL', report['failures'])
sys.exit(0 if report['ok'] else 1)
