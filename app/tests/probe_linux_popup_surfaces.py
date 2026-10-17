"""GTK3 popup menus as z-ordered surfaces beside their window (spec 07), X11.

Usage: probe_linux_popup_surfaces.py WORK /usr/bin/app popup_menu_gtk.py COMPOSITOR

COMPOSITOR is ``none`` or ``picom``. Xvfb (1600x1000) + openbox. ``Target``
owns a window and a real Gtk.Menu whose items are solid red, green and blue.
``Other`` is a second process that maps an override-redirect popup over
Target's window. ``Guard`` holds focus. The probe asks the installed app's
own driver for Target's state with the menu closed, open, open under Other's
popup, and closed again, then addresses the menu window directly.

Expected with the fix: while the menu is open, ``screenshots`` holds exactly
Target's window (zIndex 0) and the menu (zIndex 1), the menu surface's origin
and size equal the X server's geometry for the menu window, its pixels show
the three item colors (and, without a compositor, equal the root's pixels at
that rectangle), Other's popup is never included, and closing the menu
returns to one surface. Addressing the menu window directly stays degraded
with no element tokens (the linux-identity rule), Target's tokens never name
menu items, and no driver call deactivates the guard.
"""
import base64
import hashlib
import io
import json
import os
import pathlib
import select
import signal
import socket
import subprocess
import sys
import time
import traceback

if sys.platform != 'linux': raise SystemExit('Linux-only probe')
work = pathlib.Path(sys.argv[1]); work.mkdir(parents=True)
exe = pathlib.Path(sys.argv[2]).resolve(); fixture = pathlib.Path(sys.argv[3]).resolve()
compositor = sys.argv[4]; assert compositor in ('none', 'picom')
processes = []; logs = []; calls = []; driver_identity = None
report = {'compositor': compositor, 'calls': calls,
    'probe_sha256': hashlib.sha256(pathlib.Path(__file__).read_bytes()).hexdigest(),
    'fixture_sha256': hashlib.sha256(fixture.read_bytes()).hexdigest()}
env = {k: v for k, v in os.environ.items() if not k.startswith(('ICU_', 'WAYLAND_'))}
env.update({'XDG_CACHE_HOME': str(work/'cache'), 'XDG_DATA_HOME': str(work/'data'),
    'XDG_CONFIG_HOME': str(work/'config'), 'XDG_RUNTIME_DIR': str(work/'runtime'),
    'XDG_SESSION_TYPE': 'x11', 'GTK_MODULES': 'gail:atk-bridge', 'NO_AT_BRIDGE': '0',
    'LIBGL_ALWAYS_SOFTWARE': '1', 'ICU_BRAIN': 'ollama'})
(work/'runtime').mkdir(); (work/'runtime').chmod(0o700)
COLORS = {'Copy': (0xd0, 0x30, 0x30), 'Paste': (0x30, 0xa0, 0x40), 'Delete': (0x30, 0x50, 0xd0)}


def identity(pid):
    try: return pathlib.Path(f'/proc/{pid}/stat').read_text().rpartition(') ')[2].split()[19]
    except FileNotFoundError: return None


def launch(args, name, stdout=None):
    log = (work/(name+'.log')).open('wb'); logs.append(log)
    child = subprocess.Popen(args, env=env, stdin=subprocess.DEVNULL, stdout=stdout or log, stderr=log, start_new_session=True)
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


def geometry(xid):
    info = subprocess.run(['xwininfo', '-id', str(xid)], env=env, capture_output=True, text=True, timeout=5).stdout
    value = lambda key: int(next(l for l in info.splitlines() if l.strip().startswith(key)).split(':')[1])
    return {'x': value('Absolute upper-left X'), 'y': value('Absolute upper-left Y'),
        'width': value('Width'), 'height': value('Height'), 'viewable': 'IsViewable' in info}


def deadline_signal(*_): raise TimeoutError('popup probe deadline')


signal.signal(signal.SIGTERM, deadline_signal); signal.signal(signal.SIGALRM, deadline_signal); signal.alarm(180)
try:
    from PIL import Image
    report['host_network_namespace'] = os.environ['ICU_PROBE_HOST_NETNS']
    assert report['host_network_namespace'] != os.readlink('/proc/self/ns/net'), 'private network namespace required'
    xvfb = launch(['Xvfb', '-displayfd', '1', '-screen', '0', '1600x1000x24', '-nolisten', 'tcp'], 'xvfb', subprocess.PIPE)
    assert select.select([xvfb.stdout], [], [], 15)[0], 'Xvfb startup timeout'
    env['DISPLAY'] = ':'+xvfb.stdout.readline().decode().strip()
    subprocess.run(['dbus-update-activation-environment', 'DISPLAY', 'XDG_RUNTIME_DIR', 'XDG_DATA_HOME', 'XDG_CONFIG_HOME'], env=env, check=True, timeout=5)
    launch(['openbox'], 'openbox')
    if compositor == 'picom': launch(['picom', '--backend', 'xrender'], 'picom')
    time.sleep(.5)
    children = {'Target': launch(['/usr/bin/python3', str(fixture), str(work), 'Target', '100', '100'], 'target'),
        'Other': launch(['/usr/bin/python3', str(fixture), str(work), 'Other', '900', '100', '--overlay', '380', '300'], 'other'),
        'Guard': launch(['/usr/bin/python3', str(fixture), str(work), 'Guard', '1000', '600'], 'guard')}

    def ids(name, check=None):
        child = children[name]
        def read():
            if child.poll() is not None: raise RuntimeError(f'fixture {name} exited')
            path = work/f'fixture-{child.pid}.json'
            value = json.loads(path.read_text()) if path.exists() else None
            return value if value and (check is None or check(value)) else None
        return wait_for(read, 10, f'fixture {name} not ready')
    fx = {name: ids(name) for name in children}; report['fixtures'] = fx
    target_pid, target_xid = fx['Target']['pid'], fx['Target']['window']

    def command(name, word, check):
        (work/f'cmd-{name}.tmp').write_text(word); (work/f'cmd-{name}.tmp').rename(work/f'cmd-{name}')
        time.sleep(.2)
        return ids(name, check)

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
    def raw_call(name, args):
        return protocol(sock, {'method': 'call', 'name': name, 'args': args, 'session_id': 'popup-probe', 'client_kind': 'cli'}, 15)
    raw_call('set_agent_cursor_enabled', {'session': 'popup-probe', 'enabled': False})
    subprocess.run(['xdotool', 'windowactivate', '--sync', str(fx['Guard']['window'])], env=env, check=True, timeout=4)
    time.sleep(.5)

    def state(label, pid, xid):
        before = len(events())
        result = raw_call('get_window_state', {'pid': pid, 'window_id': xid, 'include_screenshot': True, 'include_accessibility_tree': True})
        time.sleep(.2)
        s = result.get('structuredContent') or {}
        surfaces = []
        for surface in s.get('screenshots') or []:
            png = base64.b64decode(surface.get('png_b64', ''))
            path = work/f'{len(calls):02d}-{label}-{surface.get("id")}.png'; path.write_bytes(png)
            image = Image.open(io.BytesIO(png)).convert('RGB')
            surfaces.append({**{k: v for k, v in surface.items() if k != 'png_b64'}, 'file': path.name, 'decoded': list(image.size)})
        entry = {'label': label, 'is_error': bool(result.get('isError')), 'surfaces': surfaces,
            'has_screenshots_key': 'screenshots' in s, 'degraded_reason': s.get('degraded_reason'),
            'labels': [e.get('label') for e in s.get('elements') or []],
            'tokens': sum(1 for e in s.get('elements') or [] if e.get('element_token')),
            'guard_deactivated': any(e['kind'] == 'active' and e['window'] == 'Guard' and not e['value'] for e in events()[before:])}
        calls.append(entry); return entry

    def colors_present(file):
        image = Image.open(work/file).convert('RGB'); pixels = list(image.getdata())
        near = lambda p, c: all(abs(a-b) <= 12 for a, b in zip(p, c))
        return {name: sum(1 for p in pixels if near(p, rgb)) / len(pixels) for name, rgb in COLORS.items()}

    def root_matches(file, rect):
        # Without a compositor the root framebuffer is what is on screen.
        from Xlib import X, display
        conn = display.Display(env['DISPLAY']); root = conn.screen().root
        raw = root.get_image(rect['x'], rect['y'], rect['width'], rect['height'], X.ZPixmap, 0xffffffff)
        conn.close()
        screen = Image.frombytes('RGB', (rect['width'], rect['height']), raw.data, 'raw', 'BGRX')
        screen.save(work/'root-at-menu.png')
        surface = Image.open(work/file).convert('RGB')
        if surface.size != screen.size: return 0.0
        a, b = list(surface.getdata()), list(screen.getdata())
        return sum(1 for p, q in zip(a, b) if p == q) / len(a)

    closed = state('menu-closed', target_pid, target_xid)
    opened_ids = command('Target', 'popup', lambda v: v.get('menu_visible') and v.get('menu'))
    time.sleep(.6)
    menu_xid = opened_ids['menu']; report['menu_xid'] = menu_xid
    report['menu_geometry'] = menu_geometry = geometry(menu_xid)
    opened = state('menu-open', target_pid, target_xid)
    menu_surface = next((s for s in opened['surfaces'] if s.get('zIndex') == 1), None)
    report['menu_surface_root_match'] = root_matches(menu_surface['file'], menu_geometry) if menu_surface and compositor == 'none' else None
    overlay_ids = command('Other', 'overlay', lambda v: v.get('overlay'))
    time.sleep(.5)
    report['overlay_xid'] = overlay_ids['overlay']; report['overlay_geometry'] = geometry(overlay_ids['overlay'])
    under_other = state('menu-open-other-popup', target_pid, target_xid)
    direct = state('menu-xid-direct', target_pid, menu_xid)
    command('Target', 'popdown', lambda v: not v.get('menu_visible'))
    command('Other', 'unoverlay', lambda v: True)
    time.sleep(.5)
    reclosed = state('menu-closed-again', target_pid, target_xid)

    report['menu_surface_colors'] = colors_present(menu_surface['file']) if menu_surface else None
    rect_of = lambda s: {'x': s.get('originX'), 'y': s.get('originY'), 'width': s.get('width'), 'height': s.get('height')}
    menu_rect = {k: menu_geometry[k] for k in ('x', 'y', 'width', 'height')}
    report['checks'] = checks = {
        'closed_single_surface': not closed['is_error'] and not closed['has_screenshots_key'],
        'open_two_surfaces': [s.get('zIndex') for s in opened['surfaces']] == [0, 1]
            and opened['surfaces'][0].get('window_id') == target_xid and menu_surface is not None and menu_surface.get('window_id') == menu_xid,
        'menu_geometry_matches_x': menu_surface is not None and rect_of(menu_surface) == menu_rect
            and menu_surface['decoded'] == [menu_surface.get('pixel_width'), menu_surface.get('pixel_height')],
        'menu_pixels_show_items': bool(report['menu_surface_colors']) and all(v > .02 for v in report['menu_surface_colors'].values()),
        'menu_pixels_equal_root': compositor != 'none' or (report['menu_surface_root_match'] or 0) > .99,
        'other_process_popup_excluded': report['overlay_geometry']['viewable']
            and [s.get('window_id') for s in under_other['surfaces']] == [target_xid, menu_xid],
        'target_tokens_never_name_menu_items': opened['tokens'] > 0 and not any(l in COLORS for l in opened['labels']),
        'menu_xid_direct_stays_tokenless': direct['tokens'] == 0 and 'accessibility_window_identity_unproven' in (direct['degraded_reason'] or ''),
        'closed_again_single_surface': not reclosed['is_error'] and not reclosed['has_screenshots_key'],
        'no_guard_deactivation': not any(c['guard_deactivated'] for c in calls),
    }
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
    print(json.dumps({k: report.get(k) for k in ('ok', 'error', 'checks', 'menu_surface_colors', 'menu_surface_root_match')}, indent=2))
    sys.exit(0 if report.get('ok') else 1)
