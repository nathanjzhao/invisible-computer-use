"""Multi-monitor X11 capture/input agreement through the installed app's own driver.

Usage: probe_linux_displays.py WORK /usr/bin/app display_grid_gtk.py

A real Xorg server (dummy video driver) runs two RandR CRTCs: a 1280x1024
secondary monitor at the root origin and the 1920x1080 primary to its right at
+1280+0, so the primary does not start at (0,0) and part of the root (below the
shorter monitor) is on no monitor. X11 root coordinates cannot be negative;
the negative-origin case is a window moved partly past the left root edge.
One GTK process owns two identical grid windows (Left, Right) so a
window-local point is ambiguous unless the driver keeps the requested window.
A second process runs with GDK_SCALE=2 (X11 scale is per client, not per
monitor). Every case captures the requested window at the app's 640 px tile
size, clicks the centre of one button in that screenshot's pixels and checks
which button actually fired. A separate guard owns foreground/focus/pointer.
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
exe = pathlib.Path(sys.argv[2]).resolve()
fixture = pathlib.Path(sys.argv[3]).resolve()
MONITORS = [('DUMMY1', 1280, 1024, 0, 0), ('DUMMY0', 1920, 1080, 1280, 0)]
ROOT_W, ROOT_H = 3200, 1080
XORG_CONF = '''Section "Device"\n Identifier "d"\n Driver "dummy"\n VideoRam 256000\nEndSection\nSection "Screen"\n Identifier "s"\n Device "d"\n DefaultDepth 24\n SubSection "Display"\n  Depth 24\n  Virtual 3200 1200\n EndSubSection\nEndSection\n'''
MODES = {'1920x1080': '173.00 1920 2048 2248 2576 1080 1083 1088 1120 -hsync +vsync'.split(),
    '1280x1024': '109.00 1280 1368 1496 1712 1024 1027 1034 1063 -hsync +vsync'.split()}
processes = []; logs = []; samples = []; cases = []
monitor_stop = threading.Event(); monitor = None; driver_identity = None
report = {'source_commit': os.environ.get('ICU_TEST_APP_REVISION', 'unknown'),
    'probe_sha256': hashlib.sha256(pathlib.Path(__file__).read_bytes()).hexdigest(),
    'fixture_sha256': hashlib.sha256(fixture.read_bytes()).hexdigest(), 'cases': cases}
env = {k: v for k, v in os.environ.items() if not k.startswith('ICU_')}
env.update({'XDG_CACHE_HOME': str(work/'cache'), 'XDG_DATA_HOME': str(work/'data'),
    'XDG_CONFIG_HOME': str(work/'config'), 'XDG_RUNTIME_DIR': str(work/'runtime'),
    'XDG_SESSION_TYPE': 'x11', 'GTK_MODULES': 'gail:atk-bridge', 'NO_AT_BRIDGE': '0',
    'LIBGL_ALWAYS_SOFTWARE': '1', 'ICU_BRAIN': 'ollama'})
(work/'runtime').mkdir(); (work/'runtime').chmod(0o700)


def identity(pid):
    try: return pathlib.Path(f'/proc/{pid}/stat').read_text().rpartition(') ')[2].split()[19]
    except FileNotFoundError: return None


def launch(args, name, stdout=None, extra=None):
    log = (work/(name+'.log')).open('wb'); logs.append(log)
    child = subprocess.Popen(args, env={**env, **(extra or {})}, stdin=subprocess.DEVNULL,
        stdout=stdout if stdout is not None else log, stderr=log, start_new_session=True)
    processes.append(child); return child


def wait_for(predicate, seconds, message):
    deadline = time.monotonic()+seconds
    while time.monotonic() < deadline:
        result = predicate()
        if result: return result
        time.sleep(.02)
    raise RuntimeError(message)


def protocol(path, request, timeout=6):
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
            if len(data) > 16*1024*1024: raise ValueError('oversized probe response')
    response = json.loads(data.split(b'\n')[0]); assert response.get('ok'), response
    return response['result']


def events():
    path = work/'receiver-events.jsonl'
    if not path.exists(): return []
    return [json.loads(line) for line in path.read_text().splitlines(keepends=True) if line.endswith('\n')]


def run(*args, **kw):
    return subprocess.run(list(args), env=env, check=True, timeout=6, capture_output=True, text=True, **kw).stdout


def deadline_signal(*_): raise TimeoutError('display probe deadline')


signal.signal(signal.SIGTERM, deadline_signal); signal.signal(signal.SIGALRM, deadline_signal); signal.alarm(170)
try:
    report['host_network_namespace'] = os.environ['ICU_PROBE_HOST_NETNS']
    report['test_network_namespace'] = os.readlink('/proc/self/ns/net')
    assert report['host_network_namespace'] != report['test_network_namespace'], 'private network namespace required'
    (work/'xorg.conf').write_text(XORG_CONF)
    xorg = launch(['Xorg', '-displayfd', '1', '-config', str(work/'xorg.conf'), '-noreset', '-nolisten', 'tcp',
        '-logfile', str(work/'xorg-server.log')], 'xorg', subprocess.PIPE)
    assert select.select([xorg.stdout], [], [], 15)[0], 'Xorg startup timeout'
    number = xorg.stdout.readline().decode().strip(); assert number.isdigit()
    env['DISPLAY'] = ':'+number; report['display'] = env['DISPLAY']
    for mode, timings in MODES.items(): run('xrandr', '--newmode', mode, *timings)
    layout = []
    for name, w, h, x, y in MONITORS:
        run('xrandr', '--addmode', name, f'{w}x{h}')
        layout += ['--output', name, '--mode', f'{w}x{h}', '--pos', f'{x}x{y}']
    run('xrandr', *layout, '--output', 'DUMMY0', '--primary')
    report['monitors'] = run('xrandr', '--listmonitors')
    listed = {line.split()[-1]: line.split()[2] for line in report['monitors'].splitlines()[1:]}
    assert all(n in listed and listed[n].startswith(f'{w}/') and listed[n].endswith(f'+{x}+{y}') for n, w, h, x, y in MONITORS), f'RandR layout not applied: {listed}'
    report['root'] = run('xdpyinfo').split('dimensions:')[1].split('pixels')[0].strip()
    assert report['root'] == f'{ROOT_W}x{ROOT_H}', report['root']
    subprocess.run(['dbus-update-activation-environment', 'DISPLAY', 'XDG_RUNTIME_DIR', 'XDG_DATA_HOME', 'XDG_CONFIG_HOME'], env=env, check=True, timeout=5)
    report['compositor'] = os.environ.get('ICU_DISPLAYS_COMPOSITOR', 'picom')
    launch(['openbox'], 'openbox')
    if report['compositor'] == 'picom': launch(['picom', '--backend', 'xrender'], 'picom')
    time.sleep(.5)
    grid = launch(['/usr/bin/python3', str(fixture), str(work), 'Left:100:80:900:400', 'Right:2100:150:900:400'], 'grid')
    scaled = launch(['/usr/bin/python3', str(fixture), str(work), 'Hi:2000:560:540:220'], 'scaled', extra={'GDK_SCALE': '2'})
    guard = launch(['/usr/bin/python3', str(fixture), str(work), 'Guard:1400:800:360:240'], 'guard')

    def fixture_ready(child):
        def check():
            if child.poll() is not None: raise RuntimeError(f'fixture {child.pid} exited')
            path = work/f'fixture-{child.pid}.json'
            return json.loads(path.read_text()) if path.exists() else None
        return wait_for(check, 10, f'fixture {child.pid} not ready')
    fx = {'grid': fixture_ready(grid), 'scaled': fixture_ready(scaled), 'guard': fixture_ready(guard)}
    report['fixtures'] = fx
    assert fx['scaled']['scale']['Hi'] == 2 and fx['grid']['scale']['Left'] == 1, 'fixture scale not applied'
    windows = {**{n: (fx['grid']['pid'], x) for n, x in fx['grid']['windows'].items()},
        'Hi': (fx['scaled']['pid'], fx['scaled']['windows']['Hi'])}
    guard_xid = fx['guard']['windows']['Guard']

    app = launch([str(exe)], 'app'); report['app_pid'] = app.pid
    sock = work/'cache/icu'/f'driver-{app.pid}.sock'

    def metadata():
        if app.poll() is not None: raise RuntimeError('app exited during startup')
        try: return protocol(sock, {'method': 'metadata'}, 1)
        except (OSError, ValueError): return None
    meta = wait_for(metadata, 30, 'owned driver not ready'); report['metadata'] = meta
    assert pathlib.Path(f'/proc/{meta["pid"]}/exe').resolve() == exe.with_name('cua-driver'), 'wrong driver binary'
    driver_identity = (meta['pid'], identity(meta['pid'])); assert driver_identity[1] is not None
    report['driver_sha256'] = hashlib.sha256(exe.with_name('cua-driver').read_bytes()).hexdigest()
    report['app_sha256'] = hashlib.sha256(exe.read_bytes()).hexdigest()

    def raw_call(name, args, timeout=10):
        return protocol(sock, {'method': 'call', 'name': name, 'args': args, 'session_id': 'display-probe', 'client_kind': 'cli'}, timeout)

    raw_call('set_agent_cursor_enabled', {'session': 'display-probe', 'enabled': False})
    os.environ.update(env)
    from Xlib import X, display
    connection = display.Display(env['DISPLAY']); root = connection.screen().root
    active_atom = connection.intern_atom('_NET_ACTIVE_WINDOW')
    subprocess.run(['xdotool', 'windowactivate', '--sync', str(guard_xid), 'mousemove', '25', '25'], env=env, check=True, timeout=4)

    def sample(conn, rt):
        prop = rt.get_full_property(active_atom, X.AnyPropertyType); pointer = rt.query_pointer(); focus = conn.get_input_focus().focus
        return {'active': int(prop.value[0]) if prop is not None else 0, 'focus': getattr(focus, 'id', focus),
            'pointer': [pointer.root_x, pointer.root_y]}
    wait_for(lambda: sample(connection, root)['active'] == guard_xid, 3, 'guard not active')
    baseline = sample(connection, root); report['baseline'] = baseline

    def monitor_ownership():
        observer = display.Display(env['DISPLAY']); observed_root = observer.screen().root
        try:
            while not monitor_stop.is_set():
                samples.append(sample(observer, observed_root)); monitor_stop.wait(.02)
        finally: observer.close()
    monitor = threading.Thread(target=monitor_ownership, daemon=True); monitor.start()
    report['sampling_started'] = time.monotonic()

    def geometry(name):
        pid, xid = windows[name]
        listed = [w for w in raw_call('list_windows', {}).get('structuredContent', {}).get('windows', [])
            if w.get('window_id') == xid]
        try: info = run('xwininfo', '-id', str(xid))
        except subprocess.CalledProcessError:
            return {'driver_bounds': listed[0].get('bounds') if listed else None, 'exists': False}
        abs_xy = [int(line.split(':')[1]) for line in info.splitlines() if 'Absolute upper-left' in line]
        size = [int(line.split(':')[1]) for line in info.splitlines() if line.strip().startswith(('Width:', 'Height:'))]
        state = 'IsViewable' in info
        return {'driver_bounds': listed[0].get('bounds') if listed else None, 'root_origin': abs_xy, 'size': size, 'viewable': state}

    def capture(name, max_dimension=640):
        pid, xid = windows[name]
        result = raw_call('get_window_state', {'pid': pid, 'window_id': xid, 'include_screenshot': True,
            'include_accessibility_tree': False, 'max_dimension': max_dimension})
        image = next((c for c in result.get('content', []) if c.get('type') == 'image'), None)
        outcome = {'is_error': bool(result.get('isError')), 'structured': result.get('structuredContent'),
            'text': [c.get('text') for c in result.get('content', []) if c.get('type') == 'text'][:2]}
        if image:
            png = base64.b64decode(image['data']); assert png[:8] == b'\x89PNG\r\n\x1a\n'
            path = work/f'capture-{len(cases):02d}-{name}.png'; path.write_bytes(png)
            outcome.update(size=list(struct.unpack('!II', png[16:24])), file=path.name)
        return outcome

    def click(name, point, wait=True):
        pid, xid = windows[name]; before = len(events())
        result = raw_call('click', {'session': 'display-probe', 'pid': pid, 'window_id': xid,
            'x': point[0], 'y': point[1], 'delivery_mode': 'background'})
        if wait:
            try: wait_for(lambda: any(e['kind'] == 'clicked' for e in events()[before:]), 2.5, 'no activation')
            except RuntimeError: pass
        time.sleep(.3)
        return {'is_error': bool(result.get('isError')), 'structured': result.get('structuredContent'),
            'text': [c.get('text') for c in result.get('content', []) if c.get('type') == 'text'][:2],
            'events': events()[before:]}

    def centre(size, row, col):
        return [round((col+.5)*size[0]/3, 1), round((row+.5)*size[1]/2, 1)]

    def activated(outcome):
        return [e['button'] for e in outcome['events'] if e['kind'] == 'clicked']

    def case(label, name, row, col, before=None, between=None, expect='hit'):
        """expect: 'hit' = exactly the requested button fires; 'refuse' = typed error and no
        activation; 'truthful' = either of those, never a success report without the effect."""
        entry = {'case': label, 'window': name, 'target': f'{name}-{row}{col}', 'expect': expect}
        cases.append(entry)
        try:
            if before: entry['before'] = before()
            entry['geometry'] = geometry(name)
            entry['capture'] = capture(name)
            size = entry['capture'].get('size')
            point = centre(size, row, col) if size else entry.get('point', [100, 100])
            entry['point'] = point
            if between: entry['between'] = between(); entry['geometry_at_click'] = geometry(name)
            entry['click'] = click(name, point, wait=expect == 'hit')
            fired = activated(entry['click'])
            if expect == 'hit':
                # Without a frame there is no screenshot point to agree with; the
                # click above used a fallback point and proves nothing.
                entry['ok'] = bool(size) and fired == [entry['target']] and not entry['click']['is_error']
                if not size: entry['failure'] = 'capture_failed'
            elif expect == 'refuse':
                entry['ok'] = entry['click']['is_error'] and not fired
            else:  # truthful: either a typed refusal with no effect or exactly the requested effect
                entry['ok'] = (entry['click']['is_error'] and not fired) or (not entry['click']['is_error'] and fired == [entry['target']])
            entry['fired'] = fired
        except Exception as error:
            entry.update(ok=False, error=f'{type(error).__name__}: {error}')
        return entry

    def move(name, x, y):
        run('xdotool', 'windowmove', '--sync', str(windows[name][1]), str(x), str(y)); time.sleep(.4)
        return {'moved_to': [x, y]}

    def resize(name, w, h):
        run('xdotool', 'windowsize', '--sync', str(windows[name][1]), str(w), str(h)); time.sleep(.5)
        return {'resized_to': [w, h]}

    case('left_monitor', 'Left', 1, 1)
    case('right_monitor_same_process', 'Right', 0, 2)
    case('right_monitor_other_button', 'Right', 1, 0)
    case('moved_between_capture_and_click', 'Right', 1, 0, between=lambda: move('Right', 250, 560))
    case('spanning_monitor_boundary', 'Right', 0, 2, before=lambda: move('Right', 900, 560))
    case('negative_origin_visible_part', 'Right', 0, 2, before=lambda: move('Right', -300, 560))
    case('negative_origin_offscreen_part', 'Right', 0, 0)
    case('scaled_client_gdk_scale_2', 'Hi', 1, 2)
    case('scaled_client_other_button', 'Hi', 0, 0)
    case('resized_larger', 'Left', 0, 1, before=lambda: resize('Left', 1100, 460))
    case('resized_smaller', 'Left', 1, 2, before=lambda: resize('Left', 600, 300))
    case('minimized', 'Left', 0, 0, before=lambda: (run('xdotool', 'windowminimize', '--sync', str(windows['Left'][1])), time.sleep(.5), {'minimized': True})[2],
        expect='truthful')
    case('closed', 'Right', 0, 1, before=lambda: (run('wmctrl', '-i', '-c', hex(windows['Right'][1])), time.sleep(.8), {'closed': True})[2],
        expect='refuse')

    desktop = raw_call('get_desktop_state', {'screenshot_out_file': str(work/'desktop.png')})
    report['desktop'] = {'is_error': bool(desktop.get('isError')), 'structured': desktop.get('structuredContent')}
    if (work/'desktop.png').exists():
        png = (work/'desktop.png').read_bytes(); report['desktop']['size'] = list(struct.unpack('!II', png[16:24]))
    # Ownership sampling covers every driver case; the picker check below is
    # deliberate in-app UI interaction inside this private X server.
    monitor_stop.set(); monitor.join(2)
    report['sampling_stopped'] = time.monotonic()
    report['guard_focus_losses'] = [e for e in events() if e['kind'] == 'active' and e['window'] == 'Guard'
        and not e['value'] and report['sampling_started'] <= e['time'] <= report['sampling_stopped']]
    report['ownership_unchanged'] = bool(samples) and all(s == baseline for s in samples) and not report['guard_focus_losses']
    report['picker'] = picker = {}
    try:
        # Open the app's own window picker (in this private X server) and read
        # the whole-screen tile's label from the webview's accessibility tree.
        import pyatspi
        def app_nodes():
            found = []
            for application in pyatspi.Registry.getDesktop(0):
                if application is None: continue
                try:
                    if application.get_process_id() == app.pid: found.append(application)
                except Exception: pass
            return found
        def walk(node, depth=0):
            yield node
            if depth < 40:
                for child in node:
                    if child is not None: yield from walk(child, depth+1)
        def texts():
            rows = []
            for application in app_nodes():
                for node in walk(application):
                    try: rows.append((node, node.getRoleName(), node.name or ''))
                    except Exception: pass
            return rows
        opener = wait_for(lambda: next((n for n, role, name in texts() if role in ('push button', 'button') and 'window' in name), None), 20, 'picker button not exposed')
        opener.queryAction().doAction(0)
        rows = wait_for(lambda: [r for r in texts() if 'display' in r[2].lower() or 'screen' in r[2].lower() or 'monitor' in r[2].lower()], 10, 'picker rows not exposed')
        picker['labels'] = sorted({name for _, _, name in rows})
        time.sleep(1.5)  # let the popover paint before the evidence capture
        whole_root = report['desktop'].get('size') == [ROOT_W, ROOT_H] and len(MONITORS) > 1
        claims_primary = any('main display' in l.lower() or 'primary' in l.lower() for l in picker['labels'])
        picker['desktop_spans_all_monitors'] = whole_root
        picker['ok'] = not (whole_root and claims_primary)
        shot = raw_call('get_window_state', {'pid': app.pid, 'window_id': next(w['window_id'] for w in raw_call('list_windows', {})['structuredContent']['windows'] if w.get('pid') == app.pid),
            'include_screenshot': True, 'include_accessibility_tree': False})
        image = next((c for c in shot.get('content', []) if c.get('type') == 'image'), None)
        if image: (work/'app-picker.png').write_bytes(base64.b64decode(image['data']))
    except Exception as error:
        picker.update(ok=False, error=f'{type(error).__name__}: {error}')
    health = raw_call('health_report', {}).get('structuredContent', {})
    report['health_host'] = next((c.get('data') for c in health.get('checks', []) if c.get('name') == 'host_interactive'), None)

    report['case_summary'] = {c['case']: c.get('ok') for c in cases}
    report['ok'] = report['ownership_unchanged'] and all(c.get('ok') for c in cases) and bool(report['picker'].get('ok'))
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
    report['owned_daemon_exited'] = not driver_identity or identity(driver_identity[0]) != driver_identity[1]
    (work/'result.json').write_text(json.dumps(report, indent=2)+'\n')
    print(json.dumps({k: report.get(k) for k in ('ok', 'error', 'case_summary', 'ownership_unchanged', 'samples', 'guard_focus_losses', 'desktop', 'picker')}, indent=2))
    sys.exit(0 if report.get('ok') else 1)
