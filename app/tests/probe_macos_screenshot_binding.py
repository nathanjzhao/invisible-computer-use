"""Screenshot binding on macOS: a pixel action bound to a `screenshot_id` is
checked against that capture's window size and backing scale, not re-measured
at click time.

    probe_macos_screenshot_binding.py WORK CUA_DRIVER MINICALC MACOS_UI DISPLAYMODE

Run inside the console user's GUI session on a disposable Mac (it posts real
clicks and switches the display mode). The driver runs as its own daemon on a
private socket. MINICALC is fixtures/minicalc_mac in `receiver` mode: its
Click Pad (200x220 pt, 240 pt from the content's left, 60 pt above its bottom,
no autoresizing) logs every mouse-down in pad points. The agent aims at pad
point (100, 110).
"""
import base64, hashlib, json, pathlib, struct, subprocess, sys, time, traceback
sys.path.insert(0, str(pathlib.Path(__file__).resolve().parent))
from macos_probe import protocol, wait_for

work = pathlib.Path(sys.argv[1]); work.mkdir(parents=True)
driver, minicalc, ui_bin, displaymode = (pathlib.Path(a).resolve() for a in sys.argv[2:6])
report = {'probe_sha256': hashlib.sha256(pathlib.Path(__file__).read_bytes()).hexdigest(),
          'driver_sha256': hashlib.sha256(driver.read_bytes()).hexdigest(), 'cases': {}}
processes = []; logs = []
PAD_TARGET = (100, 110)
EXPECTED = {
    'bound_click_after_resize': 'refused:stale_capture',
    'bound_drag_after_resize': 'refused:stale_capture',
    'unbound_click_after_resize_without_recapture': 'refused:stale_capture',
    'fresh_id_click_after_resize': 'pad:100,110',
    'bound_click_after_move': 'pad:100,110',
    'bound_click_after_backing_scale_change': 'refused:stale_capture',
    'fresh_id_click_at_new_scale': 'pad:100,110',
}

def launch(args, name):
    f = open(work/(name+'.log'), 'wb'); logs.append(f)
    p = subprocess.Popen([str(a) for a in args], stdin=subprocess.DEVNULL, stdout=f, stderr=f, start_new_session=True)
    processes.append(p); return p

def ui(*args):
    out = subprocess.run([str(ui_bin), *map(str, args)], capture_output=True, text=True, timeout=30)
    return json.loads(out.stdout.strip().splitlines()[-1]) if out.stdout.strip() else {'ok': False, 'error': out.stderr}

def approve_capture_prompts(seconds=2):
    """macOS 15 asks once per process before a direct window capture. Approving
    it is the operator's setup decision; record it and approve nothing else."""
    end = time.monotonic()+seconds
    while time.monotonic() < end:
        for pid in {w['owner'] for w in ui('onscreen').get('windows', []) if w.get('owner_name') == 'UserNotificationCenter'}:
            texts = ' '.join(n.get('name', '') for n in ui('tree', pid).get('nodes', []) if n.get('role') == 'AXStaticText')
            if 'bypass the system private window picker' in texts:
                ui('press', pid, json.dumps({'role': 'AXButton', 'name': 'Allow'}))
                report.setdefault('capture_prompts', []).append(texts[:160])
        time.sleep(.3)

def pad_events():
    p = work/'pointer-events.jsonl'
    return [json.loads(l) for l in p.read_text().splitlines() if l.strip()] if p.exists() else []

try:
    report['display_mode_at_start'] = json.loads(subprocess.run([str(displaymode), 'get'], capture_output=True, text=True).stdout)['mode']
    if report['display_mode_at_start']['scale'] != 1:
        subprocess.run([str(displaymode), 'set', '1920', '1200', '1'], check=True, timeout=20); time.sleep(3)
    receiver = launch([minicalc, work, 'receiver'], 'receiver')
    wait_for((work/'fixture-receiver-ready.json').exists, 15, 'receiver not ready')
    sock = work/'driver.sock'
    launch([driver, 'serve', '--socket', sock, '--no-permissions-gate', '--no-overlay'], 'driver')
    wait_for(sock.exists, 30, 'driver socket missing'); time.sleep(.5)
    def call(name, args, timeout=20):
        return protocol(sock, {'method': 'call', 'name': name, 'args': args, 'session_id': 'mac-binding', 'client_kind': 'cli'}, timeout)
    windows = call('list_windows', {})['structuredContent']['windows']
    wid = next(w['window_id'] for w in windows if w.get('pid') == receiver.pid and w.get('title') == 'ICU Manual Receiver')
    report['window_id'] = wid

    def frame(x, y, w, h):
        r = call('set_window_frame', {'pid': receiver.pid, 'window_id': wid, 'x': x, 'y': y, 'width': w, 'height': h})
        time.sleep(.8); return r.get('structuredContent')

    def capture(label):
        r = call('get_window_state', {'pid': receiver.pid, 'window_id': wid, 'include_screenshot': True, 'include_accessibility_tree': False})
        approve_capture_prompts(1)
        sc = r['structuredContent']
        assert not r.get('isError') and sc.get('screenshot_frame_valid'), sc
        png = base64.b64decode(next(c['data'] for c in r['content'] if c.get('type') == 'image'))
        (work/f'{label}.png').write_bytes(png)
        b = sc['window_bounds']; scale = sc['screenshot_scale']
        # Pad target in window points (top-left origin), then capture pixels.
        pad_left, pad_top = 240, b['height'] - 60 - 220
        px = [(pad_left + PAD_TARGET[0]) * scale, (pad_top + PAD_TARGET[1]) * scale]
        return {'screenshot_id': sc.get('screenshot_id'), 'size_px': list(struct.unpack('>II', png[16:24])),
                'bounds': b, 'scale': scale, 'target_px': px}

    def act(case, tool, shot, screenshot_id):
        x, y = shot['target_px']
        args = {'pid': receiver.pid, 'window_id': wid, 'delivery_mode': 'background'}
        args.update({'from_x': x, 'from_y': y, 'to_x': x + 10, 'to_y': y} if tool == 'drag' else {'x': x, 'y': y})
        if screenshot_id is not None: args['screenshot_id'] = screenshot_id
        before = len(pad_events()); r = call(tool, args); time.sleep(1.0)
        seen = [e for e in pad_events()[before:] if e['kind'] == 'down']
        sc = r.get('structuredContent') or {}
        text = ''.join(c.get('text', '') for c in r.get('content', []))
        if r.get('isError'):
            outcome = 'refused:' + str(sc.get('code') or 'untyped')
            if seen: outcome = 'refused_but_delivered'
        elif seen:
            outcome = f"pad:{round(seen[0]['x'])},{round(seen[0]['y'])}"
        else:
            outcome = 'delivered_off_pad'
        live = next(w for w in call('list_windows', {})['structuredContent']['windows'] if w.get('window_id') == wid)['bounds']
        report['cases'][case] = {'tool': tool, 'args': args, 'window_now': live, 'is_error': bool(r.get('isError')),
            'code': sc.get('code'), 'text': text[:400], 'pad_downs': seen, 'outcome': outcome,
            'expected': EXPECTED[case], 'pass': outcome == EXPECTED[case]}

    # 1. Captured at 460x448, resized taller: the pad moves down 100 pt, so the
    # agent's pixel now names a point 100 pt higher on the pad.
    frame(100, 120, 460, 448); a = capture('agent-resize'); report['agent_resize'] = a
    frame(100, 120, 460, 548)
    act('bound_click_after_resize', 'click', a, a['screenshot_id'])
    act('bound_drag_after_resize', 'drag', a, a['screenshot_id'])
    act('unbound_click_after_resize_without_recapture', 'click', a, None)
    fresh = capture('fresh-resize'); report['fresh_resize'] = fresh
    act('fresh_id_click_after_resize', 'click', fresh, fresh['screenshot_id'])
    # 2. A move keeps window-local pixels valid.
    m = capture('agent-move'); frame(300, 200, 460, 548)
    act('bound_click_after_move', 'click', m, m['screenshot_id'])
    # 3. Captured at 1x, display switched to 2x: same point size, half the pixel density.
    s1 = capture('agent-1x'); report['agent_1x'] = s1
    subprocess.run([str(displaymode), 'set', '1280', '800', '2'], check=True, timeout=20); time.sleep(4)
    frame(100, 120, 460, 548)
    act('bound_click_after_backing_scale_change', 'click', s1, s1['screenshot_id'])
    s2 = capture('fresh-2x'); report['fresh_2x'] = s2
    act('fresh_id_click_at_new_scale', 'click', s2, s2['screenshot_id'])
    report['ok'] = all(c['pass'] for c in report['cases'].values()) and len(report['cases']) == len(EXPECTED)
except Exception as error:
    report.update(ok=False, error=str(error), traceback=traceback.format_exc())
finally:
    try: subprocess.run([str(displaymode), 'set', '1920', '1200', '1'], timeout=20)
    except Exception: pass
    for p in reversed(processes):
        if p.poll() is None: p.terminate()
    for p in reversed(processes):
        try: p.wait(timeout=5)
        except subprocess.TimeoutExpired: p.kill(); p.wait()
    for f in logs: f.close()
    (work/'result.json').write_text(json.dumps(report, indent=2) + '\n')
    print(json.dumps({k: v['outcome'] for k, v in report['cases'].items()}, indent=1))
    print('ok' if report.get('ok') else 'FAIL', report.get('error', ''))
    sys.exit(0 if report.get('ok') else 1)
