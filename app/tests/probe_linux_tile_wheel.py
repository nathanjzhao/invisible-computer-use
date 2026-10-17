"""The installed app's live tile wheel scrolls the bound window, not a decoy, and names the frame it read.

    probe_linux_tile_wheel.py WORK /usr/bin/app RECEIVER TAP [overlap|apart]

RECEIVER is fixtures/x11_scroll_receiver.py (a bound `ICU Scroll Target` and an `ICU Scroll Decoy`, both
scrollable, each logging its own wheel presses); TAP is driver_socket_tap.py, which the app starts as its driver
(ICU_CUA_BIN) so every app->driver request and every frame id the driver issues is logged. No model is involved:
the tile comes from the app's own `+ window` picker (ICU_BRAIN=ollama points the brain at a port nobody serves).

With `overlap` the decoy sits at the target's screen place, stacked in front of it. The probe opens the target's
tile, then turns the real mouse wheel (XTest buttons 5 then 4) over the tile image and checks:

 1. the target received the wheel presses (down then up: its offset goes +N then back to 0), the decoy none;
 2. every `scroll` request the app sent the driver names the target window and a `screenshot_id` the driver issued;
Exit 0 only when all of that holds. Not covered: a wheel while the tile is blocked (off-Space), which is the Mac path.
"""
import hashlib
import json
import os
from pathlib import Path
import select
import shutil
import signal
import subprocess
import sys
import time
import traceback

if sys.platform != 'linux': raise SystemExit('Linux-only probe')
work = Path(sys.argv[1]); work.mkdir(parents=True)
exe = Path(sys.argv[2]).resolve(); fixture = Path(sys.argv[3]).resolve(); tap = Path(sys.argv[4]).resolve()
mode = sys.argv[5] if len(sys.argv) > 5 else 'overlap'
report = {'mode': mode, 'probe_sha256': hashlib.sha256(Path(__file__).read_bytes()).hexdigest(),
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

def requests(name):
    rows = [json.loads(l) for p in (work/'cache'/'icu').glob('*.requests.jsonl') for l in p.read_text().splitlines() if l.strip()]
    return [r.get('request') or r for r in rows if (r.get('request') or r).get('name') == name]

def responses():
    rows = [json.loads(l) for p in (work/'cache'/'icu').glob('*.responses.jsonl') for l in p.read_text().splitlines() if l.strip()]
    return [((r.get('response') or {}).get('result') or {}) for r in rows]

def issued_ids():
    return {(r.get('structuredContent') or {}).get('screenshot_id') for r in responses()} - {None}

def shot(name):
    subprocess.run(['import', '-display', env['DISPLAY'], '-window', 'root', str(work/name)], env=env, check=True, timeout=15)

signal.signal(signal.SIGALRM, lambda *_: (_ for _ in ()).throw(TimeoutError('probe deadline'))); signal.alarm(150)
try:
    xvfb = launch(['Xvfb', '-displayfd', '1', '-screen', '0', '1600x1000x24', '-nolisten', 'tcp'], 'xvfb', subprocess.PIPE)
    assert select.select([xvfb.stdout], [], [], 8)[0], 'display startup timeout'
    env['DISPLAY'] = ':'+xvfb.stdout.readline().decode().strip()
    subprocess.run(['dbus-update-activation-environment', 'DISPLAY', 'XDG_RUNTIME_DIR', 'XDG_DATA_HOME', 'XDG_CONFIG_HOME'], env=env, check=True, timeout=5)
    launch(['openbox'], 'openbox')
    # A compositor lets the driver capture the target while the decoy covers it (without one an X capture of a covered window is black).
    if shutil.which('picom'): launch(['picom', '--backend', 'xrender'], 'picom')
    time.sleep(.7)
    receiver = launch(['/usr/bin/python3', str(fixture), str(work), mode], 'receiver')
    wait_for(lambda: (work/'fixture-ready.json').exists(), 8, 'receiver not ready')
    ready = json.loads((work/'fixture-ready.json').read_text()); report['fixture'] = ready
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
        # WebKitGTK does not expose the alt-less <img>. The tile's shot-wrap hint ("controlling ...") spans the
        # image's width at its bottom, and the image starts below the tile header's CONTROL button.
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
    def xdo(*args): subprocess.run(['xdotool', *args], env=env, check=True, timeout=12)
    press(wait_for(lambda: find('+ window', True), 25, 'picker unavailable'))
    press(wait_for(lambda: find('ICU Scroll Target'), 10, 'target absent from picker'))
    box = wait_for(tile_image, 15, 'tile image not shown')
    time.sleep(1.5)
    box = tile_image(); report['tile'] = list(box)
    center = (box[0] + box[2] // 2, box[1] + box[3] // 2 - 20)
    shot('before.png')
    assert not events(), 'receiver already scrolled before the gesture'

    N = 5
    xdo('mousemove', '--sync', str(center[0]), str(center[1]))
    # Wheel down N presses (button 5), 200 ms apart (the tile throttles to one request per 80 ms), then up N (button 4).
    xdo('click', '--repeat', str(N), '--delay', '200', '5')
    wait_for(lambda: len([e for e in events() if e['button'] == 5]) >= 1, 6, 'no wheel press reached any receiver')
    time.sleep(1.0)
    down = events(); shot('after-down.png')
    xdo('click', '--repeat', str(N), '--delay', '200', '4')
    time.sleep(1.5)
    shot('after-up.png')
    ev = events(); report['receiver_events'] = ev
    scrolls = requests('scroll'); report['scroll_requests'] = scrolls
    tgt = [e for e in ev if e['window'] == 'target']; dec = [e for e in ev if e['window'] == 'decoy']
    ids = issued_ids(); report['issued_ids'] = sorted(ids)
    sargs = [(s.get('args') or {}) for s in scrolls]
    report['checks'] = checks = {
        'target_scrolled_down': any(e['button'] == 5 for e in tgt),
        'target_scrolled_back_up': any(e['button'] == 4 for e in tgt),
        'target_offset_peaked_positive': max([e['offset'] for e in tgt] or [0]) >= 1,
        'decoy_received_nothing': not dec,
        'scroll_requests_sent': len(scrolls) >= 1,
        'every_scroll_names_the_target_window': bool(sargs) and all(a.get('window_id') == ready['target'] for a in sargs),
        'every_scroll_carries_an_issued_screenshot_id': bool(sargs) and all(a.get('screenshot_id') in ids for a in sargs),
    }
    report['ok'] = all(checks.values())
except Exception as error:
    report.update(ok=False, error=str(error), traceback=traceback.format_exc())
    try: report['receiver_events'] = events()
    except Exception: pass
    try: shot('failure.png')
    except Exception: pass
    try:
        dump = []
        for node in nodes():
            try:
                bx = node.queryComponent().getExtents(pyatspi.DESKTOP_COORDS)
                try: text = node.queryText().getText(0, 120)
                except Exception: text = ''
                dump.append([node.getRoleName(), (node.name or '')[:60], showing(node), [bx.x, bx.y, bx.width, bx.height], text])
            except Exception: pass
        report['atspi_at_failure'] = dump
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
    print(json.dumps({'mode': mode, 'checks': report.get('checks')}, default=str))
    print('ok' if report.get('ok') else 'FAIL', report.get('error', ''))
    sys.exit(0 if report.get('ok') else 1)
