"""Real installed-webview text input on a disposable X server and clipboard."""
import hashlib
import json
import os
from pathlib import Path
import select
import signal
import socket
import subprocess
import sys
import threading
import time
import traceback

if sys.platform != 'linux':
    raise SystemExit('Linux-only probe')
work = Path(sys.argv[1]); work.mkdir(parents=True, exist_ok=True)
exe = Path(sys.argv[2]).resolve(); fixture = Path(sys.argv[3]).resolve()
report = {'source_commit': os.environ.get('ICU_TEST_APP_REVISION', 'unknown'),
          'scope': 'installed native webview; owned GTK receiver; isolated X11 and clipboard',
          'probe_sha256': hashlib.sha256(Path(__file__).read_bytes()).hexdigest(),
          'app_sha256': hashlib.sha256(exe.read_bytes()).hexdigest(),
          'driver_sha256': hashlib.sha256(exe.with_name('cua-driver').read_bytes()).hexdigest()}
env = {k: v for k, v in os.environ.items() if not k.startswith('ICU_')}
env.update({key: str(work / name) for key, name in [
    ('XDG_CACHE_HOME', 'cache'), ('XDG_DATA_HOME', 'data'),
    ('XDG_CONFIG_HOME', 'config'), ('XDG_RUNTIME_DIR', 'runtime')]})
env.update(XDG_SESSION_TYPE='x11', GTK_MODULES='gail:atk-bridge', NO_AT_BRIDGE='0',
           LIBGL_ALWAYS_SOFTWARE='1', ICU_BRAIN='ollama')
(work / 'runtime').mkdir(mode=0o700, exist_ok=True)
processes = []; logs = []; samples = []; monitor_errors = []
monitor_stop = threading.Event(); monitor = None; driver_identity = None

def identity(pid):
    try: return Path(f'/proc/{pid}/stat').read_text().rpartition(') ')[2].split()[19]
    except FileNotFoundError: return None

def launch(args, name, stdout=None, stdin=None):
    log = (work / (name + '.log')).open('wb'); logs.append(log)
    process = subprocess.Popen(args, env=env, stdin=stdin or subprocess.DEVNULL,
                               stdout=stdout or log, stderr=log, start_new_session=True)
    processes.append(process); return process

def wait_for(check, seconds, message):
    deadline = time.monotonic() + seconds
    while time.monotonic() < deadline:
        value = check()
        if value: return value
        time.sleep(.05)
    raise RuntimeError(message)

def protocol(request):
    with socket.socket(socket.AF_UNIX) as conn:
        conn.settimeout(5); conn.connect(str(sock))
        conn.sendall((json.dumps(request) + '\n').encode()); data = b''
        deadline = time.monotonic() + 5
        while b'\n' not in data:
            if time.monotonic() >= deadline: raise TimeoutError('protocol deadline')
            chunk = conn.recv(65536)
            if not chunk: raise ConnectionError('peer closed')
            data += chunk
            if len(data) > 4 * 1024 * 1024: raise ValueError('oversized response')
        result = json.loads(data.split(b'\n')[0]); assert result.get('ok'), result
        return result['result']

def deadline(_signum, _frame): raise TimeoutError('native manual probe deadline')
signal.signal(signal.SIGALRM, deadline); signal.signal(signal.SIGTERM, deadline); signal.alarm(120)
try:
    host_ns = os.environ['ICU_PROBE_HOST_NETNS']; test_ns = os.readlink('/proc/self/ns/net')
    assert host_ns != test_ns, 'private network namespace required'
    report.update(host_network_namespace=host_ns, test_network_namespace=test_ns)
    xvfb = launch(['Xvfb', '-displayfd', '1', '-screen', '0', '1600x1000x24', '-nolisten', 'tcp'], 'xvfb', stdout=subprocess.PIPE)
    assert select.select([xvfb.stdout], [], [], 8)[0], 'display startup timeout'
    number = xvfb.stdout.readline().decode().strip(); assert number.isdigit()
    env['DISPLAY'] = ':' + number; report['display'] = env['DISPLAY']
    subprocess.run(['dbus-update-activation-environment', 'DISPLAY', 'XDG_RUNTIME_DIR', 'XDG_DATA_HOME', 'XDG_CONFIG_HOME'], env=env, check=True, timeout=5)
    launch(['openbox'], 'openbox'); launch(['picom', '--backend', 'xrender'], 'picom'); time.sleep(.7)
    receiver = launch(['/usr/bin/python3', str(fixture), str(work)], 'receiver')
    wait_for(lambda: (work / 'fixture-ready.json').exists(), 8, 'receiver not ready')
    app = launch([str(exe)], 'app'); report.update(app_pid=app.pid, receiver_pid=receiver.pid)
    sock = work / 'cache/icu' / f'driver-{app.pid}.sock'
    def ready():
        if app.poll() is not None: raise RuntimeError('packaged app exited')
        try: return protocol({'method': 'metadata'})
        except (OSError, ValueError): return None
    meta = wait_for(ready, 30, 'owned driver not ready'); report['metadata'] = meta
    driver_identity = (meta['pid'], identity(meta['pid']))
    assert driver_identity[1] and Path(f'/proc/{meta["pid"]}/exe').resolve() == exe.with_name('cua-driver')
    report['driver_argv'] = Path(f'/proc/{meta["pid"]}/cmdline').read_bytes().decode().rstrip('\0').split('\0')
    assert '--no-overlay' in report['driver_argv']
    raw = protocol({'method':'call', 'name':'list_windows', 'args':{}, 'session_id':'icu-manual-observer', 'client_kind':'cli'})
    (work / 'windows.json').write_text(json.dumps(raw, indent=2))
    windows = raw.get('structuredContent', raw)['windows']
    app_window = next(w['window_id'] for w in windows if w.get('pid') == app.pid and w.get('title') == 'Copilot')
    receiver_window = next(w['window_id'] for w in windows if w.get('pid') == receiver.pid and w.get('title') == 'ICU Manual Receiver')
    os.environ.update(env)
    import pyatspi
    from gi.repository import Atspi
    Atspi.set_timeout(1500, 3000)
    from Xlib import display, X
    connection = display.Display(env['DISPLAY']); root = connection.screen().root
    active_atom = connection.intern_atom('_NET_ACTIVE_WINDOW')
    def nodes():
        queue = [(a, 0) for a in pyatspi.Registry.getDesktop(0) if a.get_process_id() == app.pid]; out = []
        while queue and len(out) < 1500:
            node, depth = queue.pop(); out.append(node)
            try:
                if depth < 18: queue.extend((node.getChildAtIndex(i), depth + 1) for i in range(node.childCount))
            except Exception: pass
        return out
    def find(text, exact=False):
        for node in nodes():
            try:
                match = node.name.casefold() == text.casefold() if exact else text.casefold() in node.name.casefold()
                if match and node.getState().contains(pyatspi.STATE_SHOWING): return node
            except Exception: pass
        return None
    def press(node):
        for _ in range(10):
            try:
                if node.getRoleName() in ('button', 'push button', 'menu item'):
                    action = node.queryAction()
                    if action.nActions:
                        assert action.doAction(0); return
            except NotImplementedError: pass
            node = node.parent
        raise RuntimeError('selected control has no button action')
    def xdo(*args): subprocess.run(['xdotool', *args], env=env, check=True, timeout=8)
    def snapshot(name):
        import gi
        gi.require_version('Gdk', '3.0')
        from gi.repository import Gdk
        Gdk.pixbuf_get_from_window(Gdk.get_default_root_window(), 0, 0, 1600, 1000).savev(str(work / name), 'png', [], [])
    def receiver_state(): return json.loads((work / 'receiver-state.json').read_text())
    def observe(tag):
        value = protocol({'method':'call', 'name':'get_window_state', 'args':{'pid':receiver.pid,'window_id':receiver_window,'include_screenshot':False}, 'session_id':'icu-manual-observer', 'client_kind':'cli'})
        (work / (tag + '-driver-state.json')).write_text(json.dumps(value, indent=2))
        assert 'fixture-private-value' not in json.dumps(value), 'private field leaked'
        return value
    press(wait_for(lambda: find('+ window', True), 12, 'picker unavailable'))
    press(wait_for(lambda: find('ICU Manual Receiver'), 10, 'receiver absent from picker'))
    control = wait_for(lambda: find('control', True), 10, 'control button absent')
    xdo('windowactivate', '--sync', str(app_window)); time.sleep(.5)
    box = control.queryComponent().getExtents(pyatspi.DESKTOP_COORDS)
    assert box.width > 0 and box.height > 0
    xdo('mousemove', '--sync', str(box.x + box.width // 2), str(box.y + box.height // 2), 'click', '1')
    time.sleep(.5)
    report['editor_exposed'] = find('Type into') is not None
    report['initial_receiver'] = receiver_state(); assert report['initial_receiver']['text'] == 'seed'
    assert report['initial_receiver']['draft_selected_by_window'], 'fixture did not initially select Draft'
    observe('initial')
    def ownership(d):
        rt = d.screen().root; prop = rt.get_full_property(active_atom, X.AnyPropertyType)
        pointer = rt.query_pointer(); focus = d.get_input_focus().focus
        return {'time':time.monotonic(), 'active':int(prop.value[0]) if prop is not None else 0,
                'focus':getattr(focus, 'id', focus), 'pointer':[pointer.root_x, pointer.root_y]}
    baseline = ownership(connection); report['baseline'] = baseline
    assert baseline['active'] == app_window, 'app must own foreground before manual input'
    def sample():
        d = display.Display(env['DISPLAY'])
        try:
            while not monitor_stop.is_set(): samples.append(ownership(d)); monitor_stop.wait(.02)
        except Exception as error: monitor_errors.append(repr(error))
        finally: d.close()
    monitor = threading.Thread(target=sample, daemon=True); monitor.start()
    report['stage'] = 'ascii'
    xdo('type', '--clearmodifiers', '--delay', '20', 'A')
    wait_for(lambda: receiver_state()['text'] == 'seedA', 8, 'ASCII input did not reach receiver')
    report['ascii'] = receiver_state(); observe('ascii')
    report['stage'] = 'unicode_composition'
    xdo('key', '--clearmodifiers', 'ctrl+shift+u')
    xdo('type', '--clearmodifiers', '--delay', '60', '65e5')
    time.sleep(.3)
    report['before_composition_commit'] = receiver_state()
    assert report['before_composition_commit']['text'] == 'seedA', 'provisional Unicode composition escaped'
    xdo('key', '--clearmodifiers', 'Return')
    wait_for(lambda: receiver_state()['text'] == 'seedA日', 8, 'Unicode commit did not reach receiver')
    report['unicode'] = receiver_state(); observe('unicode')
    report['stage'] = 'paste'
    pasted = '\nsecond\t🙂  '
    clipboard = launch(['xclip', '-selection', 'clipboard', '-in', '-quiet'], 'clipboard', stdin=subprocess.PIPE)
    clipboard.stdin.write(pasted.encode()); clipboard.stdin.close(); time.sleep(.2)
    xdo('key', '--clearmodifiers', 'ctrl+v')
    expected = 'seedA日' + pasted
    wait_for(lambda: receiver_state()['text'] == expected, 8, 'paste did not reach receiver exactly')
    report['paste'] = receiver_state(); report['expected'] = expected; observe('final')
    time.sleep(1); snapshot('completed.png')
    monitor_stop.set(); monitor.join(2)
    assert samples and not monitor_errors
    assert all(all(s[k] == baseline[k] for k in ['active','focus','pointer']) for s in samples), 'manual forwarding changed ownership'
    events = [json.loads(line) for line in (work / 'receiver-events.jsonl').read_text().splitlines()]
    events = [event for event in events if event['time'] >= baseline['time']]
    report['receiver_events'] = events
    assert events and all(not event['active'] for event in events), 'receiver became active during insertion'
    assert all(event['private_unchanged'] for event in events), 'manual text reached the private sibling field'
    assert not any('65e5' in event['text'] for event in events), 'IME preedit leaked into receiver'
    report['ok'] = True
except Exception as error:
    report.update(ok=False, error=str(error), error_type=type(error).__name__, traceback=traceback.format_exc())
    try: report['receiver_at_failure'] = receiver_state()
    except Exception: pass
    try: report['ui_nodes'] = [{'name':n.name,'role':n.getRoleName()} for n in nodes()][:500]
    except Exception: pass
    try: snapshot('failure.png')
    except Exception: pass
finally:
    signal.alarm(0); monitor_stop.set()
    if monitor: monitor.join(2)
    report['samples'] = samples; report['monitor_errors'] = monitor_errors
    for process in reversed(processes):
        if process.poll() is None:
            try: os.killpg(process.pid, signal.SIGTERM)
            except ProcessLookupError: pass
    for process in reversed(processes):
        try: process.wait(timeout=4)
        except subprocess.TimeoutExpired:
            try: os.killpg(process.pid, signal.SIGKILL)
            except ProcessLookupError: pass
            process.wait()
    if driver_identity:
        pid, started = driver_identity
        for sig in [signal.SIGTERM, signal.SIGKILL]:
            if identity(pid) != started: break
            try: os.kill(pid, sig)
            except ProcessLookupError: pass
            until = time.monotonic() + 2
            while identity(pid) == started and time.monotonic() < until: time.sleep(.02)
        report['owned_daemon_exited'] = identity(pid) != started
        if not report['owned_daemon_exited']: report['ok'] = False
    report['owned_processes_reaped'] = all(p.poll() is not None for p in processes)
    for log in logs: log.close()
    (work / 'result.json').write_text(json.dumps(report, indent=2) + '\n')
    print(json.dumps(report, indent=2)); sys.exit(0 if report.get('ok') else 1)
