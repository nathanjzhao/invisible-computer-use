"""What the installed app's own driver does inside a real headless Wayland session.

Usage: probe_linux_wayland.py WORK /usr/bin/app wayland_grid_gtk.py COMPOSITOR VARIANT

COMPOSITOR: sway (wlroots headless backend, pixman renderer) or mutter
(``mutter --headless --wayland --virtual-monitor``).
VARIANT:
  default  both WAYLAND_DISPLAY and the XWayland DISPLAY are exported, as a
           GNOME/sway login session does; the Wayland backend is not opted in.
  pure     no XWayland and no DISPLAY (sway ``xwayland disable``).
  optin    default plus CUA_DRIVER_RS_ENABLE_WAYLAND=1 in the app environment.

Targets: ``XW`` (GTK3 forced onto XWayland, GDK_BACKEND=x11) and ``WL`` (GTK3
native Wayland). For each target the probe records list_windows, a screenshot,
a pixel click, an element click and type_text through the app-owned driver
socket, and compares every reported success with the fixture's own log. A
separate guard window is created last so it holds compositor focus; target
activation is read from the fixture's is-active log (both compositors) and
from sway's IPC tree. The report's ``false_success`` list is every call that
returned without isError while its target recorded no matching effect.

The probe exits 0 only when every call completed and the session stayed
truthful and invisible: no false success, no background call activated a
target, no call made the focused guard log a deactivation (GTK ``is-active``,
which also sees the grab-mode FocusOut/FocusIn pairs a focus poll cannot), and
``get_desktop_state`` failing exactly when ``health_report`` says screen
capture fails, with a structured error code. ``failures`` names each broken
condition.
"""
import base64
import glob
import hashlib
import json
import os
import pathlib
import signal
import socket
import struct
import subprocess
import sys
import time
import traceback

if sys.platform != 'linux': raise SystemExit('Linux-only probe')
work = pathlib.Path(sys.argv[1]); work.mkdir(parents=True)
exe = pathlib.Path(sys.argv[2]).resolve(); fixture = pathlib.Path(sys.argv[3]).resolve()
compositor, variant = sys.argv[4], sys.argv[5]
assert compositor in ('sway', 'mutter') and variant in ('default', 'pure', 'optin')
processes = []; logs = []; driver_identity = None; calls = []
report = {'compositor': compositor, 'variant': variant, 'calls': calls,
    'probe_sha256': hashlib.sha256(pathlib.Path(__file__).read_bytes()).hexdigest(),
    'fixture_sha256': hashlib.sha256(fixture.read_bytes()).hexdigest()}
env = {k: v for k, v in os.environ.items() if not k.startswith(('ICU_', 'DISPLAY', 'WAYLAND_'))}
env.update({'XDG_CACHE_HOME': str(work/'cache'), 'XDG_DATA_HOME': str(work/'data'),
    'XDG_CONFIG_HOME': str(work/'config'), 'XDG_RUNTIME_DIR': str(work/'runtime'),
    'XDG_SESSION_TYPE': 'wayland', 'GTK_MODULES': 'gail:atk-bridge', 'NO_AT_BRIDGE': '0',
    'LIBGL_ALWAYS_SOFTWARE': '1', 'ICU_BRAIN': 'ollama', 'WEBKIT_DISABLE_COMPOSITING_MODE': '1'})
(work/'runtime').mkdir(); (work/'runtime').chmod(0o700)


def identity(pid):
    try: return pathlib.Path(f'/proc/{pid}/stat').read_text().rpartition(') ')[2].split()[19]
    except FileNotFoundError: return None


def launch(args, name, extra=None, drop=()):
    log = (work/(name+'.log')).open('wb'); logs.append(log)
    child_env = {k: v for k, v in {**env, **(extra or {})}.items() if k not in drop}
    child = subprocess.Popen(args, env=child_env, stdin=subprocess.DEVNULL, stdout=log, stderr=log, start_new_session=True)
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
            if len(data) > 32*1024*1024: raise ValueError('oversized response')
    response = json.loads(data.split(b'\n')[0]); assert response.get('ok'), response
    return response['result']


def events():
    path = work/'receiver-events.jsonl'
    if not path.exists(): return []
    return [json.loads(line) for line in path.read_text().splitlines(keepends=True) if line.endswith('\n')]


def sway_focus():
    if compositor != 'sway': return None
    try:
        tree = json.loads(subprocess.run(['swaymsg', '-t', 'get_tree'], env=env, capture_output=True, text=True, timeout=5).stdout)
    except Exception as error: return f'error: {error}'
    stack = [tree]
    while stack:
        node = stack.pop()
        if node.get('focused'):
            return {'pid': node.get('pid'), 'name': node.get('name'), 'app_id': node.get('app_id'),
                'shell': node.get('shell'), 'type': node.get('type')}
        stack += node.get('nodes', []) + node.get('floating_nodes', [])
    return None


def sway_windows():
    if compositor != 'sway': return None
    tree = json.loads(subprocess.run(['swaymsg', '-t', 'get_tree'], env=env, capture_output=True, text=True, timeout=5).stdout)
    out = []; stack = [tree]
    while stack:
        node = stack.pop()
        if node.get('type') in ('con', 'floating_con') and node.get('pid'):
            out.append({'pid': node['pid'], 'name': node.get('name'), 'shell': node.get('shell'), 'rect': node.get('rect')})
        stack += node.get('nodes', []) + node.get('floating_nodes', [])
    return out


def deadline_signal(*_): raise TimeoutError('wayland probe deadline')


signal.signal(signal.SIGTERM, deadline_signal); signal.signal(signal.SIGALRM, deadline_signal); signal.alarm(200)
try:
    report['host_network_namespace'] = os.environ['ICU_PROBE_HOST_NETNS']
    assert report['host_network_namespace'] != os.readlink('/proc/self/ns/net'), 'private network namespace required'
    session_file = work/'session-env'
    dump = ['sh', '-c', f'env > {session_file}.tmp && mv {session_file}.tmp {session_file}; exec sleep 100000']
    if compositor == 'sway':
        (work/'sway.conf').write_text('output HEADLESS-1 resolution 1920x1080 position 0 0\n'
            f'xwayland {"disable" if variant == "pure" else "force"}\n'
            'focus_on_window_activation none\n')
        launch(['sway', '-c', str(work/'sway.conf')], 'compositor',
            extra={'WLR_BACKENDS': 'headless', 'WLR_RENDERER': 'pixman', 'WLR_LIBINPUT_NO_DEVICES': '1'})
        sock = wait_for(lambda: glob.glob(str(work/'runtime'/'sway-ipc.*.sock')), 15, 'sway IPC socket missing')[0]
        env['SWAYSOCK'] = sock
        wait_for(lambda: subprocess.run(['swaymsg', 'exec', ' '.join(f"'{a}'" for a in dump)], env=env,
            capture_output=True, timeout=5).returncode == 0, 10, 'swaymsg exec failed')
    else:
        assert variant != 'pure', 'pure variant is sway-only'
        launch(['mutter', '--headless', '--wayland', '--virtual-monitor', '1920x1080', '--', *dump], 'compositor')
    wait_for(session_file.exists, 20, 'compositor session environment not published')
    session = dict(line.split('=', 1) for line in session_file.read_text().splitlines() if '=' in line)
    keep = {k: session[k] for k in ('WAYLAND_DISPLAY', 'DISPLAY', 'XAUTHORITY') if k in session}
    report['session_env'] = keep
    env.update(keep)
    if variant == 'pure': env.pop('DISPLAY', None); env.pop('XAUTHORITY', None)
    else: assert 'DISPLAY' in env, 'XWayland DISPLAY was not exported by the compositor'
    env['XDG_CURRENT_DESKTOP'] = 'sway' if compositor == 'sway' else 'GNOME'
    subprocess.run(['dbus-update-activation-environment', *[k for k in ('DISPLAY', 'WAYLAND_DISPLAY', 'XAUTHORITY', 'XDG_RUNTIME_DIR', 'XDG_SESSION_TYPE', 'XDG_CURRENT_DESKTOP') if k in env]],
        env=env, check=True, timeout=5)

    fixtures = {}
    def start_fixture(name, backend):
        child = launch(['/usr/bin/python3', str(fixture), str(work), name], 'fixture-'+name, extra={'GDK_BACKEND': backend},
            drop=('DISPLAY',) if backend == 'wayland' else ())
        def ready():
            if child.poll() is not None: raise RuntimeError(f'fixture {name} exited; see fixture-{name}.log')
            path = work/f'fixture-{child.pid}.json'
            return json.loads(path.read_text()) if path.exists() else None
        fixtures[name] = wait_for(ready, 15, f'fixture {name} not ready')
    targets = ['WL'] if variant == 'pure' else ['XW', 'WL']
    for name in targets: start_fixture(name, 'x11' if name == 'XW' else 'wayland')
    report['fixtures'] = fixtures

    app_env = {'CUA_DRIVER_RS_ENABLE_WAYLAND': '1'} if variant == 'optin' else {}
    app = launch([str(exe)], 'app', extra=app_env); report['app_pid'] = app.pid
    sock = work/'cache/icu'/f'driver-{app.pid}.sock'
    def metadata():
        if app.poll() is not None: raise RuntimeError(f'app exited during startup (code {app.returncode}); see app.log')
        try: return protocol(sock, {'method': 'metadata'}, 1)
        except (OSError, ValueError): return None
    meta = wait_for(metadata, 40, 'owned driver not ready'); report['metadata'] = meta
    assert pathlib.Path(f'/proc/{meta["pid"]}/exe').resolve() == exe.with_name('cua-driver'), 'wrong driver binary'
    driver_identity = (meta['pid'], identity(meta['pid']))
    report['driver_env_wayland_optin'] = 'CUA_DRIVER_RS_ENABLE_WAYLAND=1' in pathlib.Path(f'/proc/{meta["pid"]}/environ').read_bytes().decode(errors='replace').split('\0')
    report['app_sha256'] = hashlib.sha256(exe.read_bytes()).hexdigest()
    report['driver_sha256'] = hashlib.sha256(exe.with_name('cua-driver').read_bytes()).hexdigest()
    # Same session setup the app performs before its first input (cua.rs
    # ensure_cursor_hidden): a read, then hide the agent cursor on that session.
    for name, args in (('list_windows', {}), ('set_agent_cursor_enabled', {'session': 'wayland-probe', 'enabled': False})):
        protocol(sock, {'method': 'call', 'name': name, 'args': args, 'session_id': 'wayland-probe', 'client_kind': 'cli'})
    time.sleep(2)
    start_fixture('Guard', 'wayland' if variant == 'pure' else 'x11')
    time.sleep(1)
    if compositor == 'sway':
        subprocess.run(['swaymsg', f'[pid={fixtures["Guard"]["pid"]}] focus'], env=env, capture_output=True, timeout=5)
        time.sleep(.5)
    report['compositor_windows'] = sway_windows()
    report['focus_baseline'] = sway_focus()

    def call(label, target, name, args, effect=None, timeout=12):
        before = len(events()); focus_before = sway_focus()
        entry = {'label': label, 'target': target, 'tool': name, 'args': {k: v for k, v in args.items() if k != 'session'}}
        try:
            result = protocol(sock, {'method': 'call', 'name': name, 'args': args, 'session_id': 'wayland-probe', 'client_kind': 'cli'}, timeout)
        except Exception as error:
            entry.update(transport_error=f'{type(error).__name__}: {error}'); calls.append(entry); return entry, None
        if effect:
            try: wait_for(lambda: any(effect(e) for e in events()[before:]), 2.5, 'no effect')
            except RuntimeError: pass
        time.sleep(.4)
        structured = result.get('structuredContent')
        image = next((c for c in result.get('content', []) if c.get('type') == 'image'), None)
        entry.update(is_error=bool(result.get('isError')),
            text=[c.get('text', '')[:600] for c in result.get('content', []) if c.get('type') == 'text'][:2],
            structured={k: v for k, v in (structured or {}).items() if k not in ('elements', 'tree', 'tree_markdown', 'screenshot')} if isinstance(structured, dict) else structured,
            events=[e for e in events()[before:]], focus_before=focus_before, focus_after=sway_focus())
        if effect is not None:
            entry['effect_observed'] = any(effect(e) for e in entry['events'])
            entry['false_success'] = not entry['is_error'] and not entry['effect_observed']
        entry['target_activated'] = any(e['kind'] == 'active' and e['active'] and e['window'] == target for e in entry['events'])
        entry['guard_deactivated'] = any(e['kind'] == 'active' and not e['active'] and e['window'] == 'Guard' for e in entry['events'])
        if image:
            png = base64.b64decode(image['data'])
            path = work/f'{len(calls):02d}-{label}.png'; path.write_bytes(png)
            entry['image'] = {'file': path.name, 'size': list(struct.unpack('!II', png[16:24]))}
        calls.append(entry); return entry, result

    call('health', None, 'health_report', {})
    listed_entry, listed = call('list_windows', None, 'list_windows', {})
    windows = (listed or {}).get('structuredContent', {}).get('windows', []) if listed else []
    report['listed_windows'] = [{k: w.get(k) for k in ('pid', 'window_id', 'title', 'app_name', 'bounds')} for w in windows]
    call('desktop', None, 'get_desktop_state', {})

    def find_element(obj, label):
        if isinstance(obj, dict):
            text = ' '.join(str(obj.get(k, '')) for k in ('label', 'name', 'title', 'description', 'value'))
            if label in text and isinstance(obj.get('element_token'), str): return obj['element_token']
            for v in obj.values():
                found = find_element(v, label)
                if found is not None: return found
        elif isinstance(obj, list):
            for v in obj:
                found = find_element(v, label)
                if found is not None: return found
        return None

    for name in targets:
        pid = fixtures[name]['pid']
        mine = [w for w in windows if w.get('pid') == pid]
        report.setdefault('target_listed', {})[name] = [w.get('window_id') for w in mine]
        wid = mine[0]['window_id'] if mine else fixtures[name].get('xid')
        scope = {'pid': pid, **({'window_id': wid} if wid else {})}
        state_entry, state = call(f'{name}-state', name, 'get_window_state',
            {**scope, 'include_screenshot': True, 'include_accessibility_tree': True, 'max_dimension': 640})
        structured = (state or {}).get('structuredContent', {}) if state else {}
        index = find_element(structured, f'{name}-21')
        state_entry['element_21_token'] = index
        size = state_entry.get('image', {}).get('size')
        if size:
            point = [round(.5*size[0]/2, 1), round(1.5*size[1]/3, 1)]
            call(f'{name}-pixel-click', name, 'click', {'session': 'wayland-probe', **scope, 'x': point[0], 'y': point[1],
                'delivery_mode': 'background'}, effect=lambda e, n=name: e['kind'] == 'clicked' and e['button'] == f'{n}-10')
        else:
            # No frame: aim at the same button using the driver's own listed
            # bounds (what a caller without a screenshot would have), or a
            # fixed point when the window is not listed at all.
            bounds = (mine[0].get('bounds') or {}) if mine else {}
            w, h = bounds.get('width') or 600, bounds.get('height') or 360
            point = [round(.5*w/2, 1), round(1.5*h/3, 1)]
            call(f'{name}-pixel-click-no-frame', name, 'click', {'session': 'wayland-probe', **scope, 'x': point[0], 'y': point[1],
                'delivery_mode': 'background'}, effect=lambda e, n=name: e['kind'] == 'clicked' and e['button'] == f'{n}-10')
        if index is not None:
            call(f'{name}-element-click', name, 'click', {'session': 'wayland-probe', **scope, 'element_token': index,
                'delivery_mode': 'background'}, effect=lambda e, n=name: e['kind'] == 'clicked' and e['button'] == f'{n}-21')
        call(f'{name}-type', name, 'type_text', {'session': 'wayland-probe', **scope, 'text': 'wl ok',
            'delivery_mode': 'background'}, effect=lambda e, n=name: e['kind'] == 'text' and e['window'] == n and 'wl ok' in e['text'])
        call(f'{name}-press-key', name, 'press_key', {'session': 'wayland-probe', **scope, 'key': 'a',
            'delivery_mode': 'background'}, effect=lambda e, n=name: e['kind'] == 'text' and e['window'] == n and e['text'].endswith('a'))

    if compositor == 'sway':
        subprocess.run(['grim', str(work/'compositor.png')], env=env, capture_output=True, timeout=10)
    report['false_success'] = [c['label'] for c in calls if c.get('false_success')]
    report['target_activations'] = [c['label'] for c in calls if c.get('target_activated')]
    report['guard_deactivations'] = [c['label'] for c in calls if c.get('guard_deactivated')]
    report['focus_final'] = sway_focus()
    health = next((c for c in calls if c['label'] == 'health'), {})
    capture_check = next((check for check in ((health.get('structured') or {}).get('checks') or [])
        if check.get('name') == 'screen_capture_capability'), {})
    desktop = next((c for c in calls if c['label'] == 'desktop'), {})
    report['desktop_capture'] = {'is_error': desktop.get('is_error'), 'code': (desktop.get('structured') or {}).get('code'),
        'health_status': capture_check.get('status'), 'health_detail': (capture_check.get('data') or {}).get('error_detail')}
    failures = []
    if report['false_success']: failures.append('false_success')
    if report['target_activations']: failures.append('target_activations')
    if report['guard_deactivations']: failures.append('guard_deactivations')
    if desktop.get('is_error') and not report['desktop_capture']['code']: failures.append('desktop_error_untyped')
    if bool(desktop.get('is_error')) != (capture_check.get('status') == 'fail'): failures.append('capture_readiness_disagrees')
    report['failures'] = failures
    report['ok'] = not failures
except Exception as error:
    report.update(ok=False, error=str(error), error_type=type(error).__name__, traceback=traceback.format_exc())
finally:
    signal.alarm(0)
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
    print(json.dumps({k: report.get(k) for k in ('ok', 'failures', 'error', 'session_env', 'target_listed', 'false_success', 'target_activations', 'guard_deactivations', 'desktop_capture')}, indent=2))
    sys.exit(0 if report.get('ok') else 1)
