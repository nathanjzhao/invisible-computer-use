"""Round-3 review item O1: after this driver's own write, a token for an
element labelled by its text is not rebound to whatever the element shows
now. An editable list that re-sorts on edit moved record 2 into the entry the
token named, and the second write went into record 2. The second write must
refuse, and the model re-observes. An entry with an accessible name keeps its
label while its text changes, so writing into it twice through one token works.

    probe_linux_review3_o1.py WORK CUA_DRIVER FIXTURE

FIXTURE is fixtures/resort_rows_gtk.py (GTK3 on Xvfb + openbox). The driver
runs as its own daemon; every call is background and passes an element_token.
Exits 0 only when every case matches EXPECTED.
"""
import hashlib, json, os, pathlib, select, signal, socket, subprocess, sys, time, traceback

if sys.platform != 'linux': raise SystemExit('Linux-only probe')
work = pathlib.Path(sys.argv[1]); work.mkdir(parents=True)
driver = pathlib.Path(sys.argv[2]).resolve(); fixture = pathlib.Path(sys.argv[3]).resolve()
procs = []; logs = []
report = {'driver_sha256': hashlib.sha256(driver.read_bytes()).hexdigest(),
          'probe_sha256': hashlib.sha256(pathlib.Path(__file__).read_bytes()).hexdigest(),
          'fixture_sha256': hashlib.sha256(fixture.read_bytes()).hexdigest(), 'cases': {}}
# case -> 'refused' (stale_element_target, no edit anywhere), ('record', id,
# text): exactly that list record edited to that text, or ('notes', text).
EXPECTED = {
    'resort_first_type_text': ('record', 1, 'carolz'),
    'resort_second_type_text': 'refused',
    'resort_reobserved_type_text': ('record', 1, 'Qcarolz'),
    'named_entry_first_type_text': ('notes', 'ab'),
    'named_entry_second_type_text': ('notes', 'abcd'),
    'named_entry_set_value': ('notes', 'zed'),
}
env = dict(os.environ); env.update({'XDG_RUNTIME_DIR': str(work/'runtime'), 'XDG_SESSION_TYPE': 'x11', 'GTK_MODULES': 'gail:atk-bridge', 'NO_AT_BRIDGE': '0'})
env.pop('WAYLAND_DISPLAY', None); (work/'runtime').mkdir(); (work/'runtime').chmod(0o700)
LOG = work/'events.jsonl'
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
signal.signal(signal.SIGALRM, lambda *_: (_ for _ in ()).throw(TimeoutError('deadline'))); signal.alarm(300)
try:
    x = launch(['Xvfb', '-displayfd', '1', '-screen', '0', '1280x800x24', '-nolisten', 'tcp'], 'xvfb', subprocess.PIPE)
    assert select.select([x.stdout], [], [], 8)[0]; env['DISPLAY'] = ':' + x.stdout.readline().decode().strip()
    launch(['openbox'], 'openbox'); time.sleep(.5)
    launch(['/usr/libexec/at-spi-bus-launcher', '--launch-immediately'], 'atspi'); time.sleep(1)
    launch(['/usr/bin/python3', str(fixture), str(LOG)], 'fixture')
    wait_for(lambda: any(e.get('hit') == 'ready' for e in events()), 15, 'fixture'); pid = events()[0]['pid']
    sock = work/'driver.sock'; launch([str(driver), 'serve', '--socket', str(sock)], 'driver')
    wait_for(sock.exists, 20, 'driver socket'); time.sleep(.3)
    def call(name, args): return protocol(sock, {'method': 'call', 'name': name, 'args': args, 'session_id': 'review3o1', 'client_kind': 'cli'})
    call('set_agent_cursor_enabled', {'session': 'review3o1', 'enabled': False})
    wid = wait_for(lambda: [w['window_id'] for w in call('list_windows', {})['structuredContent']['windows'] if w.get('pid') == pid], 10, 'window')[0]
    def token(label):
        state = call('get_window_state', {'pid': pid, 'window_id': wid, 'include_screenshot': False}).get('structuredContent') or {}
        found = [e for e in state.get('elements', []) if e.get('label') == label and 'text' in (e.get('role') or '')]
        report.setdefault('tokens', {})[label] = found[:1]
        assert found, f'no text element labelled {label!r}: {[(e.get("label"), e.get("role")) for e in state.get("elements", [])]}'
        return found[0]['element_token']
    def act(case, tool, args):
        b = len(events()); r = call(tool, {'pid': pid, 'window_id': wid, 'delivery_mode': 'background', **args}); time.sleep(1.0)
        sc = r.get('structuredContent') or {}
        code = (sc.get('refusal') or {}).get('code') or sc.get('code')
        edits = [e for e in events()[b:] if e.get('hit') in ('edit', 'notes')]
        want = EXPECTED[case]
        if want == 'refused':
            ok = bool(r.get('isError')) and code == 'stale_element_target' and not edits
        elif want[0] == 'record':
            ok = not r.get('isError') and bool(edits) and {e.get('record') for e in edits} == {want[1]} and edits[-1]['text'] == want[2]
        else:
            ok = not r.get('isError') and bool(edits) and {e['hit'] for e in edits} == {'notes'} and edits[-1]['text'] == want[1]
        report['cases'][case] = {'tool': tool, 'args': args, 'is_error': bool(r.get('isError')), 'code': code,
            'text': ''.join(c.get('text', '') for c in r.get('content', []))[:300], 'edits': edits,
            'rows_after': next((e['rows'] for e in reversed(events()) if e.get('rows')), None), 'expected': want, 'ok': ok}
        print('PASS' if ok else 'FAIL', case, code, [(e['hit'], e.get('record'), e.get('text')) for e in edits], flush=True)
    # Entry 0 shows record 1 "carol". Typing "z" makes it "carolz", which sorts
    # after record 2 "carol2", so entry 0 then shows carol2.
    carol = token('carol')
    act('resort_first_type_text', 'type_text', {'element_token': carol, 'text': 'z'})
    act('resort_second_type_text', 'type_text', {'element_token': carol, 'text': 'Q'})
    act('resort_reobserved_type_text', 'type_text', {'element_token': token('carolz'), 'text': 'Q'})
    notes = token('Notes')
    act('named_entry_first_type_text', 'type_text', {'element_token': notes, 'text': 'ab'})
    act('named_entry_second_type_text', 'type_text', {'element_token': notes, 'text': 'cd'})
    act('named_entry_set_value', 'set_value', {'element_token': notes, 'value': 'zed'})
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
