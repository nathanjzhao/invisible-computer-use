"""Round-2 review item O1: an element whose label is its text (an entry with
no accessible name) is token-bound to the text the model saw. A recycled row
that now shows another record refuses. Since round 3 (O1) this driver's own
write stales the token too, because the label it was issued for changed: the
second write refuses and a re-observed token writes.

    probe_linux_text_label_meaning.py WORK CUA_DRIVER FIXTURE

FIXTURE is fixtures/text_label_rows_gtk.py (GTK3 on Xvfb + openbox): two
unnamed entries showing "alice" and "bob", and a command file that rebinds
them to other records. The driver runs as its own daemon; every call is
background and passes an element_token. Exits 0 only when every case matches
EXPECTED.
"""
import hashlib, json, os, pathlib, select, signal, socket, subprocess, sys, time, traceback

if sys.platform != 'linux': raise SystemExit('Linux-only probe')
work = pathlib.Path(sys.argv[1]); work.mkdir(parents=True)
driver = pathlib.Path(sys.argv[2]).resolve(); fixture = pathlib.Path(sys.argv[3]).resolve()
procs = []; logs = []
report = {'driver_sha256': hashlib.sha256(driver.read_bytes()).hexdigest(),
          'probe_sha256': hashlib.sha256(pathlib.Path(__file__).read_bytes()).hexdigest(),
          'fixture_sha256': hashlib.sha256(fixture.read_bytes()).hexdigest(), 'cases': {}}
# case -> 'refused' (stale_element_target, fixture unchanged) or the text the
# first entry must hold afterwards.
EXPECTED = {
    'named_button_control': 'clicked Bravo',
    'recycled_entry_type_text': 'refused',
    'recycled_entry_set_value': 'refused',
    'own_edit_type_text_first': 'Xcarol',
    'own_edit_type_text_second': 'refused',
    # GTK puts the caret after the first write (XYcarol); Qt leaves it at the end (XcarolY).
    'own_edit_reobserved_type_text': ('XYcarol', 'XcarolY'),
    'own_edit_reobserved_set_value': 'zed',
    'recycled_after_own_edits_type_text': 'refused',
    'empty_entry_typed_first': 'ab',
    'empty_entry_typed_second': 'refused',
    'empty_entry_reobserved_typed': 'abcd',
}
env = dict(os.environ); env.update({'XDG_RUNTIME_DIR': str(work/'runtime'), 'XDG_SESSION_TYPE': 'x11', 'GTK_MODULES': 'gail:atk-bridge', 'NO_AT_BRIDGE': '0'})
env.pop('WAYLAND_DISPLAY', None); (work/'runtime').mkdir(); (work/'runtime').chmod(0o700)
LOG, CMD = work/'events.jsonl', work/'fixture.cmd'
def launch(args, name, stdout=None):
    log = (work/(name+'.log')).open('wb'); logs.append(log)
    p = subprocess.Popen(args, env=env, stdin=subprocess.DEVNULL, stdout=stdout or log, stderr=log, start_new_session=True); procs.append(p); return p
def wait_for(pred, s, msg):
    d = time.monotonic() + s
    while time.monotonic() < d:
        r = pred()
        if r: return r
        time.sleep(.05)
    raise RuntimeError(msg)
def protocol(path, req):
    with socket.socket(socket.AF_UNIX) as s:
        s.settimeout(30); s.connect(str(path)); s.sendall((json.dumps(req)+'\n').encode()); data = b''
        while b'\n' not in data:
            ch = s.recv(1 << 20)
            if not ch: raise ConnectionError('closed')
            data += ch
    r = json.loads(data.split(b'\n')[0]); assert r.get('ok'), r; return r['result']
def events(): return [json.loads(l) for l in LOG.read_text().splitlines()] if LOG.exists() else []
def send(cmd):
    n = len([e for e in events() if e.get('hit') in ('applied', 'cmd_error')]); CMD.write_text(json.dumps(cmd) + '\n')
    wait_for(lambda: len([e for e in events() if e.get('hit') in ('applied', 'cmd_error')]) > n, 5, 'cmd'); time.sleep(.8)
def first_entry_text():
    applied = [e for e in events() if e.get('hit') == 'applied']
    changed = [e for e in events() if e.get('hit') == 'entry_changed' and e.get('serial') == 200]
    last_applied = applied[-1]['t'] if applied else 0
    text = next((t for s, r, t in applied[-1]['entries'] if s == 200), None) if applied else 'alice'
    later = [e for e in changed if e['t'] > last_applied]
    return later[-1]['text'] if later else text
signal.signal(signal.SIGALRM, lambda *_: (_ for _ in ()).throw(TimeoutError('deadline'))); signal.alarm(300)
try:
    x = launch(['Xvfb', '-displayfd', '1', '-screen', '0', '1280x800x24', '-nolisten', 'tcp'], 'xvfb', subprocess.PIPE)
    assert select.select([x.stdout], [], [], 8)[0]; env['DISPLAY'] = ':' + x.stdout.readline().decode().strip()
    launch(['openbox'], 'openbox'); time.sleep(.5)
    launch(['/usr/libexec/at-spi-bus-launcher', '--launch-immediately'], 'atspi'); time.sleep(1)
    launch(['/usr/bin/python3', str(fixture), str(LOG), str(CMD)], 'fixture')
    wait_for(lambda: any(e.get('hit') == 'ready' for e in events()), 15, 'fixture'); pid = events()[0]['pid']
    sock = work/'driver.sock'; launch([str(driver), 'serve', '--socket', str(sock)], 'driver')
    wait_for(sock.exists, 20, 'driver socket'); time.sleep(.3)
    def call(name, args): return protocol(sock, {'method': 'call', 'name': name, 'args': args, 'session_id': 'textlabel', 'client_kind': 'cli'})
    call('set_agent_cursor_enabled', {'session': 'textlabel', 'enabled': False})
    wid = wait_for(lambda: [w['window_id'] for w in call('list_windows', {})['structuredContent']['windows'] if w.get('pid') == pid], 10, 'window')[0]
    def token(label, role=None):
        state = call('get_window_state', {'pid': pid, 'window_id': wid, 'include_screenshot': False}).get('structuredContent') or {}
        found = [e for e in state.get('elements', []) if e.get('label') == label and (role is None or role in (e.get('role') or ''))]
        if not found and label == '':
            found = [e for e in state.get('elements', []) if role in (e.get('role') or '') and not e.get('label')]
        report.setdefault('tokens', {})[label] = found[:1]
        assert found, f'no element labelled {label!r}'
        return found[0]['element_token']
    def act(case, tool, args):
        b = len(events()); r = call(tool, {'pid': pid, 'window_id': wid, 'delivery_mode': 'background', **args}); time.sleep(.9)
        seen = [e for e in events()[b:] if e.get('hit') in ('clicked', 'entry_changed')]
        sc = r.get('structuredContent') or {}
        code = (sc.get('refusal') or {}).get('code') or sc.get('code')
        want = EXPECTED[case]
        if want == 'refused':
            ok = bool(r.get('isError')) and code == 'stale_element_target' and not seen
        elif not isinstance(want, str):
            ok = not r.get('isError') and first_entry_text() in want
        elif want.startswith('clicked '):
            ok = not r.get('isError') and [e.get('label') for e in seen if e.get('hit') == 'clicked'] == [want[8:]]
        else:
            ok = not r.get('isError') and first_entry_text() == want
        report['cases'][case] = {'tool': tool, 'args': args, 'is_error': bool(r.get('isError')), 'code': code,
            'text': ''.join(c.get('text', '') for c in r.get('content', []))[:300], 'effects': seen,
            'first_entry_text': first_entry_text(), 'expected': want, 'ok': ok}
        print('PASS' if ok else 'FAIL', case, code, first_entry_text(), flush=True)
    act('named_button_control', 'click', {'element_token': token('Bravo')})
    alice = token('alice', 'text')
    send({'op': 'rebind', 'texts': ['carol', 'dave']})
    act('recycled_entry_type_text', 'type_text', {'element_token': alice, 'text': 'X'})
    act('recycled_entry_set_value', 'set_value', {'element_token': alice, 'value': 'overwritten'})
    send({'op': 'rebind', 'texts': ['carol', 'dave']})  # undo whatever a refusal failed to prevent
    carol = token('carol', 'text')
    act('own_edit_type_text_first', 'type_text', {'element_token': carol, 'text': 'X'})
    act('own_edit_type_text_second', 'type_text', {'element_token': carol, 'text': 'Y'})
    xcarol = token('Xcarol', 'text')
    act('own_edit_reobserved_type_text', 'type_text', {'element_token': xcarol, 'text': 'Y'})
    act('own_edit_reobserved_set_value', 'set_value', {'element_token': token(first_entry_text(), 'text'), 'value': 'zed'})
    zed = token('zed', 'text')
    send({'op': 'rebind', 'texts': ['erin', 'frank']})
    act('recycled_after_own_edits_type_text', 'type_text', {'element_token': zed, 'text': 'Z'})
    send({'op': 'rebind', 'texts': ['', 'bob']})
    empty = token('', 'text')
    act('empty_entry_typed_first', 'type_text', {'element_token': empty, 'text': 'ab'})
    act('empty_entry_typed_second', 'type_text', {'element_token': empty, 'text': 'cd'})
    act('empty_entry_reobserved_typed', 'type_text', {'element_token': token('ab', 'text'), 'text': 'cd'})
    report['done'] = True
except Exception as e:
    report.update(error=str(e), traceback=traceback.format_exc())
finally:
    signal.alarm(0)
    for p in reversed(procs):
        if p.poll() is None:
            try: os.killpg(p.pid, signal.SIGTERM)
            except ProcessLookupError: pass
    for p in reversed(procs):
        try: p.wait(timeout=3)
        except subprocess.TimeoutExpired: os.killpg(p.pid, signal.SIGKILL)
    for l in logs: l.close()
    failures = sorted(k for k, v in report['cases'].items() if not v['ok']) + sorted(set(EXPECTED) - set(report['cases']))
    report['failures'] = failures; report['ok'] = bool(report.get('done')) and not failures
    (work/'result.json').write_text(json.dumps(report, indent=1) + '\n')
    print('RESULT', 'ok' if report['ok'] else f'fail {failures}', report.get('error', ''))
    sys.exit(0 if report['ok'] else 1)
