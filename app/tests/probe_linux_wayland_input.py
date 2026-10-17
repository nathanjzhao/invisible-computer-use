"""Wayland readiness, picker notice, capture and keyboard routes through the installed app's driver.

Usage: probe_linux_wayland_input.py WORK /usr/bin/app wayland_grid_gtk.py COMPOSITOR VARIANT

COMPOSITOR: sway (wlroots headless) or mutter (--headless --wayland).
VARIANT:
  default  WAYLAND_DISPLAY and the XWayland DISPLAY exported, backend not opted in.
           Records list_windows' coverage note, health's wayland_backend entry and
           the text of the app's own window picker (read from its accessibility tree).
  optin    CUA_DRIVER_RS_ENABLE_WAYLAND=1. For the native GTK3 target WL: readiness
           routes, per-window capture (compared with the compositor's own output
           pixels on sway), background type_text with and without an element token,
           background and foreground press_key, foreground type_text, and on sway
           a floating window moved over WL to test overlap attestation.

Oracles: the fixture's own log (entry text, clicks, GTK is-active) and, on sway,
the IPC focused container before and after each call. A guard window is
created last and holds focus. ``false_success`` lists calls that returned
without isError while the requested effect never happened.
"""
import base64
import glob
import hashlib
import io
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
assert compositor in ('sway', 'mutter') and variant in ('default', 'optin')
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
            connection.settimeout(remaining); chunk = connection.recv(1 << 20)
            if not chunk: raise ConnectionError('peer closed')
            data += chunk
    response = json.loads(data.split(b'\n')[0]); assert response.get('ok'), response
    return response['result']


def events():
    path = work/'receiver-events.jsonl'
    if not path.exists(): return []
    return [json.loads(line) for line in path.read_text().splitlines(keepends=True) if line.endswith('\n')]


def sway(*args):
    return subprocess.run(['swaymsg', *args], env=env, capture_output=True, text=True, timeout=5)


def sway_nodes():
    if compositor != 'sway': return []
    tree = json.loads(sway('-t', 'get_tree').stdout); out = []; stack = [tree]
    while stack:
        node = stack.pop()
        if node.get('pid'): out.append(node)
        stack += node.get('nodes', []) + node.get('floating_nodes', [])
    return out


def sway_focus():
    return next(({'pid': n['pid'], 'name': n.get('name')} for n in sway_nodes() if n.get('focused')), None)


def deadline_signal(*_): raise TimeoutError('wayland input probe deadline')


signal.signal(signal.SIGTERM, deadline_signal); signal.signal(signal.SIGALRM, deadline_signal); signal.alarm(240)
try:
    from PIL import Image
    report['host_network_namespace'] = os.environ['ICU_PROBE_HOST_NETNS']
    assert report['host_network_namespace'] != os.readlink('/proc/self/ns/net'), 'private network namespace required'
    session_file = work/'session-env'
    dump = ['sh', '-c', f'env > {session_file}.tmp && mv {session_file}.tmp {session_file}; exec sleep 100000']
    if compositor == 'sway':
        (work/'sway.conf').write_text('output HEADLESS-1 resolution 1920x1080 position 0 0\nxwayland force\nfocus_on_window_activation none\n')
        launch(['sway', '-c', str(work/'sway.conf')], 'compositor',
            extra={'WLR_BACKENDS': 'headless', 'WLR_RENDERER': 'pixman', 'WLR_LIBINPUT_NO_DEVICES': '1'})
        env['SWAYSOCK'] = wait_for(lambda: glob.glob(str(work/'runtime'/'sway-ipc.*.sock')), 15, 'sway IPC socket missing')[0]
        wait_for(lambda: sway('exec', ' '.join(f"'{a}'" for a in dump)).returncode == 0, 10, 'swaymsg exec failed')
    else:
        launch(['mutter', '--headless', '--wayland', '--virtual-monitor', '1920x1080', '--', *dump], 'compositor')
    wait_for(session_file.exists, 20, 'compositor session environment not published')
    session = dict(line.split('=', 1) for line in session_file.read_text().splitlines() if '=' in line)
    env.update({k: session[k] for k in ('WAYLAND_DISPLAY', 'DISPLAY', 'XAUTHORITY') if k in session})
    assert 'DISPLAY' in env, 'XWayland DISPLAY was not exported by the compositor'
    env['XDG_CURRENT_DESKTOP'] = 'sway' if compositor == 'sway' else 'GNOME'
    subprocess.run(['dbus-update-activation-environment', *[k for k in ('DISPLAY', 'WAYLAND_DISPLAY', 'XAUTHORITY', 'XDG_RUNTIME_DIR', 'XDG_SESSION_TYPE', 'XDG_CURRENT_DESKTOP') if k in env]],
        env=env, check=True, timeout=5)

    fixtures = {}
    def start_fixture(name, backend):
        child = launch(['/usr/bin/python3', str(fixture), str(work), name], 'fixture-'+name, extra={'GDK_BACKEND': backend},
            drop=('DISPLAY',) if backend == 'wayland' else ())
        def ready():
            if child.poll() is not None: raise RuntimeError(f'fixture {name} exited')
            path = work/f'fixture-{child.pid}.json'
            return json.loads(path.read_text()) if path.exists() else None
        fixtures[name] = wait_for(ready, 15, f'fixture {name} not ready')
    start_fixture('WL', 'wayland')
    if variant == 'optin' and compositor == 'sway': start_fixture('Float', 'wayland')

    app = launch([str(exe)], 'app', extra={'CUA_DRIVER_RS_ENABLE_WAYLAND': '1'} if variant == 'optin' else {})
    report['app_pid'] = app.pid
    sock = work/'cache/icu'/f'driver-{app.pid}.sock'
    def metadata():
        if app.poll() is not None: raise RuntimeError(f'app exited during startup (code {app.returncode})')
        try: return protocol(sock, {'method': 'metadata'}, 1)
        except (OSError, ValueError): return None
    meta = wait_for(metadata, 40, 'owned driver not ready'); report['metadata'] = meta
    assert pathlib.Path(f'/proc/{meta["pid"]}/exe').resolve() == exe.with_name('cua-driver'), 'wrong driver binary'
    driver_identity = (meta['pid'], identity(meta['pid']))
    report['app_sha256'] = hashlib.sha256(exe.read_bytes()).hexdigest()
    report['driver_sha256'] = hashlib.sha256(exe.with_name('cua-driver').read_bytes()).hexdigest()
    def raw_call(name, args, timeout=15):
        return protocol(sock, {'method': 'call', 'name': name, 'args': args, 'session_id': 'wl-input', 'client_kind': 'cli'}, timeout)
    raw_call('list_windows', {}); raw_call('set_agent_cursor_enabled', {'session': 'wl-input', 'enabled': False})
    time.sleep(1.5)
    start_fixture('Guard', 'x11')
    time.sleep(1)
    if compositor == 'sway':
        sway(f'[pid={fixtures["Guard"]["pid"]}] focus'); time.sleep(.5)
    report['compositor_windows'] = [{k: n.get(k) for k in ('id', 'pid', 'name', 'shell', 'rect', 'focused')} for n in sway_nodes()]
    report['focus_baseline'] = sway_focus()

    def call(label, name, args, effect=None, target='WL', timeout=15):
        before = len(events()); focus_before = sway_focus()
        entry = {'label': label, 'tool': name, 'args': {k: v for k, v in args.items() if k != 'session'}}
        try: result = raw_call(name, args, timeout)
        except Exception as error:
            entry.update(transport_error=f'{type(error).__name__}: {error}'); calls.append(entry); return entry, None
        if effect:
            try: wait_for(lambda: any(effect(e) for e in events()[before:]), 2.5, 'no effect')
            except RuntimeError: pass
        time.sleep(.5)
        structured = result.get('structuredContent')
        entry.update(is_error=bool(result.get('isError')),
            text=[c.get('text', '')[:700] for c in result.get('content', []) if c.get('type') == 'text'][:2],
            structured={k: v for k, v in (structured or {}).items() if k not in ('elements', 'tree_markdown', 'screenshots', 'passive_elements')} if isinstance(structured, dict) else structured,
            events=events()[before:], focus_before=focus_before, focus_after=sway_focus())
        if effect is not None:
            entry['effect_observed'] = any(effect(e) for e in entry['events'])
            entry['false_success'] = not entry['is_error'] and not entry['effect_observed']
        entry['target_activated'] = any(e['kind'] == 'active' and e['active'] and e['window'] == target for e in entry['events'])
        entry['guard_deactivated'] = any(e['kind'] == 'active' and not e['active'] and e['window'] == 'Guard' for e in entry['events'])
        image = next((c for c in result.get('content', []) if c.get('type') == 'image'), None)
        if image:
            png = base64.b64decode(image['data']); path = work/f'{len(calls):02d}-{label}.png'; path.write_bytes(png)
            entry['image'] = {'file': path.name, 'size': list(struct.unpack('!II', png[16:24]))}
        calls.append(entry); return entry, result

    health, health_result = call('health', 'health_report', {})
    checks_by_name = {c.get('name'): c for c in ((health_result or {}).get('structuredContent') or {}).get('checks', [])}
    report['wayland_backend'] = checks_by_name.get('wayland_backend')
    listed_entry, listed = call('list_windows', 'list_windows', {})
    listed_s = (listed or {}).get('structuredContent') or {}
    report['window_coverage'] = listed_s.get('window_coverage')
    windows = listed_s.get('windows', [])
    report['listed_windows'] = [{k: w.get(k) for k in ('pid', 'window_id', 'title', 'bounds')} for w in windows]
    wl = next((w for w in windows if w.get('pid') == fixtures['WL']['pid']), None)
    report['checks'] = checks = {}

    if variant == 'default':
        # The app's own picker, read from its accessibility tree.
        import pyatspi
        def walk(node, depth=0):
            yield node
            if depth < 40:
                for child in node:
                    if child is not None: yield from walk(child, depth+1)
        def rows():
            out = []
            for application in pyatspi.Registry.getDesktop(0):
                try:
                    if application is None or application.get_process_id() != app.pid: continue
                except Exception: continue
                for node in walk(application):
                    try:
                        try: text = node.queryText().getText(0, -1)
                        except Exception: text = ''
                        out.append((node, node.getRoleName(), node.name or '', text))
                    except Exception: pass
            return out
        opener = wait_for(lambda: next((n for n, role, name, _ in rows() if role in ('push button', 'button') and 'window' in name), None), 30, 'picker button not exposed')
        opener.queryAction().doAction(0)
        time.sleep(3)
        report['picker_texts'] = sorted({(name or text).strip()[:200] for _, _, name, text in rows() if (name or text).strip()})
        if compositor == 'sway':
            subprocess.run(['grim', str(work/'compositor.png')], env=env, capture_output=True, timeout=10)
        checks.update({
            'native_window_absent': wl is None,
            'coverage_note': (report['window_coverage'] or {}).get('code') == 'native_wayland_windows_not_listed',
            'readiness_says_so': (report['wayland_backend'] or {}).get('status') == 'fail'
                and ((report['wayland_backend'] or {}).get('data') or {}).get('error_detail') == 'native_wayland_windows_not_listed',
            'picker_notice': any("Native Wayland windows can't be listed" in t for t in report['picker_texts']),
        })
    else:
        routes = ((report['wayland_backend'] or {}).get('data') or {}).get('wayland_routes')
        report['routes'] = routes
        assert wl, 'native target not listed in opt-in mode'
        scope = {'pid': wl['pid'], 'window_id': wl['window_id']}
        state_entry, state = call('WL-state', 'get_window_state', {**scope, 'include_screenshot': True, 'include_accessibility_tree': True, 'max_dimension': 4096})
        s = (state or {}).get('structuredContent') or {}
        entry_token = next((e.get('element_token') for e in s.get('elements', []) if e.get('role') in ('text', 'entry')), None)
        frame_check = None
        if compositor == 'sway' and state_entry.get('image'):
            node = next(n for n in sway_nodes() if n['pid'] == wl['pid']); rect = node['rect']
            subprocess.run(['grim', '-g', f"{rect['x']},{rect['y']} {rect['width']}x{rect['height']}", str(work/'grim-WL.png')], env=env, check=True, timeout=10)
            a = Image.open(work/state_entry['image']['file']).convert('RGB'); b = Image.open(work/'grim-WL.png').convert('RGB')
            same = a.size == b.size and sum(1 for p, q in zip(a.getdata(), b.getdata()) if p == q) / (a.size[0]*a.size[1])
            frame_check = {'driver': list(a.size), 'grim': list(b.size), 'equal_fraction': same}
        report['frame_vs_compositor'] = frame_check
        text_is = lambda value: (lambda e: e['kind'] == 'text' and e['window'] == 'WL' and e['text'] == value)
        typed, _ = call('WL-type-window-bg', 'type_text', {'session': 'wl-input', **scope, 'text': 'wl ok', 'delivery_mode': 'background'}, effect=text_is('wl ok'))
        typed_el = None
        if entry_token:
            typed_el, _ = call('WL-type-element-bg', 'type_text', {'session': 'wl-input', **scope, 'element_token': entry_token, 'text': '+el', 'delivery_mode': 'background'},
                effect=lambda e: e['kind'] == 'text' and e['window'] == 'WL' and e['text'].endswith('+el'))
        key_bg, _ = call('WL-press-key-bg', 'press_key', {'session': 'wl-input', **scope, 'key': 'a', 'delivery_mode': 'background'},
            effect=lambda e: e['kind'] == 'text' and e['window'] == 'WL' and e['text'].endswith('a'))
        key_fg, _ = call('WL-press-key-fg', 'press_key', {'session': 'wl-input', **scope, 'key': 'b', 'delivery_mode': 'foreground'},
            effect=lambda e: e['kind'] == 'text' and e['window'] == 'WL' and e['text'].endswith('b'))
        type_fg, _ = call('WL-type-fg', 'type_text', {'session': 'wl-input', **scope, 'text': ' fg é', 'delivery_mode': 'foreground'},
            effect=lambda e: e['kind'] == 'text' and e['window'] == 'WL' and e['text'].endswith(' fg é'))
        overlap = None
        if compositor == 'sway':
            sway(f'[pid={fixtures["Float"]["pid"]}] floating enable, resize set 500 400, move position {wl["bounds"]["x"] + 80} {wl["bounds"]["y"] + 80}')
            time.sleep(.8)
            report['float_rect'] = next((n['rect'] for n in sway_nodes() if n['pid'] == fixtures['Float']['pid']), None)
            overlap, _ = call('WL-state-overlapped', 'get_window_state', {**scope, 'include_screenshot': True, 'include_accessibility_tree': False})
        background = [typed, key_bg] + ([typed_el] if typed_el else []) + [state_entry] + ([overlap] if overlap else [])
        code = lambda entry: ((entry or {}).get('structured') or {}).get('screenshot_error', {}).get('code') if isinstance((entry or {}).get('structured'), dict) else None
        checks.update({
            'readiness_routes_reported': bool(routes) and routes.get('background_keys') == 'background_unavailable',
            'capture_truthful': (state_entry.get('image') is not None and (frame_check or {}).get('equal_fraction', 0) > .99) if compositor == 'sway'
                else code(state_entry) == 'surface_identity_unproven',
            'capture_matches_readiness': bool(routes) and ((routes.get('window_capture') == 'compositor_attested_output_crop') == (state_entry.get('image') is not None)),
            'window_text_bg_delivered': typed.get('effect_observed') is True and not typed.get('is_error'),
            'element_text_bg_delivered': typed_el is not None and typed_el.get('effect_observed') is True,
            'key_bg_refused_typed': key_bg.get('is_error') and not key_bg.get('effect_observed')
                and (key_bg.get('structured') or {}).get('code') == 'background_unavailable',
            'foreground_keys_match_readiness': bool(routes) and (
                (key_fg.get('effect_observed') is True and type_fg.get('effect_observed') is True) if routes.get('foreground_keys') == 'zwp_virtual_keyboard'
                else (key_fg.get('is_error') and not key_fg.get('effect_observed') and type_fg.get('is_error') and not type_fg.get('effect_observed'))),
            'overlap_refused': compositor != 'sway' or (code(overlap) == 'surface_identity_unproven' and 'overlaps' in json.dumps(overlap.get('structured'))),
            'background_never_moves_focus': all(not c.get('guard_deactivated') and not c.get('target_activated')
                and c.get('focus_before') == c.get('focus_after') for c in background),
            'foreground_restores_focus': compositor != 'sway' or all(c.get('focus_after') == report['focus_baseline'] for c in (key_fg, type_fg)),
        })
        if compositor == 'sway':
            subprocess.run(['grim', str(work/'compositor.png')], env=env, capture_output=True, timeout=10)
    report['false_success'] = [c['label'] for c in calls if c.get('false_success')]
    checks['no_false_success'] = not report['false_success']
    report['ok'] = all(checks.values())
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
    print(json.dumps({k: report.get(k) for k in ('ok', 'error', 'checks', 'false_success', 'routes', 'frame_vs_compositor')}, indent=2))
    sys.exit(0 if report.get('ok') else 1)
