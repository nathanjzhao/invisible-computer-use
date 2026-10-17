"""The installed app's live tile binds a manual click to the frame it shows.

    probe_linux_tile_binding.py WORK /usr/bin/app RECEIVER TAP

RECEIVER is fixtures/x11_drag_receiver.py (a 1280x800 raw X window that logs
button events in window coordinates); TAP is driver_socket_tap.py, which the
app starts as its driver (ICU_CUA_BIN) so every app->driver request is logged.

1. Open the receiver's tile and click the tile's center. The click request
   must carry a `screenshot_id` issued by this driver (the driver accepts it)
   and land near the receiver's center.
2. Resize the receiver and click the tile at once, before the tile shows a
   frame of the new size. The driver must refuse it with `stale_capture`
   against the frame the tile was still showing, and the receiver must get
   nothing. Up to three attempts; one where the new frame won the race is
   recorded and retried.
"""
import hashlib
import json
import os
from pathlib import Path
import select
import signal
import subprocess
import sys
import time
import traceback

if sys.platform != 'linux': raise SystemExit('Linux-only probe')
work = Path(sys.argv[1]); work.mkdir(parents=True)
exe = Path(sys.argv[2]).resolve(); fixture = Path(sys.argv[3]).resolve(); tap = Path(sys.argv[4]).resolve()
report = {'probe_sha256': hashlib.sha256(Path(__file__).read_bytes()).hexdigest(),
    'app_sha256': hashlib.sha256(exe.read_bytes()).hexdigest(),
    'driver_sha256': hashlib.sha256(exe.with_name('cua-driver').read_bytes()).hexdigest()}
env = {k: v for k, v in os.environ.items() if not k.startswith('ICU_')}
env.update({key: str(work/name) for key, name in [('XDG_CACHE_HOME', 'cache'), ('XDG_DATA_HOME', 'data'),
    ('XDG_CONFIG_HOME', 'config'), ('XDG_RUNTIME_DIR', 'runtime')]})
env.update(XDG_SESSION_TYPE='x11', GTK_MODULES='gail:atk-bridge', NO_AT_BRIDGE='0', LIBGL_ALWAYS_SOFTWARE='1',
    ICU_BRAIN='ollama', ICU_CUA_BIN=str(tap), ICU_TAP_REAL_DRIVER=str(exe.with_name('cua-driver')), ICU_TAP_RESPONSES='1')
(work/'runtime').mkdir(mode=0o700)
processes = []; logs = []

def launch(args, name, stdout=None):
    log = (work/(name+'.log')).open('wb'); logs.append(log)
    child = subprocess.Popen(args, env=env, stdin=subprocess.DEVNULL, stdout=stdout or log, stderr=log, start_new_session=True)
    processes.append(child); return child

def wait_for(check, seconds, message):
    deadline = time.monotonic()+seconds
    while time.monotonic() < deadline:
        value = check()
        if value: return value
        time.sleep(.05)
    raise RuntimeError(message)

def events():
    path = work/'receiver-events.jsonl'
    return [json.loads(l) for l in path.read_text().splitlines()] if path.exists() else []

def clicks():
    paths = list((work/'cache'/'icu').glob('*.requests.jsonl'))
    rows = [json.loads(l) for p in paths for l in p.read_text().splitlines() if l.strip()]
    return [r for r in rows if (r.get('request') or r).get('name') == 'click']

def responses(since=0.0):
    rows = [json.loads(l) for p in (work/'cache'/'icu').glob('*.responses.jsonl') for l in p.read_text().splitlines() if l.strip()]
    return [((r.get('response') or {}).get('result') or {}) for r in rows if r['time'] >= since]

def issued_ids():
    return {(r.get('structuredContent') or {}).get('screenshot_id') for r in responses()} - {None}

signal.signal(signal.SIGALRM, lambda *_: (_ for _ in ()).throw(TimeoutError('probe deadline'))); signal.alarm(150)
try:
    xvfb = launch(['Xvfb', '-displayfd', '1', '-screen', '0', '1600x1000x24', '-nolisten', 'tcp'], 'xvfb', subprocess.PIPE)
    assert select.select([xvfb.stdout], [], [], 8)[0], 'display startup timeout'
    env['DISPLAY'] = ':'+xvfb.stdout.readline().decode().strip()
    subprocess.run(['dbus-update-activation-environment', 'DISPLAY', 'XDG_RUNTIME_DIR', 'XDG_DATA_HOME', 'XDG_CONFIG_HOME'], env=env, check=True, timeout=5)
    launch(['openbox'], 'openbox'); time.sleep(.7)
    receiver = launch(['/usr/bin/python3', str(fixture), str(work), 'resize'], 'receiver')
    wait_for(lambda: (work/'fixture-ready.json').exists(), 8, 'receiver not ready')
    ready = json.loads((work/'fixture-ready.json').read_text()); target = ready['target']
    app = launch([str(exe)], 'app'); report['app_pid'] = app.pid
    os.environ.update(env)
    import pyatspi
    from gi.repository import Atspi
    Atspi.set_timeout(1500, 3000)
    def nodes():
        queue = [(a, 0) for a in pyatspi.Registry.getDesktop(0) if a.get_process_id() == app.pid]; out = []
        while queue and len(out) < 1500:
            node, depth = queue.pop(); out.append(node)
            try:
                if depth < 18: queue.extend((node.getChildAtIndex(i), depth+1) for i in range(node.childCount))
            except Exception: pass
        return out
    def showing(node):
        try: return node.getState().contains(pyatspi.STATE_SHOWING)
        except Exception: return False
    def find(text, exact=False):
        for node in nodes():
            try:
                name = node.name or ''
                if (name.casefold() == text.casefold() if exact else text.casefold() in name.casefold()) and showing(node): return node
            except Exception: pass
    def press(node):
        for _ in range(10):
            try:
                if node.getRoleName() in ('button', 'push button', 'menu item') and node.queryAction().nActions:
                    assert node.queryAction().doAction(0); return
            except NotImplementedError: pass
            node = node.parent
        raise RuntimeError('no button action')
    def tile_image():
        # WebKitGTK does not expose the alt-less <img>. The tile's shot-wrap
        # hint ("controlling ...") spans the image's width at its bottom, and
        # the image starts below the tile header's CONTROL button.
        hint = control = None
        for node in nodes():
            try:
                if not showing(node): continue
                if (node.name or '') == 'CONTROL': control = node.queryComponent().getExtents(pyatspi.DESKTOP_COORDS)
                try: text = node.queryText().getText(0, 40)
                except Exception: text = ''
                if node.getRoleName() == 'section' and text.startswith('controlling'): hint = node.queryComponent().getExtents(pyatspi.DESKTOP_COORDS)
            except Exception: pass
        if hint is None or control is None: return None
        top = control.y + control.height + 10
        return (hint.x, top, hint.width, hint.y + hint.height - top)
    def xdo(*args): subprocess.run(['xdotool', *args], env=env, check=True, timeout=8)
    press(wait_for(lambda: find('+ window', True), 20, 'picker unavailable'))
    press(wait_for(lambda: find('ICU Drag Receiver'), 10, 'receiver absent from picker'))
    box = wait_for(tile_image, 15, 'tile image not shown')
    time.sleep(1.5)
    box = tile_image(); report['tile'] = list(box)
    center = (box[0] + box[2] // 2, box[1] + box[3] // 2 - 20)

    # 1. A tile click carries the displayed frame's id and lands.
    before = len(events())
    xdo('mousemove', '--sync', str(center[0]), str(center[1]), 'click', '1')
    wait_for(lambda: len(clicks()) >= 1, 5, 'no click request reached the driver')
    wait_for(lambda: len(events()) > before, 5, 'tile click did not reach the receiver')
    first = clicks()[0]; args = (first.get('request') or first).get('args', {})
    down = next(e for e in events()[before:] if e['kind'] == 'down')
    report['first_click'] = {'args': args, 'receiver_down': [down['x'], down['y']]}
    report['checks'] = checks = {
        'click_carries_an_issued_screenshot_id': args.get('screenshot_id') in issued_ids(),
        # The tile shows a 640x400 frame of the 1280x800 receiver.
        'click_lands_at_its_frame_pixel': abs(down['x'] - 2 * args.get('x', -99)) <= 2 and abs(down['y'] - 2 * args.get('y', -99)) <= 2,
    }
    # 2. Resize, then click before the tile shows a new frame.
    attempts = []
    for attempt, size in enumerate([(1000, 800), (1280, 800), (1000, 800)]):
        time.sleep(1.5)
        before_events, before_clicks, t0 = len(events()), len(clicks()), time.time()
        # The pointer is already on the tile; one xdotool run resizes, then clicks.
        xdo('windowsize', str(target), str(size[0]), str(size[1]), 'click', '1')
        wait_for(lambda: len(clicks()) > before_clicks, 5, 'no click request after resize')
        time.sleep(1.0)
        row = clicks()[before_clicks]; sent = (row.get('request') or row).get('args', {})
        codes = [(r.get('structuredContent') or {}).get('code') for r in responses(t0)]
        attempts.append({'size': size, 'args': sent, 'receiver_events': events()[before_events:],
            'refusal': 'stale_capture' if 'stale_capture' in codes else None})
        if not attempts[-1]['receiver_events']: break
    report['stale_attempts'] = attempts
    last = attempts[-1]
    checks['stale_click_refused_without_input'] = not last['receiver_events'] and last['refusal'] == 'stale_capture'
    checks['stale_click_was_bound'] = last['args'].get('screenshot_id') in issued_ids()
    report['ok'] = all(checks.values())
except Exception as error:
    report.update(ok=False, error=str(error), traceback=traceback.format_exc())
    try:
        dump = []
        for node in nodes():
            try:
                box = node.queryComponent().getExtents(pyatspi.DESKTOP_COORDS)
                try: text = node.queryText().getText(0, 120)
                except Exception: text = ''
                dump.append([node.getRoleName(), (node.name or '')[:60], showing(node), [box.x, box.y, box.width, box.height], text])
            except Exception: pass
        report['atspi_at_failure'] = dump
    except Exception: pass
    try:
        import gi
        gi.require_version('Gdk', '3.0')
        from gi.repository import Gdk
        Gdk.pixbuf_get_from_window(Gdk.get_default_root_window(), 0, 0, 1600, 1000).savev(str(work/'failure.png'), 'png', [], [])
    except Exception: pass
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
    (work/'result.json').write_text(json.dumps(report, indent=2, default=str)+'\n')
    print(json.dumps({'checks': report.get('checks'), 'attempts': [(a['size'], a['refusal'], len(a['receiver_events'])) for a in report.get('stale_attempts', [])]}, default=str))
    print('ok' if report.get('ok') else 'FAIL', report.get('error', ''))
    sys.exit(0 if report.get('ok') else 1)
