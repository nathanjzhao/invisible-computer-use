"""Screenshot binding on sway (native Wayland opt-in). Since round 3 (R3-B1)
sway window capture is refused outright, so there is no sway frame for a
pixel to be bound to: the capture returns a typed refusal and no
screenshot_id, and an id the driver never issued is refused `stale_capture`
rather than trusted. The pre-round-3 cases (a bound click refused after a
resize and a tile capture) no longer have a capture to start from.

    probe_sway_screenshot_binding.py WORK CUA_DRIVER GRID_FIXTURE

GRID_FIXTURE is fixtures/wayland_grid_gtk.py (an entry over a 2x2 button
grid, homogeneous, so a resize moves every button). The window is floated at
600x360; the agent reads the center of button WL-21 in its capture. Resized
to 1000x700 that pixel is on WL-10; the 1000x700 capture's WL-21 pixel is on
WL-11 at 1200x900.
"""
import base64
import glob
import hashlib
import json
import os
import pathlib
import signal
import struct
import subprocess
import sys
import time
import traceback
import socket

work = pathlib.Path(sys.argv[1]); work.mkdir(parents=True)
driver = pathlib.Path(sys.argv[2]).resolve()
fixture = pathlib.Path(sys.argv[3]).resolve()
processes = []; logs = []
report = {'probe_sha256': hashlib.sha256(pathlib.Path(__file__).read_bytes()).hexdigest(),
    'driver_sha256': hashlib.sha256(driver.read_bytes()).hexdigest(), 'cases': {}}
env = {k: v for k, v in os.environ.items() if k not in ('DISPLAY', 'WAYLAND_DISPLAY')}
env.update({'XDG_RUNTIME_DIR': str(work/'runtime'), 'XDG_SESSION_TYPE': 'wayland', 'XDG_CURRENT_DESKTOP': 'sway',
    'GTK_MODULES': 'gail:atk-bridge', 'NO_AT_BRIDGE': '0'})
(work/'runtime').mkdir(); (work/'runtime').chmod(0o700)

EXPECTED = {
    'capture_refused_without_an_id': 'refused:surface_identity_unproven',
    'click_with_an_id_never_issued': 'refused:stale_capture',
}

def launch(args, name, extra=None):
    log = (work/(name+'.log')).open('wb'); logs.append(log)
    child = subprocess.Popen(args, env={**env, **(extra or {})}, stdin=subprocess.DEVNULL, stdout=log, stderr=log, start_new_session=True)
    processes.append(child); return child

def wait_for(predicate, seconds, message):
    deadline = time.monotonic()+seconds
    while time.monotonic() < deadline:
        result = predicate()
        if result: return result
        time.sleep(.05)
    raise RuntimeError(message)

def sway(*args):
    return subprocess.run(['swaymsg', *args], env=env, capture_output=True, text=True, timeout=5)

def node_rect(pid):
    tree = json.loads(sway('-t', 'get_tree').stdout); stack = [tree]
    while stack:
        node = stack.pop()
        if node.get('pid') == pid: return node['rect']
        stack += node.get('nodes', []) + node.get('floating_nodes', [])

def events():
    path = work/'receiver-events.jsonl'
    if not path.exists(): return []
    return [json.loads(line) for line in path.read_text().splitlines(keepends=True) if line.endswith('\n')]

def protocol(path, request, timeout=15):
    with socket.socket(socket.AF_UNIX) as connection:
        connection.settimeout(timeout); connection.connect(str(path))
        connection.sendall((json.dumps(request)+'\n').encode()); data = b''
        while b'\n' not in data:
            chunk = connection.recv(1 << 20)
            if not chunk: raise ConnectionError('peer closed')
            data += chunk
    response = json.loads(data.split(b'\n')[0]); assert response.get('ok'), response
    return response['result']

signal.signal(signal.SIGALRM, lambda *_: (_ for _ in ()).throw(TimeoutError('probe deadline'))); signal.alarm(150)
try:
    (work/'sway.conf').write_text('output HEADLESS-1 resolution 1920x1080 position 0 0\nfocus_on_window_activation none\n')
    launch(['sway', '-c', str(work/'sway.conf')], 'sway', {'WLR_BACKENDS': 'headless', 'WLR_RENDERER': 'pixman', 'WLR_LIBINPUT_NO_DEVICES': '1'})
    env['SWAYSOCK'] = wait_for(lambda: glob.glob(str(work/'runtime'/'sway-ipc.*.sock')), 15, 'sway IPC socket missing')[0]
    env['WAYLAND_DISPLAY'] = wait_for(lambda: [p.name for p in (work/'runtime').glob('wayland-*') if not p.name.endswith('.lock')], 10, 'no wayland socket')[0]
    subprocess.run(['dbus-update-activation-environment', 'WAYLAND_DISPLAY', 'XDG_RUNTIME_DIR', 'XDG_SESSION_TYPE', 'XDG_CURRENT_DESKTOP'], env=env, check=True, timeout=5)
    grid = launch(['/usr/bin/python3', str(fixture), str(work), 'WL'], 'fixture', {'GDK_BACKEND': 'wayland'})
    wait_for(lambda: (work/f'fixture-{grid.pid}.json').exists(), 15, 'fixture not ready')
    sway(f'[pid={grid.pid}] floating enable, resize set 600 360, move position 100 100'); time.sleep(.8)
    sock = work/'driver.sock'
    launch([str(driver), 'serve', '--socket', str(sock)], 'driver', {'CUA_DRIVER_RS_ENABLE_WAYLAND': '1'})
    wait_for(sock.exists, 20, 'driver socket missing'); time.sleep(.3)
    def call(name, args):
        return protocol(sock, {'method': 'call', 'name': name, 'args': args, 'session_id': 'sway-binding', 'client_kind': 'cli'})
    call('set_agent_cursor_enabled', {'session': 'sway-binding', 'enabled': False})
    windows = call('list_windows', {})['structuredContent']['windows']
    wid = next(w['window_id'] for w in windows if w.get('pid') == grid.pid)
    report['window_id'] = wid

    def click(case, point, screenshot_id):
        args = {'pid': grid.pid, 'window_id': wid, 'x': point[0], 'y': point[1], 'delivery_mode': 'background'}
        if screenshot_id is not None: args['screenshot_id'] = screenshot_id
        before = len(events()); result = call('click', args); time.sleep(1.0)
        clicked = [e['button'] for e in events()[before:] if e['kind'] == 'clicked']
        sc = result.get('structuredContent') or {}
        outcome = ('refused:' + str(sc.get('code') or 'untyped')) if result.get('isError') else (clicked[0] if clicked else 'no_effect')
        if clicked and result.get('isError'): outcome = 'refused_but_delivered'
        report['cases'][case] = {'point': point, 'screenshot_id': screenshot_id, 'rect': node_rect(grid.pid), 'is_error': bool(result.get('isError')),
            'text': ''.join(c.get('text', '') for c in result.get('content', []))[:400], 'clicked': clicked,
            'outcome': outcome, 'expected': EXPECTED[case], 'pass': outcome == EXPECTED[case]}

    result = call('get_window_state', {'pid': grid.pid, 'window_id': wid, 'include_screenshot': True, 'include_accessibility_tree': False})
    sc = result.get('structuredContent') or {}
    image = any(c.get('type') == 'image' for c in result.get('content', []))
    code = (sc.get('screenshot_error') or {}).get('code')
    outcome = 'image' if image else f'refused:{code}'
    if sc.get('screenshot_id'): outcome += '+screenshot_id'
    report['cases']['capture_refused_without_an_id'] = {'screenshot_error': sc.get('screenshot_error'), 'screenshot_id': sc.get('screenshot_id'),
        'outcome': outcome, 'expected': EXPECTED['capture_refused_without_an_id'], 'pass': outcome == EXPECTED['capture_refused_without_an_id']}
    click('click_with_an_id_never_issued', [450, 300], 'f00000000-1')
    report['ok'] = all(c['pass'] for c in report['cases'].values()) and len(report['cases']) == len(EXPECTED)
except Exception as error:
    report.update(ok=False, error=str(error), traceback=traceback.format_exc())
finally:
    signal.alarm(0)
    for child in reversed(processes):
        if child.poll() is None:
            try: os.killpg(child.pid, signal.SIGTERM)
            except ProcessLookupError: pass
    for child in reversed(processes):
        try: child.wait(timeout=3)
        except subprocess.TimeoutExpired:
            os.killpg(child.pid, signal.SIGKILL); child.wait()
    for log in logs: log.close()
    (work/'result.json').write_text(json.dumps(report, indent=2)+'\n')
    print(json.dumps({k: v['outcome'] for k, v in report['cases'].items()}))
    print('ok' if report.get('ok') else 'FAIL', report.get('error', ''))
    sys.exit(0 if report.get('ok') else 1)
