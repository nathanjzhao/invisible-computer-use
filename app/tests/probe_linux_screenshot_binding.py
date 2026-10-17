"""Screenshot binding on X11: a pixel action bound to a `screenshot_id` is
checked against that capture, not whichever capture of the window ran last.

    probe_linux_screenshot_binding.py WORK CUA_DRIVER RECEIVER

RECEIVER is fixtures/x11_drag_receiver.py (a raw 1280x800 X window that logs
every button event it receives in window coordinates). The driver runs as its
own daemon on a private socket. Each case records what the receiver saw and
what the driver returned; the probe exits 0 only when every case matches the
binding contract (EXPECTED below).
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
import time
import traceback

if sys.platform != 'linux': raise SystemExit('Linux-only probe')
work = pathlib.Path(sys.argv[1]); work.mkdir(parents=True)
driver = pathlib.Path(sys.argv[2]).resolve()
fixture = pathlib.Path(sys.argv[3]).resolve()
processes = []; logs = []
report = {'probe_sha256': hashlib.sha256(pathlib.Path(__file__).read_bytes()).hexdigest(),
    'driver_sha256': hashlib.sha256(driver.read_bytes()).hexdigest(), 'cases': {}}
env = dict(os.environ)
env.update({'XDG_CACHE_HOME': str(work/'cache'), 'XDG_RUNTIME_DIR': str(work/'runtime'),
    'XDG_SESSION_TYPE': 'x11', 'LIBGL_ALWAYS_SOFTWARE': '1'})
env.pop('WAYLAND_DISPLAY', None)
(work/'runtime').mkdir(); (work/'runtime').chmod(0o700)

# Where the agent's pixel (160, 100) must land, or the refusal code, per case.
# Window pixels: the agent reads a 640-wide capture of a 1280-wide window, so
# its (160, 100) is window (320, 200). Without an id, the pixel is read against
# the calling session's own latest capture (c9-unify). This probe's one session
# takes both the agent and the tile captures, so that is the tile's: (160, 100)
# of a 1000-wide window read at 640 is (250, 156), of a 1280-wide one read at
# 1200 is (170, 106), and a resize since it refuses stale_capture.
EXPECTED = {
    'bound_click_after_resize_and_tile_capture': 'refused:stale_capture',
    'bound_double_click_after_resize_and_tile_capture': 'refused:stale_capture',
    'bound_right_click_after_resize_and_tile_capture': 'refused:stale_capture',
    'bound_drag_after_resize_and_tile_capture': 'refused:stale_capture',
    'unbound_click_after_tile_capture': [250, 156],
    'tile_id_click_after_resize': [250, 156],
    'unbound_click_after_resize_without_recapture': 'refused:stale_capture',
    'bound_click_after_tile_capture_at_other_budget': [320, 200],
    'unbound_click_after_tile_capture_at_other_budget': [170, 106],
    'bound_click_after_move': [320, 200],
    'unknown_id': 'refused:stale_capture',
    'other_windows_id': 'refused:invalid_arguments',
}

def launch(args, name, stdout=None):
    log = (work/(name+'.log')).open('wb'); logs.append(log)
    child = subprocess.Popen(args, env=env, stdin=subprocess.DEVNULL,
        stdout=stdout if stdout is not None else log, stderr=log, start_new_session=True)
    processes.append(child); return child

def wait_for(predicate, seconds, message):
    deadline = time.monotonic()+seconds
    while time.monotonic() < deadline:
        result = predicate()
        if result: return result
        time.sleep(.02)
    raise RuntimeError(message)

def protocol(path, request, timeout=8):
    with socket.socket(socket.AF_UNIX) as connection:
        connection.settimeout(timeout); connection.connect(str(path))
        connection.sendall((json.dumps(request)+'\n').encode()); data = b''
        while b'\n' not in data:
            chunk = connection.recv(1 << 20)
            if not chunk: raise ConnectionError('peer closed')
            data += chunk
    response = json.loads(data.split(b'\n')[0]); assert response.get('ok'), response
    return response['result']

def events():
    path = work/'receiver-events.jsonl'
    if not path.exists(): return []
    return [json.loads(line) for line in path.read_text().splitlines(keepends=True) if line.endswith('\n')]

def deadline_signal(*_): raise TimeoutError('probe deadline')
signal.signal(signal.SIGALRM, deadline_signal); signal.alarm(150)
try:
    xvfb = launch(['Xvfb', '-displayfd', '1', '-screen', '0', '1600x1000x24', '-nolisten', 'tcp'], 'xvfb', subprocess.PIPE)
    assert select.select([xvfb.stdout], [], [], 8)[0], 'Xvfb startup timeout'
    env['DISPLAY'] = ':'+xvfb.stdout.readline().decode().strip()
    launch(['openbox'], 'openbox'); time.sleep(.5)
    receiver = launch(['/usr/bin/python3', str(fixture), str(work), 'resize'], 'receiver')
    wait_for(lambda: (work/'fixture-ready.json').exists(), 5, 'receiver not ready')
    ready = json.loads((work/'fixture-ready.json').read_text())
    target, guard, pid = ready['target'], ready['guard'], ready['pid']
    sock = work/'driver.sock'
    launch([str(driver), 'serve', '--socket', str(sock)], 'driver')
    wait_for(lambda: sock.exists(), 20, 'driver socket missing'); time.sleep(.3)

    def call(name, args):
        return protocol(sock, {'method': 'call', 'name': name, 'args': args,
            'session_id': 'binding-probe', 'client_kind': 'cli'})
    call('set_agent_cursor_enabled', {'session': 'binding-probe', 'enabled': False})

    def capture(wid, max_dimension, label):
        result = call('get_window_state', {'pid': pid, 'window_id': wid, 'include_screenshot': True,
            'include_accessibility_tree': False, 'max_dimension': max_dimension})
        assert not result.get('isError'), result
        image = next(c for c in result['content'] if c.get('type') == 'image')
        png = base64.b64decode(image['data']); (work/f'{label}.png').write_bytes(png)
        size = list(struct.unpack('!II', png[16:24]))
        return {'size': size, 'screenshot_id': result['structuredContent'].get('screenshot_id')}

    def xdotool(*args):
        subprocess.run(['xdotool', *args], env=env, check=True, timeout=5); time.sleep(.4)

    def geometry():
        out = subprocess.run(['xdotool', 'getwindowgeometry', str(target)], env=env,
            capture_output=True, text=True, timeout=5).stdout
        return out.split('Geometry:')[1].split()[0]

    def act(case, tool, screenshot_id, extra=None):
        args = {'pid': pid, 'window_id': target, 'delivery_mode': 'background'}
        args.update(extra or {'x': 160, 'y': 100})
        if screenshot_id is not None: args['screenshot_id'] = screenshot_id
        before = len(events())
        result = call(tool, args)
        time.sleep(.6)
        seen = events()[before:]
        sc = result.get('structuredContent') or {}
        downs = [[e['x'], e['y']] for e in seen if e['kind'] == 'down']
        if result.get('isError'):
            outcome = 'refused:' + str(sc.get('code') or 'untyped')
        else:
            outcome = downs[0] if downs else 'no_event'
        if seen and result.get('isError'):
            outcome = 'refused_but_delivered'
        report['cases'][case] = {'tool': tool, 'screenshot_id': screenshot_id, 'window_geometry': geometry(),
            'is_error': bool(result.get('isError')), 'code': sc.get('code'),
            'text': ''.join(c.get('text', '') for c in result.get('content', []))[:400],
            'downs': downs, 'outcome': outcome, 'expected': EXPECTED[case],
            'pass': outcome == EXPECTED[case]}

    # 1. Agent capture, window resized, tile recapture, agent acts on its own pixel.
    for tool in ('click', 'double_click', 'right_click', 'drag'):
        xdotool('windowsize', str(target), '1280', '800')
        agent = capture(target, 640, f'agent-{tool}'); assert agent['size'] == [640, 400], agent
        xdotool('windowsize', str(target), '1000', '800')
        tile = capture(target, 640, f'tile-{tool}'); assert tile['size'] == [640, 512], tile
        report.setdefault('captures', {})[tool] = {'agent': agent, 'tile': tile}
        extra = {'from_x': 160, 'from_y': 100, 'to_x': 200, 'to_y': 100} if tool == 'drag' else None
        act(f'bound_{tool}_after_resize_and_tile_capture', tool, agent['screenshot_id'], extra)
    # Without an id the session's latest capture (the tile's) is read; so does the tile's own id.
    act('unbound_click_after_tile_capture', 'click', None)
    act('tile_id_click_after_resize', 'click', tile['screenshot_id'])
    # 2. Resized after the session's latest capture, no id: stale.
    xdotool('windowsize', str(target), '1280', '800')
    act('unbound_click_after_resize_without_recapture', 'click', None)
    # 3. Same size, tile recaptures at another budget: the bound pixel keeps its own ratio.
    agent = capture(target, 640, 'agent-budget'); assert agent['size'] == [640, 400], agent
    tile = capture(target, 1200, 'tile-budget'); assert tile['size'] == [1200, 750], tile
    act('bound_click_after_tile_capture_at_other_budget', 'click', agent['screenshot_id'])
    act('unbound_click_after_tile_capture_at_other_budget', 'click', None)
    # 4. A move keeps window-local pixels valid.
    agent = capture(target, 640, 'agent-move')
    xdotool('windowmove', str(target), '120', '160')
    act('bound_click_after_move', 'click', agent['screenshot_id'])
    # 5. Ids this daemon never issued, or issued for another window.
    act('unknown_id', 'click', 'f00000000-999999')
    other = capture(guard, 640, 'guard')
    act('other_windows_id', 'click', other['screenshot_id'])
    report['ok'] = all(case['pass'] for case in report['cases'].values()) and len(report['cases']) == len(EXPECTED)
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
    print(json.dumps({k: v['outcome'] for k, v in report['cases'].items()}, indent=1))
    print('ok' if report.get('ok') else 'FAIL', report.get('error', ''))
    sys.exit(0 if report.get('ok') else 1)
