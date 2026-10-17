"""Minimized, hidden and off-Space targets for installed Copilot.app.

On a disposable Mac, the owned AppKit calculator is minimized, ordered out or
moved to a never-shown Space by its own process (the user's window management).
For each state the probe records what the owned driver reports and does, then
runs a real chat task on the window while a separate guard app owns the
foreground. Pass criteria per state:

- no activation: the guard keeps frontmost, menu bar, keyboard focus and
  pointer; the active Space id never changes; the fixture is never activated;
- no silent state change: minimized stays minimized, hidden stays hidden and
  the off-Space window stays off the active Space;
- truthfulness: when input has no effect, the driver returns an error and the
  GUI reports a failure, never "done"; when the task reaches done, the fixture
  really shows 42.

  probe_macos_spaces.py <work> <Copilot.app> <minicalc_mac> <macos_ui>
"""
import hashlib, http.server, json, os, pathlib, re, signal, socket, subprocess, sys, threading, time, traceback
sys.path.insert(0, str(pathlib.Path(__file__).resolve().parent))
from macos_probe import Session, wait_for, center

if sys.platform != 'darwin': raise SystemExit('macOS-only probe')
work = pathlib.Path(sys.argv[1])
report = {'source_commit': os.environ.get('ICU_TEST_APP_REVISION', 'unknown'), 'cases': {},
          'scope': 'installed Copilot.app, owned AppKit calculator minimized / ordered out / on a never-shown Space, guard app in front',
          'probe_sha256': hashlib.sha256(pathlib.Path(__file__).read_bytes()).hexdigest()}
s = Session(work, *sys.argv[2:5], report)
model_requests = []; server = None; task = {'batches': 0}


class Model(http.server.BaseHTTPRequestHandler):
    def log_message(self, *args): pass
    def do_POST(self):
        body = json.loads(self.rfile.read(int(self.headers['Content-Length'])))
        model_requests.append(body)
        text = next(m['content'] for m in body['messages'] if m['role'] == 'user')
        observation = text.partition('\n\nWINDOW ELEMENT TREE:\n')[2].rpartition('\n\nNext action as JSON:')[0]
        # Done only after this task's own clicks produced a fresh 42.
        if task['batches'] and '"Result" value="42"' in observation: decision = {'action': {'kind': 'done', 'summary': 'Observed 42'}}
        elif task['batches'] < 2:
            task['batches'] += 1
            decision = {'thought': 'compute', 'actions': [{'kind': 'click', 'target': t} for t in ['C', '6', '×', '7', '=']]}
        else: decision = {'action': {'kind': 'fail', 'reason': '42 absent from fresh observation'}}
        encoded = json.dumps({'message': {'role': 'assistant', 'content': json.dumps(decision, ensure_ascii=False)}}).encode()
        self.send_response(200); self.send_header('Content-Type', 'application/json'); self.send_header('Content-Length', str(len(encoded))); self.end_headers(); self.wfile.write(encoded)


def command(cmd):
    before = len(jsonl('command-results.jsonl'))
    (work / 'command-calc.json').write_text(json.dumps({'cmd': cmd}))
    return wait_for(lambda: (lambda r: r[before] if len(r) > before else None)(jsonl('command-results.jsonl')), 8, 'fixture did not run ' + cmd)


def jsonl(name):
    p = work / name
    return [json.loads(l) for l in p.read_text().splitlines() if l.strip()] if p.exists() else []


def record(window_id):
    raw = s.call('list_windows', {}); sc = raw.get('structuredContent', raw)
    w = next((w for w in sc['windows'] if w['window_id'] == window_id), None)
    return {'current_space_id': sc.get('current_space_id'), 'window': w}


def entry():
    return s.find(lambda n: n.get('role') == 'AXTextField' and 'e.g. in @Calculator' in n.get('placeholder', ''))


def deadline(_s, _f): raise TimeoutError('spaces probe deadline')
signal.signal(signal.SIGALRM, deadline); signal.alarm(420)
try:
    with socket.socket() as port:
        assert port.connect_ex(('127.0.0.1', 11434)) != 0, 'port 11434 already has a listener'
    server = http.server.ThreadingHTTPServer(('127.0.0.1', 11434), Model)
    threading.Thread(target=server.serve_forever, daemon=True).start()
    fx, _ = s.fixture_proc('calc'); guard, _ = s.fixture_proc('guard')
    s.start_app(['ICU_BRAIN=ollama'])
    windows = s.windows()
    target = next(w for w in windows if w.get('pid') == fx.pid and w.get('title') == 'ICU Calculator')
    wid = target['window_id']
    s.pick_window('ICU Calculator')
    for case, enter, leave in [('baseline', None, None), ('minimized', 'minimize', 'restore'), ('hidden', 'hide', 'show'), ('off_space', 'offspace', 'onspace')]:
        c = report['cases'][case] = {}
        command('clear')
        pre = s.call('get_window_state', {'pid': fx.pid, 'window_id': wid, 'include_screenshot': False})
        six = next(e for e in pre.get('structuredContent', pre)['elements'] if e.get('label') == '6')
        if enter: c['command'] = command(enter)
        time.sleep(3)  # the app refreshes window metadata every 2.5 s
        c['driver_record'] = record(wid)
        c['tile_off_space_badge'] = s.find(s.named('off-Space', 'AXButton')) is not None
        s.screenshot(f'tile-{case}.png')
        state = s.call('get_window_state', {'pid': fx.pid, 'window_id': wid, 'include_screenshot': False})
        sc = state.get('structuredContent', state)
        c['driver_state'] = {'isError': state.get('isError'), 'elements': len(sc.get('elements', []) or []),
                             'text': ' '.join(x.get('text', '') for x in state.get('content', []) if x.get('type') == 'text')[:300]}
        clicked = s.call('click', {'pid': fx.pid, 'window_id': wid, 'element_token': six.get('element_token')})
        c['driver_stale_token_click'] = {'isError': clicked.get('isError'), 'text': ' '.join(x.get('text', '') for x in clicked.get('content', []) if x.get('type') == 'text')[:300]}
        # A coordinate click at the 6 key, as manual tile input sends it (1x display).
        wb = pre.get('structuredContent', pre).get('window_bounds') or target['bounds']
        f = six['frame']; point = {'x': f['x'] + f['w'] / 2 - wb['x'], 'y': f['y'] + f['h'] / 2 - wb['y']}
        guard_field = next(n for n in s.tree(guard.pid) if n.get('role') == 'AXTextField')
        s.ui('click', *center(guard_field))
        wait_for(lambda: s.ui('state')['front_pid'] == guard.pid, 5, 'guard did not take the foreground')
        cbase = s.ui('state', str(s.driver_pid)); cpath = work / f'ownership-{case}-coordinate.jsonl'
        sampler = subprocess.Popen([str(s.ui_bin), 'sample', str(cpath), str(s.driver_pid)], stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
        time.sleep(.3)
        events_mark = len(jsonl('receiver-events.jsonl'))
        pointed = s.call('click', {'pid': fx.pid, 'window_id': wid, **point})
        time.sleep(.8)
        sampler.send_signal(signal.SIGTERM); sampler.wait(5)
        csamples = [json.loads(l) for l in cpath.read_text().splitlines() if l.strip()]
        c['driver_coordinate_click'] = {'point': point, 'isError': pointed.get('isError'),
                                        'text': ' '.join(x.get('text', '') for x in pointed.get('content', []) if x.get('type') == 'text')[:300],
                                        'receiver_events': jsonl('receiver-events.jsonl')[events_mark:], 'samples': len(csamples),
                                        'ownership_violations': [x for x in csamples if x['front_pid'] != guard.pid or x['menubar_pid'] != guard.pid
                                                                 or x.get('focused_element') != cbase.get('focused_element') or x['pointer'] != cbase['pointer']][:5],
                                        'activation_events': [e for e in jsonl('activation-events.jsonl') if e['time'] >= cbase['time']]}
        cc = c['driver_coordinate_click']
        c['checks_extra'] = {'coordinate_click_truthful': bool(cc['receiver_events']) != bool(pointed.get('isError')),
                             'coordinate_click_no_ownership_change': not cc['ownership_violations'] and cc['samples'] >= 10,
                             'coordinate_click_no_appkit_activation': not cc['activation_events'] and all(not e['active'] for e in cc['receiver_events'])}
        s.ui('click', 960, 110)
        wait_for(lambda: s.ui('state')['front_pid'] == s.app_pid, 5, 'Copilot did not come forward')
        events_before = len(jsonl('receiver-events.jsonl')); requests_before = len(model_requests)
        # The user types a task into Copilot, then leaves for another app.
        s.ui('click', *center(entry())); s.ui('key', 0, 'cmd'); s.ui('type', 'Compute six times seven using the calculator buttons.')
        wait_for(lambda: (entry() or {}).get('value') == 'Compute six times seven using the calculator buttons.', 5, 'goal not typed')
        guard_field = next(n for n in s.tree(guard.pid) if n.get('role') == 'AXTextField')
        s.ui('click', *center(guard_field))
        wait_for(lambda: s.ui('state')['front_pid'] == guard.pid, 5, 'guard did not take the foreground')
        baseline = s.ui('state', str(s.driver_pid)); c['space_before'] = record(wid)['current_space_id']
        path = work / f'ownership-{case}.jsonl'
        sampler = subprocess.Popen([str(s.ui_bin), 'sample', str(path), str(s.driver_pid)], stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
        time.sleep(.3)
        task['batches'] = 0
        s.press('Send')  # an accessibility press: the user's click already left Copilot
        wait_for(lambda: s.find(s.named('Send', 'AXButton')) and not s.find(s.named('Stop', 'AXButton')), 30, 'task did not finish')
        time.sleep(.6)
        sampler.send_signal(signal.SIGTERM); sampler.wait(5)
        texts = [n['name'] for n in s.tree() if n.get('role') == 'AXStaticText']
        agent = [t for t in texts if t.startswith(('done.', 'error:', "can't do that", 'stopped:', 'on it', "couldn't start"))]
        last_start = max((i for i, t in enumerate(agent) if t.startswith('on it')), default=-1)
        c['gui_messages'] = agent[last_start:] if last_start >= 0 else agent[-1:]
        samples = [json.loads(l) for l in path.read_text().splitlines() if l.strip()]
        c['samples'] = len(samples)
        c['ownership_violations'] = [x for x in samples if x['front_pid'] != guard.pid or x['menubar_pid'] != guard.pid
                                     or x.get('focused_element') != baseline.get('focused_element') or x['pointer'] != baseline['pointer']][:5]
        c['driver_onscreen_window_samples'] = sum(bool(x.get('owned_onscreen_windows')) for x in samples)
        c['space_after'] = record(wid)['current_space_id']
        c['model_requests'] = len(model_requests) - requests_before
        c['receiver_events'] = jsonl('receiver-events.jsonl')[events_before:]
        c['fixture_state'] = json.loads((work / 'fixture-state.json').read_text())
        if enter:
            (work / 'command-calc.json').write_text(json.dumps({'cmd': 'status'}))
            status = wait_for(lambda: (lambda r: r[-1] if r and r[-1]['cmd'] == 'status' else None)(jsonl('command-results.jsonl')), 8, 'status missing')
            c['window_after_task'] = status['window']
        c['activation_events'] = [e for e in jsonl('activation-events.jsonl') if e['time'] >= baseline['time']]
        done = any(t.startswith('done.') for t in c['gui_messages'])
        c['claimed_done'] = done
        c['checks'] = {
            'no_ownership_change': not c['ownership_violations'] and c['samples'] >= 10,
            'no_overlay': c['driver_onscreen_window_samples'] == 0,
            'space_unchanged': c['space_before'] == c['space_after'],
            'fixture_never_activated': not any(e['kind'] == 'active' for e in c['activation_events']) and all(not e['active'] for e in c['receiver_events']),
            'state_preserved': enter is None or {
                'minimize': lambda w: w['miniaturized'], 'hide': lambda w: not w['visible'] and not w['miniaturized'],
                'offspace': lambda w: not w['on_active_space']}[enter](c['window_after_task']),
            'done_only_if_true': (not done) or c['fixture_state']['display'] == '42',
            'failure_is_reported': done or any(t.startswith(('error:', "can't do that", 'stopped:')) for t in c['gui_messages']),
        }
        if case in ('baseline', 'off_space'):
            # The driver reports Space membership truthfully and the tile shows it.
            on = (c['driver_record']['window'] or {}).get('on_current_space')
            c['checks']['space_field_truthful'] = on is (case == 'baseline')
            c['checks']['badge_matches_space'] = c['tile_off_space_badge'] == (case == 'off_space')
        if leave: c['leave'] = command(leave); time.sleep(1)
        s.ui('click', 960, 110)  # back to Copilot for the next case
        wait_for(lambda: s.ui('state')['front_pid'] == s.app_pid, 5, 'Copilot did not come forward')
    report['failed_checks'] = {k: [n for n, v in c['checks'].items() if not v] for k, c in report['cases'].items() if not all(c['checks'].values())}
    s.screenshot('completed.png')
    report['ok'] = not report['failed_checks']
except Exception as error:
    report.update(ok=False, error=str(error), error_type=type(error).__name__, traceback=traceback.format_exc())
    s.screenshot('failure.png')
finally:
    signal.alarm(0)
    if server: server.shutdown(); server.server_close()
    s.close()
    (work / 'result.json').write_text(json.dumps(report, indent=2, ensure_ascii=False) + '\n')
    summary = {k: {'checks': c.get('checks'), 'gui': c.get('gui_messages'), 'driver_state': c.get('driver_state'), 'click': c.get('driver_stale_token_click'),
                   'record': (c.get('driver_record') or {}).get('window', {}) and {x: c['driver_record']['window'].get(x) for x in ('is_on_screen', 'on_current_space', 'space_ids')}}
               for k, c in report['cases'].items()}
    print(json.dumps({'ok': report.get('ok'), 'error': report.get('error'), 'failed': report.get('failed_checks'), 'cases': summary}, indent=1, ensure_ascii=False)[:8000])
    if report.get('error'): print(report.get('traceback', ''))
    sys.exit(0 if report.get('ok') else 1)
