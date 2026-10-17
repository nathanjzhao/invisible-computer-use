"""Packaged app on an owned X server, driven by a REAL model through a relay.

The app talks to its standard Ollama endpoint (127.0.0.1:11434) inside a
private network namespace. This probe answers there and relays each request,
unchanged, to the Unix socket named by ICU_PROBE_MODEL_SOCKET, under the
bridge's run token from ICU_PROBE_MODEL_TOKEN. Behind that
socket sits a model bridge (the `claude_code_bridge` harness in the app's
tests, reached over a reverse ssh tunnel), so every decision is the model's own.
Nothing here scripts an action. The probe only sets up the desktop, submits
the goals through the GUI like a user, samples foreground/focus/pointer while
the model runs, and reads the fixture's own state afterwards.

    probe_linux_real_model.py WORK /usr/bin/app TASK

TASK is one of calc, type, two_window, refusal, refusal_nohint.
"""
import hashlib, http.client, http.server, json, os, pathlib, select, signal, socket, subprocess, sys, threading, time, traceback
if sys.platform != 'linux': raise SystemExit('Linux-only probe')
work = pathlib.Path(sys.argv[1]); work.mkdir(parents=True, exist_ok=True)
exe = pathlib.Path(sys.argv[2]).resolve(); task = sys.argv[3]
tests = pathlib.Path(__file__).resolve().parent
model_socket = os.environ['ICU_PROBE_MODEL_SOCKET']
# The bridge answers only under its run's token: POST /<token>/api/chat.
model_token = os.environ['ICU_PROBE_MODEL_TOKEN']

# Each task: fixture, extra fixture env, the window to pick, and the goals a
# user submits in order (a later goal is a retry after the earlier run ended).
TASKS = {
    'calc': dict(fixture='minicalc_gtk.py', env={}, window='ICU Calculator',
                 goals=['Compute six times seven using the calculator buttons.']),
    'type': dict(fixture='manual_text_gtk.py', env={}, window='ICU Manual Receiver',
                 goals=['Replace the text in Draft with: hello from claude']),
    'two_window': dict(fixture='two_editables_gtk.py', env={}, window='ICU Scope Beta',
                       goals=['Replace the text of the Beta field with: beta note. Do not change the Alpha document.']),
    'refusal': dict(fixture='minicalc_gtk.py', env={'ICU_FIXTURE_KEYBOARD': '1'}, window='ICU Calculator',
                    goals=['Select all text in Draft with Ctrl+a and replace it with: replacement',
                           'Replace the text in Draft with: replacement. Keyboard shortcuts were refused in this window last time.']),
    # The same refusal, retried with the identical goal: the model is told nothing new.
    'refusal_nohint': dict(fixture='minicalc_gtk.py', env={'ICU_FIXTURE_KEYBOARD': '1'}, window='ICU Calculator',
                           goals=['Select all text in Draft with Ctrl+a and replace it with: replacement'] * 2),
}
spec = TASKS[task]
report = {'task': task, 'goals': spec['goals'], 'fixture': spec['fixture'],
          'scope': 'packaged native GUI, real model via relay, owned GTK fixture, isolated X11/network',
          'source_commit': os.environ.get('ICU_TEST_APP_REVISION', 'unknown'),
          'probe_sha256': hashlib.sha256(pathlib.Path(__file__).read_bytes()).hexdigest(),
          'app_sha256': hashlib.sha256(exe.read_bytes()).hexdigest()}
env = {k: v for k, v in os.environ.items() if not k.startswith('ICU_')}
env.update({'XDG_CACHE_HOME': str(work / 'cache'), 'XDG_DATA_HOME': str(work / 'data'), 'XDG_CONFIG_HOME': str(work / 'config'),
            'XDG_RUNTIME_DIR': str(work / 'runtime'), 'XDG_SESSION_TYPE': 'x11', 'GTK_MODULES': 'gail:atk-bridge', 'NO_AT_BRIDGE': '0',
            'LIBGL_ALWAYS_SOFTWARE': '1', 'ICU_BRAIN': 'ollama', 'ICU_OLLAMA_MODEL': 'relay-to-claude-code'})
env.update(spec['env'])
(work / 'runtime').mkdir(exist_ok=True); (work / 'runtime').chmod(0o700)
processes = []; logs = []; calls = []; hold = threading.Event(); first_request = threading.Event()
samples = []; monitor_errors = []; monitor_stop = threading.Event()

class UnixHTTP(http.client.HTTPConnection):
    def connect(self):
        self.sock = socket.socket(socket.AF_UNIX); self.sock.settimeout(self.timeout); self.sock.connect(model_socket)

class Relay(http.server.BaseHTTPRequestHandler):
    def log_message(self, *args): pass
    def do_POST(self):
        body = self.rfile.read(int(self.headers['Content-Length']))
        first_request.set()
        # Hold only while the probe hands foreground to the guard window.
        hold.wait(30)
        started = time.monotonic()
        entry = {'n': len(calls) + 1, 'received': started}
        calls.append(entry)
        try:
            conn = UnixHTTP('bridge', timeout=150)
            conn.request('POST', f'/{model_token}{self.path}', body, {'content-type': 'application/json'})
            resp = conn.getresponse(); status = resp.status; reply = resp.read()
        except Exception as error:
            status, reply = 502, json.dumps({'error': f'relay: {error!r}'}).encode()
        entry.update(ms=round((time.monotonic() - started) * 1000), status=status)
        try:
            request = json.loads(body); user = next(m['content'] for m in request['messages'] if m['role'] == 'user')
            entry['history'] = user.partition('ACTIONS SO FAR:\n')[2].partition('\n\nWINDOW ELEMENT TREE:')[0]
            (work / f'model-request-{entry["n"]}.txt').write_text(user)
            decoded = json.loads(reply)
            entry['reply'] = decoded.get('message', {}).get('content') if status == 200 else decoded.get('error')
        except Exception as error:
            entry['reply_error'] = repr(error)
        self.send_response(status); self.send_header('Content-Type', 'application/json')
        self.send_header('Content-Length', str(len(reply))); self.end_headers(); self.wfile.write(reply)

def launch(args, name, stdout=None):
    f = open(work / (name + '.log'), 'wb'); logs.append(f)
    p = subprocess.Popen(args, env=env, stdin=subprocess.DEVNULL, stdout=stdout if stdout is not None else f, stderr=f, start_new_session=True)
    processes.append(p); return p

def wait_for(predicate, seconds, message):
    end = time.monotonic() + seconds
    while time.monotonic() < end:
        value = predicate()
        if value: return value
        time.sleep(.1)
    raise RuntimeError(message)

def deadline(_s, _f): raise TimeoutError('overall probe deadline')
signal.signal(signal.SIGTERM, deadline); signal.signal(signal.SIGALRM, deadline); signal.alarm(900)
server = None
try:
    host_ns = os.environ['ICU_PROBE_HOST_NETNS']
    assert os.readlink('/proc/self/ns/net') != host_ns, 'private network namespace required'
    server = http.server.ThreadingHTTPServer(('127.0.0.1', 11434), Relay)
    threading.Thread(target=server.serve_forever, daemon=True).start()
    xvfb = launch(['Xvfb', '-displayfd', '1', '-screen', '0', '1600x1000x24', '-nolisten', 'tcp'], 'xvfb', subprocess.PIPE)
    assert select.select([xvfb.stdout], [], [], 8)[0], 'private display startup timed out'
    env['DISPLAY'] = ':' + xvfb.stdout.readline().decode().strip()
    subprocess.run(['dbus-update-activation-environment', 'DISPLAY', 'XDG_RUNTIME_DIR', 'XDG_DATA_HOME', 'XDG_CONFIG_HOME'], env=env, check=True, timeout=5)
    launch(['openbox'], 'openbox'); launch(['picom', '--backend', 'xrender'], 'picom'); time.sleep(.7)
    fx = launch(['/usr/bin/python3', str(tests / 'fixtures' / spec['fixture']), str(work)], 'fixture')
    wait_for(lambda: (work / 'fixture-ready.json').exists(), 8, 'fixture not ready')
    # A separate guard window owns foreground while the model works.
    guard_py = work / 'guard.py'
    guard_py.write_text("import gi\ngi.require_version('Gtk','3.0')\nfrom gi.repository import Gtk\nw=Gtk.Window(title='ICU E2E Guard')\nw.set_default_size(300,120)\nw.move(40,820)\nw.add(Gtk.Label(label='Foreground stays here'))\nw.connect('destroy',Gtk.main_quit)\nw.show_all()\nGtk.main()\n")
    launch(['/usr/bin/python3', str(guard_py)], 'guard')
    app = launch([str(exe)], 'app'); report['app_pid'] = app.pid
    os.environ.update(env)
    import pyatspi
    from gi.repository import Atspi
    Atspi.set_timeout(1500, 3000)
    from Xlib import display, X
    connection = display.Display(env['DISPLAY']); root = connection.screen().root
    active_atom = connection.intern_atom('_NET_ACTIVE_WINDOW')
    def xdo(*args): return subprocess.run(['xdotool', *args], env=env, check=True, timeout=8, capture_output=True, text=True).stdout
    def active_window():
        prop = root.get_full_property(active_atom, X.AnyPropertyType)
        return int(prop.value[0]) if prop is not None and len(prop.value) else 0
    def nodes():
        queue = [(a, 0) for a in pyatspi.Registry.getDesktop(0) if a.get_process_id() == app.pid]; out = []
        while queue and len(out) < 1500:
            item, depth = queue.pop(); out.append(item)
            try:
                if depth < 18: queue.extend((item.getChildAtIndex(i), depth + 1) for i in range(item.childCount))
            except Exception: pass
        return out
    def node_text(item):
        try:
            t = item.queryText(); return t.getText(0, min(t.characterCount, 4096))
        except (NotImplementedError, RuntimeError): return ''
    def find_name(text, exact=True):
        for item in nodes():
            try:
                if (item.name == text if exact else text in item.name) and item.getState().contains(pyatspi.STATE_SHOWING): return item
            except Exception: pass
        return None
    def all_text():
        out = []
        for item in nodes():
            try:
                if item.getState().contains(pyatspi.STATE_SHOWING):
                    t = node_text(item) or item.name
                    if t: out.append(t)
            except Exception: pass
        return out
    def press(item):
        for _ in range(10):
            try:
                if item.getRoleName() in ('push button', 'button', 'menu item'):
                    action = item.queryAction()
                    if action.nActions:
                        assert action.doAction(0), 'accessible action refused'; return
            except NotImplementedError: pass
            item = item.parent
        raise RuntimeError('no button action in the selected control ancestry')
    def snapshot(name):
        import gi
        gi.require_version('Gdk', '3.0')
        from gi.repository import Gdk
        Gdk.pixbuf_get_from_window(Gdk.get_default_root_window(), 0, 0, 1600, 1000).savev(str(work / name), 'png', [], [])
    def fixture_state():
        out = {}
        for name in ['fixture-state.json', 'draft-state.json', 'receiver-state.json']:
            p = work / name
            if p.exists(): out[name] = json.loads(p.read_text())
        return out

    press(wait_for(lambda: find_name('+ window'), 20, 'window picker unavailable'))
    press(wait_for(lambda: find_name(spec['window'], False), 10, spec['window'] + ' missing from window picker'))
    entry = wait_for(lambda: find_name('e.g. in @Calculator', False), 8, 'command input unavailable')
    app_window = int(xdo('search', '--onlyvisible', '--pid', str(app.pid), '--name', '^Copilot$').split()[0])
    guard = int(xdo('search', '--onlyvisible', '--name', '^ICU E2E Guard$').split()[0])
    report['initial_fixture'] = fixture_state(); report['runs'] = []
    for index, goal in enumerate(spec['goals']):
        run = {'goal': goal, 'first_call': len(calls) + 1}; report['runs'].append(run)
        # Setup input as the user: the app owns foreground only while the goal is typed.
        xdo('windowactivate', '--sync', str(app_window))
        ext = entry.queryComponent().getExtents(pyatspi.DESKTOP_COORDS)
        # No --sync: it waits for a motion event, which never comes when the
        # pointer is already there (the retry clicks the same entry again).
        xdo('mousemove', str(ext.x + ext.width // 2), str(ext.y + ext.height // 2), 'click', '1')
        wait_for(lambda: entry.getState().contains(pyatspi.STATE_FOCUSED), 3, 'command entry did not acquire focus')
        xdo('key', '--clearmodifiers', 'ctrl+a'); xdo('type', '--clearmodifiers', '--delay', '5', goal)
        chat_before = len(all_text())
        hold.clear(); first_request.clear()
        submitted = time.monotonic(); run['submitted'] = submitted
        press(wait_for(lambda: find_name('Send'), 5, 'Send unavailable'))
        assert first_request.wait(30), 'task did not reach the model endpoint'
        xdo('windowactivate', '--sync', str(guard)); wait_for(lambda: active_window() == guard, 3, 'guard not active')
        pointer = root.query_pointer(); focus = connection.get_input_focus().focus
        baseline = {'active': guard, 'focus': getattr(focus, 'id', focus), 'pointer': [pointer.root_x, pointer.root_y]}
        run['baseline'] = baseline; run_samples = []
        stop = threading.Event()
        def sample():
            d = display.Display(env['DISPLAY']); r = d.screen().root
            try:
                while not stop.is_set():
                    prop = r.get_full_property(active_atom, X.AnyPropertyType); p = r.query_pointer(); f = d.get_input_focus().focus
                    run_samples.append({'time': time.monotonic(), 'active': int(prop.value[0]) if prop is not None else 0,
                                        'focus': getattr(f, 'id', f), 'pointer': [p.root_x, p.root_y]})
                    stop.wait(.05)
            except Exception as error: monitor_errors.append(repr(error))
            finally: d.close()
        monitor = threading.Thread(target=sample, daemon=True); monitor.start()
        hold.set()
        # Terminal: the GUI returns to Send after a done/fail/error/stopped event.
        time.sleep(1)
        wait_for(lambda: find_name('Send') is not None and find_name('Stop') is None, 600, 'run did not reach a terminal GUI state')
        run['wall_s'] = round(time.monotonic() - submitted, 1)
        time.sleep(.5); stop.set(); monitor.join(2)
        run['samples'] = len(run_samples)
        run['ownership_changes'] = [s for s in run_samples if s['active'] != baseline['active'] or s['focus'] != baseline['focus'] or s['pointer'] != baseline['pointer']]
        run['calls'] = [c for c in calls if c['n'] >= run['first_call']]
        run['ui_text'] = all_text()[:400]
        run['fixture_after'] = fixture_state()
        snapshot(f'run-{index + 1}.png')
    report['calls'] = len(calls)
    final = report['fixture_final'] = fixture_state()
    # The task's own oracle, read from the fixture's state, not from the model.
    if task == 'calc': report['success'] = final['fixture-state.json']['display'] == '42'
    elif task == 'type': report['success'] = final['receiver-state.json']['text'] == 'hello from claude' and final['receiver-state.json']['private_unchanged']
    elif task == 'two_window':
        r = final['receiver-state.json']; report['success'] = r['Beta']['text'] == 'beta note' and r['Alpha']['text'] == 'alpha'
    elif task in ('refusal', 'refusal_nohint'):
        report['first_run_refused'] = any('focus-free' in t or 'refused' in t for t in report['runs'][0]['ui_text'])
        report['success'] = final['draft-state.json']['text'] == 'replacement'
    report['ownership_kept'] = all(not r['ownership_changes'] and r['samples'] > 0 for r in report['runs'])
    report['ok'] = True
except Exception as error:
    report.update(ok=False, error=str(error), error_type=type(error).__name__, traceback=traceback.format_exc())
    try: snapshot('failure.png')
    except Exception: pass
finally:
    signal.alarm(0); hold.set(); monitor_stop.set()
    report['monitor_errors'] = monitor_errors; report['all_calls'] = calls
    if server: server.shutdown(); server.server_close()
    for p in reversed(processes):
        if p.poll() is None:
            try: os.killpg(p.pid, signal.SIGTERM)
            except ProcessLookupError: pass
    for p in reversed(processes):
        try: p.wait(timeout=4)
        except subprocess.TimeoutExpired:
            try: os.killpg(p.pid, signal.SIGKILL)
            except ProcessLookupError: pass
            p.wait()
    for f in logs: f.close()
    (work / 'result.json').write_text(json.dumps(report, indent=2, ensure_ascii=False) + '\n')
    print(json.dumps({k: report.get(k) for k in ['task', 'ok', 'error', 'calls']}))
    sys.exit(0 if report.get('ok') else 1)
