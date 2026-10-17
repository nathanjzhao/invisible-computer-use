"""Exact-window native text routing with two editables owned by one process."""
import hashlib
import json
import os
from pathlib import Path
import select
import signal
import shutil
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
          'scope': 'native type_text boundary; two same-PID GTK windows; isolated X11',
          'probe_sha256': hashlib.sha256(Path(__file__).read_bytes()).hexdigest(),
          'app_sha256': hashlib.sha256(exe.read_bytes()).hexdigest(),
          'driver_sha256': hashlib.sha256(exe.with_name('cua-driver').read_bytes()).hexdigest()}
env = {k: v for k, v in os.environ.items() if not k.startswith('ICU_')}
env.update({key: str(work / name) for key, name in [
    ('XDG_CACHE_HOME', 'cache'), ('XDG_DATA_HOME', 'data'),
    ('XDG_CONFIG_HOME', 'config'), ('XDG_RUNTIME_DIR', 'runtime')]})
env.update(XDG_SESSION_TYPE='x11', GTK_MODULES='gail:atk-bridge', NO_AT_BRIDGE='0',
           LIBGL_ALWAYS_SOFTWARE='1', ICU_BRAIN='ollama')
check_empty = os.environ.get('ICU_SCOPE_EMPTY') == '1'
if check_empty: env['ICU_SCOPE_EMPTY'] = '1'
report['check_empty_window'] = check_empty
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
    if check_empty:
        driver_env = dict(part.decode().split('=', 1) for part in Path(f'/proc/{meta["pid"]}/environ').read_bytes().split(b'\0') if b'=' in part)
        trap = shutil.which('wish', path=driver_env.get('PATH', ''))
        assert trap == '/opt/scope-traps/wish', 'driver cannot reach the owned wish trap'
        assert Path(trap).read_bytes() == Path('/opt/tests/wish_trap.py').read_bytes(), 'driver wish differs from the pinned trap'
        assert driver_env.get('SCOPE_TRAP_OUTPUT') == str(work / 'wish-called.json'), 'driver trap output is not bound to this probe'
        control_path = work / 'wish-readiness.json'
        control_env = dict(env, PATH=driver_env['PATH'], SCOPE_TRAP_OUTPUT=str(control_path))
        control = subprocess.run([trap], input='readiness-only', text=True, env=control_env, capture_output=True, timeout=3)
        assert control.returncode == 1 and json.loads(control_path.read_text())['script'] == 'readiness-only'
        assert not (work / 'wish-called.json').exists()
        report['wish_trap_ready'] = True
    raw = protocol({'method':'call', 'name':'list_windows', 'args':{}, 'session_id':'icu-window-scope-72', 'client_kind':'cli'})
    (work / 'windows.json').write_text(json.dumps(raw, indent=2))
    windows = raw.get('structuredContent', raw)['windows']
    app_window = next(w['window_id'] for w in windows if w.get('pid') == app.pid and w.get('title') == 'Copilot')
    receiver_windows = {label: next(w['window_id'] for w in windows if w.get('pid') == receiver.pid and w.get('title') == 'ICU Scope ' + label) for label in ['Alpha','Beta']}
    assert len(set(receiver_windows.values())) == 2
    if check_empty:
        receiver_windows['Empty'] = next(w['window_id'] for w in windows if w.get('pid') == receiver.pid and w.get('title') == 'ICU Scope Empty')
        assert len(set(receiver_windows.values())) == 3
    report['receiver_windows'] = receiver_windows
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
    xdo('windowmove', '--sync', str(app_window), '0', '170')
    xdo('windowactivate', '--sync', str(app_window)); time.sleep(.5)
    report['initial_receiver'] = receiver_state()
    assert {k:v['text'] for k,v in report['initial_receiver'].items()} == {'Alpha':'alpha','Beta':'beta'}
    for label,wid in receiver_windows.items():
        value = protocol({'method':'call','name':'get_window_state','args':{'pid':receiver.pid,'window_id':wid,'include_screenshot':False},'session_id':'icu-window-scope-72','client_kind':'cli'})
        (work / ('initial-'+label+'.json')).write_text(json.dumps(value,indent=2))
        tree = value.get('structuredContent',value)
        labels = [e.get('label') for e in tree.get('elements',[])]
        assert (label == 'Empty' or 'Field '+label in labels) and all('Field '+other not in labels for other in receiver_windows if other != label), 'observation did not isolate the requested window'
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
    report['stage'] = 'window_scoped_text'
    report['cases'] = []
    for label,wid in receiver_windows.items():
        before = receiver_state(); text = '|'+label+'|'
        args = {'session':'icu-window-scope-72','pid':receiver.pid,'window_id':wid,'text':text,'delivery_mode':'background','_skip_window_change_detection':True}
        response = protocol({'method':'call','name':'type_text','args':args,'session_id':'icu-window-scope-72','client_kind':'cli'})
        time.sleep(.25); after = receiver_state()
        changed = [name for name in before if before[name]['text'] != after[name]['text']]
        correct = (response.get('isError') is True and not changed) if label == 'Empty' else (after[label]['text'] == before[label]['text'] + text and changed == [label])
        report['cases'].append({'target':label,'args':args,'response':response,'before':before,'after':after,'changed_windows':changed,'correct':correct})
    time.sleep(1); snapshot('completed.png')
    monitor_stop.set(); monitor.join(2)
    assert samples and not monitor_errors
    assert all(all(s[k] == baseline[k] for k in ['active','focus','pointer']) for s in samples), 'native typing changed ownership'
    events = [json.loads(line) for line in (work / 'receiver-events.jsonl').read_text().splitlines()]
    report['receiver_events'] = events
    assert events and all(not item['active'] for event in events for item in event['state'].values()), 'a receiver became active during a callback'
    report['observation_scope_verified'] = True
    report['wrong_window_write'] = any(any(changed != case['target'] for changed in case['changed_windows']) for case in report['cases'])
    report['unbound_tk_fallback_attempted'] = (work / 'wish-called.json').exists()
    assert not report['unbound_tk_fallback_attempted'], 'window input reached the unbound Tk fallback'
    assert all(case['correct'] for case in report['cases']), 'text did not stay in the exact requested window'
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
