"""Typed X11 capture refusals through the installed app's own driver and its window tiles.

Usage: probe_linux_capture_refusals.py WORK /usr/bin/app display_grid_gtk.py COMPOSITOR

COMPOSITOR is ``none`` (openbox only) or ``picom``. One Xvfb root (1600x1000)
holds two GTK3 grid windows, Min and Off, and a separate guard window that owns
focus. The probe opens tiles for Min and Off through the app's own picker (the
webview's accessibility tree, inside this private X server), lets each tile
show a live frame, then minimizes Min and moves Off to x = -300, partly past
the root's left edge. It records what the driver returns for each window, with
and without the accessibility tree, and what each tile shows.

Expected with the fix: a minimized window returns screenshot_error.code
``window_minimized``; the off-root window returns ``window_offscreen`` without
a compositor and a real frame with picom; get_window_state with the tree keeps
the tree; each tile shows the refusal instead of its last frame. ``ok`` is
true only when all of that holds. Driver calls run with a 20 ms ownership
sampler and the guard's GTK is-active log.
"""
import base64
import hashlib
import json
import os
import pathlib
import select
import signal
import socket
import struct
import subprocess
import sys
import threading
import time
import traceback

if sys.platform != 'linux': raise SystemExit('Linux-only probe')
work = pathlib.Path(sys.argv[1]); work.mkdir(parents=True)
exe = pathlib.Path(sys.argv[2]).resolve(); fixture = pathlib.Path(sys.argv[3]).resolve()
compositor = sys.argv[4]; assert compositor in ('none', 'picom')
processes = []; logs = []; samples = []; calls = []
monitor_stop = threading.Event(); monitor = None; driver_identity = None
report = {'compositor': compositor, 'calls': calls,
    'probe_sha256': hashlib.sha256(pathlib.Path(__file__).read_bytes()).hexdigest(),
    'fixture_sha256': hashlib.sha256(fixture.read_bytes()).hexdigest()}
env = {k: v for k, v in os.environ.items() if not k.startswith(('ICU_', 'WAYLAND_'))}
env.update({'XDG_CACHE_HOME': str(work/'cache'), 'XDG_DATA_HOME': str(work/'data'),
    'XDG_CONFIG_HOME': str(work/'config'), 'XDG_RUNTIME_DIR': str(work/'runtime'),
    'XDG_SESSION_TYPE': 'x11', 'GTK_MODULES': 'gail:atk-bridge', 'NO_AT_BRIDGE': '0',
    'LIBGL_ALWAYS_SOFTWARE': '1', 'ICU_BRAIN': 'ollama', 'WEBKIT_DISABLE_COMPOSITING_MODE': '1'})
(work/'runtime').mkdir(); (work/'runtime').chmod(0o700)


def identity(pid):
    try: return pathlib.Path(f'/proc/{pid}/stat').read_text().rpartition(') ')[2].split()[19]
    except FileNotFoundError: return None


def launch(args, name, stdout=None, extra=None):
    log = (work/(name+'.log')).open('wb'); logs.append(log)
    child = subprocess.Popen(args, env={**env, **(extra or {})}, stdin=subprocess.DEVNULL,
        stdout=stdout or log, stderr=log, start_new_session=True)
    processes.append(child); return child


def wait_for(predicate, seconds, message):
    deadline = time.monotonic()+seconds
    while time.monotonic() < deadline:
        result = predicate()
        if result: return result
        time.sleep(.05)
    raise RuntimeError(message)


def protocol(path, request, timeout=10):
    deadline = time.monotonic()+timeout
    with socket.socket(socket.AF_UNIX) as connection:
        connection.settimeout(timeout); connection.connect(str(path))
        connection.sendall((json.dumps(request)+'\n').encode()); data = b''
        while b'\n' not in data:
            remaining = deadline-time.monotonic()
            if remaining <= 0: raise TimeoutError('protocol deadline')
            connection.settimeout(remaining); chunk = connection.recv(65536)
            if not chunk: raise ConnectionError('peer closed')
            data += chunk
    response = json.loads(data.split(b'\n')[0]); assert response.get('ok'), response
    return response['result']


def events():
    path = work/'receiver-events.jsonl'
    if not path.exists(): return []
    return [json.loads(line) for line in path.read_text().splitlines(keepends=True) if line.endswith('\n')]


def run(*args, **kw):
    return subprocess.run(args, env=env, check=True, capture_output=True, text=True, timeout=10, **kw).stdout


def deadline_signal(*_): raise TimeoutError('capture refusal probe deadline')


signal.signal(signal.SIGTERM, deadline_signal); signal.signal(signal.SIGALRM, deadline_signal); signal.alarm(200)
try:
    report['host_network_namespace'] = os.environ['ICU_PROBE_HOST_NETNS']
    assert report['host_network_namespace'] != os.readlink('/proc/self/ns/net'), 'private network namespace required'
    xvfb = launch(['Xvfb', '-displayfd', '1', '-screen', '0', '1600x1000x24', '-nolisten', 'tcp'], 'xvfb', subprocess.PIPE)
    assert select.select([xvfb.stdout], [], [], 15)[0], 'Xvfb startup timeout'
    env['DISPLAY'] = ':'+xvfb.stdout.readline().decode().strip()
    subprocess.run(['dbus-update-activation-environment', 'DISPLAY', 'XDG_RUNTIME_DIR', 'XDG_DATA_HOME', 'XDG_CONFIG_HOME'], env=env, check=True, timeout=5)
    launch(['openbox'], 'openbox')
    if compositor == 'picom': launch(['picom', '--backend', 'xrender'], 'picom')
    time.sleep(.5)
    grid = launch(['/usr/bin/python3', str(fixture), str(work), 'Min:80:80:520:300', 'Off:700:80:520:300'], 'grid')
    guard = launch(['/usr/bin/python3', str(fixture), str(work), 'Guard:1150:650:360:240'], 'guard')

    def fixture_ready(child):
        def check():
            if child.poll() is not None: raise RuntimeError(f'fixture {child.pid} exited')
            path = work/f'fixture-{child.pid}.json'
            return json.loads(path.read_text()) if path.exists() else None
        return wait_for(check, 10, f'fixture {child.pid} not ready')
    fx = {'grid': fixture_ready(grid), 'guard': fixture_ready(guard)}; report['fixtures'] = fx
    windows = {n: (fx['grid']['pid'], x) for n, x in fx['grid']['windows'].items()}
    guard_xid = fx['guard']['windows']['Guard']

    app = launch([str(exe)], 'app'); report['app_pid'] = app.pid
    sock = work/'cache/icu'/f'driver-{app.pid}.sock'
    def metadata():
        if app.poll() is not None: raise RuntimeError('app exited during startup')
        try: return protocol(sock, {'method': 'metadata'}, 1)
        except (OSError, ValueError): return None
    meta = wait_for(metadata, 40, 'owned driver not ready'); report['metadata'] = meta
    assert pathlib.Path(f'/proc/{meta["pid"]}/exe').resolve() == exe.with_name('cua-driver'), 'wrong driver binary'
    driver_identity = (meta['pid'], identity(meta['pid']))
    report['driver_sha256'] = hashlib.sha256(exe.with_name('cua-driver').read_bytes()).hexdigest()
    report['app_sha256'] = hashlib.sha256(exe.read_bytes()).hexdigest()

    def raw_call(name, args, timeout=10):
        return protocol(sock, {'method': 'call', 'name': name, 'args': args, 'session_id': 'refusal-probe', 'client_kind': 'cli'}, timeout)
    raw_call('set_agent_cursor_enabled', {'session': 'refusal-probe', 'enabled': False})

    # The app's picker, driven through the webview's own accessibility tree.
    import pyatspi
    def walk(node, depth=0):
        yield node
        if depth < 40:
            for child in node:
                if child is not None: yield from walk(child, depth+1)
    def app_rows():
        rows = []
        for application in pyatspi.Registry.getDesktop(0):
            try:
                if application is None or application.get_process_id() != app.pid: continue
            except Exception: continue
            for node in walk(application):
                try:
                    name = node.name or ''
                    try: text = node.queryText().getText(0, -1)
                    except Exception: text = ''
                    rows.append((node, node.getRoleName(), name, text))
                except Exception: pass
        return rows
    def press(predicate, message):
        node = wait_for(lambda: next((n for n, role, name, text in app_rows() if predicate(role, name, text)), None), 20, message)
        node.queryAction().doAction(0)
    def add_tile(title):
        press(lambda role, name, text: role in ('push button', 'button') and 'window' in name and 'ICU' not in name, 'picker button not exposed')
        press(lambda role, name, text: role in ('push button', 'button') and title in (name + text), f'picker row {title} not exposed')
        time.sleep(.5)
    report['tiles'] = tiles = {}
    add_tile('ICU Display Min'); add_tile('ICU Display Off')
    time.sleep(3)  # let both tiles load a live frame

    subprocess.run(['xdotool', 'windowactivate', '--sync', str(guard_xid), 'mousemove', '25', '25'], env=env, check=True, timeout=4)
    os.environ.update(env)
    from Xlib import X, display
    connection = display.Display(env['DISPLAY']); root = connection.screen().root
    active_atom = connection.intern_atom('_NET_ACTIVE_WINDOW')
    def sample(conn, rt):
        prop = rt.get_full_property(active_atom, X.AnyPropertyType); pointer = rt.query_pointer(); focus = conn.get_input_focus().focus
        return {'active': int(prop.value[0]) if prop is not None else 0, 'focus': getattr(focus, 'id', focus), 'pointer': [pointer.root_x, pointer.root_y]}
    wait_for(lambda: sample(connection, root)['active'] == guard_xid, 3, 'guard not active')
    baseline = sample(connection, root); report['baseline'] = baseline
    def monitor_ownership():
        observer = display.Display(env['DISPLAY']); observed_root = observer.screen().root
        try:
            while not monitor_stop.is_set():
                samples.append(sample(observer, observed_root)); monitor_stop.wait(.02)
        finally: observer.close()

    def state(label, name, tree):
        pid, xid = windows[name]
        before = len(events())
        result = raw_call('get_window_state', {'pid': pid, 'window_id': xid, 'include_screenshot': True,
            'include_accessibility_tree': tree, 'max_dimension': 640})
        time.sleep(.2)
        structured = result.get('structuredContent') or {}
        image = next((c for c in result.get('content', []) if c.get('type') == 'image'), None)
        entry = {'label': label, 'window': name, 'tree': tree, 'is_error': bool(result.get('isError')),
            'text': [c.get('text', '')[:700] for c in result.get('content', []) if c.get('type') == 'text'][:1],
            'screenshot_error': structured.get('screenshot_error'),
            'element_count': structured.get('element_count'),
            'guard_deactivated': any(e['kind'] == 'active' and e['window'] == 'Guard' and not e['value'] for e in events()[before:])}
        if image:
            png = base64.b64decode(image['data']); path = work/f'{len(calls):02d}-{label}.png'; path.write_bytes(png)
            entry['image'] = {'file': path.name, 'size': list(struct.unpack('!II', png[16:24]))}
        calls.append(entry); return entry

    monitor = threading.Thread(target=monitor_ownership, daemon=True); monitor.start()
    report['sampling_started'] = time.monotonic()
    visible = state('min-visible', 'Min', False)
    run('xdotool', 'windowminimize', '--sync', str(windows['Min'][1])); time.sleep(.6)
    report['min_xwininfo'] = run('xwininfo', '-id', str(windows['Min'][1])).split('Map State:')[1].split('\n')[0].strip()
    minimized = state('min-minimized', 'Min', False)
    minimized_tree = state('min-minimized-tree', 'Min', True)
    run('xdotool', 'windowmove', '--sync', str(windows['Off'][1]), '-300', '80'); time.sleep(.6)
    report['off_xwininfo'] = [l.strip() for l in run('xwininfo', '-id', str(windows['Off'][1])).splitlines() if 'Absolute upper-left X' in l or 'Map State' in l]
    offscreen = state('off-offscreen', 'Off', False)
    offscreen_tree = state('off-offscreen-tree', 'Off', True)
    monitor_stop.set(); monitor.join(2)
    report['sampling_stopped'] = time.monotonic()

    time.sleep(3)  # tiles poll roughly every second
    rows = app_rows()
    tiles['texts'] = sorted({(name or text).strip()[:200] for _, role, name, text in rows
        if any(k in (name + text) for k in ('capture', 'minimized', 'off screen', 'window'))})
    tiles['min_refusal_shown'] = any('(window_minimized)' in t for t in tiles['texts'])
    tiles['off_refusal_shown'] = any('(window_offscreen)' in t for t in tiles['texts'])
    app_window = next(w['window_id'] for w in raw_call('list_windows', {})['structuredContent']['windows'] if w.get('pid') == app.pid)
    shot = raw_call('get_window_state', {'pid': app.pid, 'window_id': app_window, 'include_screenshot': True, 'include_accessibility_tree': False})
    image = next((c for c in shot.get('content', []) if c.get('type') == 'image'), None)
    if image: (work/'app-tiles.png').write_bytes(base64.b64decode(image['data']))

    code = lambda entry: (entry.get('screenshot_error') or {}).get('code')
    report['guard_focus_losses'] = [e for e in events() if e['kind'] == 'active' and e['window'] == 'Guard'
        and not e['value'] and report['sampling_started'] <= e['time'] <= report['sampling_stopped']]
    report['ownership_unchanged'] = bool(samples) and all(s == baseline for s in samples) and not report['guard_focus_losses']
    expect_off = 'window_offscreen' if compositor == 'none' else None
    report['checks'] = checks = {
        'visible_has_frame': 'image' in visible and not visible['is_error'],
        'minimized_typed': minimized['is_error'] and 'image' not in minimized and code(minimized) == 'window_minimized',
        'minimized_tree_kept': not minimized_tree['is_error'] and (minimized_tree['element_count'] or 0) > 0 and code(minimized_tree) == 'window_minimized',
        'offscreen_expected': (code(offscreen) == expect_off and offscreen['is_error'] and 'image' not in offscreen) if expect_off
            else ('image' in offscreen and not offscreen['is_error'] and code(offscreen) is None),
        'offscreen_tree_kept': not offscreen_tree['is_error'] and (offscreen_tree['element_count'] or 0) > 0 and code(offscreen_tree) == expect_off,
        'no_backend_dump_in_codes': all('BadMatch' not in json.dumps({k: v for k, v in (c.get('screenshot_error') or {}).items() if k != 'detail'}) for c in calls),
        'min_tile_shows_refusal': tiles['min_refusal_shown'],
        'off_tile_expected': tiles['off_refusal_shown'] == bool(expect_off),
        'ownership_unchanged': report['ownership_unchanged'],
    }
    report['ok'] = all(checks.values())
except Exception as error:
    report.update(ok=False, error=str(error), error_type=type(error).__name__, traceback=traceback.format_exc())
finally:
    signal.alarm(0); monitor_stop.set()
    if monitor: monitor.join(2)
    report['samples'] = len(samples); report['changed_samples'] = [s for s in samples if s != report.get('baseline')][:20]
    report['events'] = events()
    for child in reversed(processes):
        if child.poll() is None:
            try: os.killpg(child.pid, signal.SIGTERM)
            except ProcessLookupError: pass
    if driver_identity and identity(driver_identity[0]) == driver_identity[1]:
        try: os.kill(driver_identity[0], signal.SIGKILL)
        except ProcessLookupError: pass
    for child in reversed(processes):
        try: child.wait(timeout=3)
        except subprocess.TimeoutExpired:
            os.killpg(child.pid, signal.SIGKILL); child.wait()
    for log in logs: log.close()
    report['owned_processes_reaped'] = all(child.poll() is not None for child in processes)
    (work/'result.json').write_text(json.dumps(report, indent=2)+'\n')
    print(json.dumps({k: report.get(k) for k in ('ok', 'error', 'checks', 'samples', 'guard_focus_losses', 'tiles')}, indent=2))
    sys.exit(0 if report.get('ok') else 1)
