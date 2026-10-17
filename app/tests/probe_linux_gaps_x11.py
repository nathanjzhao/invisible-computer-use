"""Linux gaps review (2026-10-04), X11: pixel-tool cases the earlier probes did not run.

    probe_linux_gaps_x11.py WORK CUA_DRIVER RECEIVER [WM]

RECEIVER is app/tests/fixtures/x11_gaps_receiver.py (T 800x600, S 400x300, one process).
WM is the window manager to run under Xvfb (default openbox; mutter works too).

Sections:
  E  S (400x300) captured at full size: the pixel one past the last pixel (x == width,
     y == height) on every pixel tool refuses invalid_arguments with no input (the
     round-4 probe checked 401, not 400); a fraction below the edge (399.6) is pixel 399.
  V  S moved (not resized) since the capture: an unbound click lands at the same
     window-local pixel; S resized and resized back to its captured size: still lands.
  Z  S captured at max_dimension 200 (the screenshot is half size). A zoom region is
     given in screenshot pixels (the tool's own description), so the zoom image's centre
     is the region's centre in window pixels: from_zoom click and mouse_button_down land
     there. Control: the same region at full size lands at the same window pixel.
  X  from_zoom on double_click and right_click translates through the zoom like click (the
     zoom image's pixel (10, 10) is the crop origin plus 10); scroll has no from_zoom and
     refuses invalid_arguments with no input.
  R  S's process is killed and a new process (new pid) gets a window under the same XID: the
     old screenshot_id, an unbound pixel and the dead pid all refuse with no input; a fresh capture lands.
Every case records the reply, its duration and the exact button events. Exit 0 only when
every case matches EXPECTED.
"""
import hashlib, json, os, pathlib, select, signal, socket, subprocess, sys, time, traceback

work = pathlib.Path(sys.argv[1]); work.mkdir(parents=True)
driver = pathlib.Path(sys.argv[2]).resolve(); recv = pathlib.Path(sys.argv[3]).resolve()
WM = sys.argv[4] if len(sys.argv) > 4 else 'openbox'
procs = []; logs = []
report = {'driver_sha256': hashlib.sha256(driver.read_bytes()).hexdigest(),
          'probe_sha256': hashlib.sha256(pathlib.Path(__file__).read_bytes()).hexdigest(), 'wm': WM, 'cases': {}}
env = dict(os.environ); env.update({'XDG_RUNTIME_DIR': str(work/'runtime'), 'XDG_SESSION_TYPE': 'x11'}); env.pop('WAYLAND_DISPLAY', None)
(work/'runtime').mkdir(); (work/'runtime').chmod(0o700)
REFUSED = 'refused'
EXPECTED = {}
def expect(case, outcome, where=None): EXPECTED[case] = (outcome, where)
def click(x, y, b=1): return [('r', 'S', 'down', b, x, y), ('r', 'S', 'up', b, x, y)]
# E
for tool in ('click', 'double_click', 'right_click', 'scroll', 'mouse_button_down'):
    for label in ('x_eq_width', 'y_eq_height'):
        expect(f'e_{tool}_{label}', 'invalid_arguments')
expect('e_drag_to_x_eq_width', 'invalid_arguments')
expect('e_drag_from_y_eq_height', 'invalid_arguments')
expect('e_click_fraction_below_edge', 'delivered', click(399, 299))
# V
expect('v_click_after_move', 'delivered', click(50, 50))
expect('v_click_after_resize_and_back', 'delivered', click(60, 60))
# Z
expect('z_control_full_from_zoom_click', 'delivered', click(200, 150))
expect('z_down_from_zoom_full', 'delivered', [('r', 'S', 'down', 1, 200, 150)])
expect('z_half_from_zoom_click', 'delivered', click(200, 150))
expect('z_half_from_zoom_down', 'delivered', [('r', 'S', 'down', 1, 200, 150)])
# X
expect('x_double_click_from_zoom', 'delivered', click(70, 90) + click(70, 90))
expect('x_right_click_from_zoom', 'delivered', click(70, 90, 3))
expect('x_scroll_from_zoom', 'invalid_arguments')
# R
expect('r_old_id_new_pid', REFUSED)
expect('r_unbound_new_pid', 'ambiguous_capture')
expect('r_old_pid_dead', REFUSED)
expect('r_fresh_capture_lands', 'delivered', [('r2', 'S', 'down', 1, 50, 50), ('r2', 'S', 'up', 1, 50, 50)])
SLOW = 20

def launch(args, name, stdout=None):
    log = (work/(name+'.log')).open('wb'); logs.append(log)
    p = subprocess.Popen(args, env=env, stdin=subprocess.DEVNULL, stdout=stdout or log, stderr=log, start_new_session=True); procs.append(p); return p
def wait_for(pred, s, msg):
    d = time.monotonic() + s
    while time.monotonic() < d:
        r = pred()
        if r: return r
        time.sleep(.05)
    raise RuntimeError(msg)
def protocol(path, req):
    with socket.socket(socket.AF_UNIX) as s:
        s.settimeout(60); s.connect(str(path)); s.sendall((json.dumps(req)+'\n').encode()); data = b''
        while b'\n' not in data:
            ch = s.recv(1 << 20)
            if not ch: raise ConnectionError('closed')
            data += ch
    r = json.loads(data.split(b'\n')[0]); assert r.get('ok'), r; return r['result']
def events(tag):
    p = work/f'{tag}-events.jsonl'
    return [json.loads(l) for l in p.read_text().splitlines()] if p.exists() else []
def ready(tag): return json.loads((work/f'{tag}-ready.json').read_text())
def send(tag, op):
    n = len([e for e in events(tag) if e.get('kind') == 'applied'])
    (work/f'{tag}.cmd').write_text(op)
    wait_for(lambda: len([e for e in events(tag) if e.get('kind') == 'applied']) > n, 5, f'{op} not applied'); time.sleep(.5)
signal.signal(signal.SIGALRM, lambda *_: (_ for _ in ()).throw(TimeoutError('deadline'))); signal.alarm(900)
try:
    x = launch(['Xvfb', '-displayfd', '1', '-screen', '0', '1600x1000x24', '-nolisten', 'tcp'], 'xvfb', subprocess.PIPE)
    assert select.select([x.stdout], [], [], 8)[0]; env['DISPLAY'] = ':' + x.stdout.readline().decode().strip()
    launch(WM.split(), 'wm'); time.sleep(1.5)
    recv_proc = launch(['/usr/bin/python3', str(recv), str(work), 'r'], 'recv-r')
    wait_for(lambda: (work/'r-ready.json').exists(), 8, 'receiver')
    RX = {'r': ready('r')}; report['receiver'] = dict(RX)
    sock = work/'driver.sock'; launch([str(driver), 'serve', '--socket', str(sock)], 'driver')
    wait_for(sock.exists, 20, 'driver socket'); time.sleep(.3)
    def call(name, args, session='agent'): return protocol(sock, {'method': 'call', 'name': name, 'args': args, 'session_id': session, 'client_kind': 'cli'})
    call('set_agent_cursor_enabled', {'session': 'agent', 'enabled': False}, 'agent')
    def cap(win, maxdim=4000):
        r = call('get_window_state', {'pid': RX['r']['pid'], 'window_id': RX['r'][win], 'include_screenshot': True,
                                      'include_accessibility_tree': False, 'max_dimension': maxdim})
        sc = r.get('structuredContent') or {}; assert sc.get('screenshot_id'), r
        return sc['screenshot_id'], (sc.get('screenshot_width'), sc.get('screenshot_height'))
    def name_of(wid, tag='r'): return next((k for k in ('T', 'S') if RX[tag][k] == wid), wid)
    def act(case, tool, win, args, tag='r', pid=None):
        full = {'pid': pid or RX[tag]['pid'], 'delivery_mode': 'background', 'window_id': RX[tag][win], **args}
        b = len(events(tag)); t0 = time.monotonic()
        try: r = call(tool, full)
        except (socket.timeout, TimeoutError) as e: r = {'isError': True, 'structuredContent': {'code': 'probe_timeout'}, 'content': [{'text': str(e)}]}
        took = round(time.monotonic() - t0, 2); time.sleep(.6)
        seen = [(tag, name_of(e['window'], tag), e['kind'], e.get('button'), e.get('x'), e.get('y')) for e in events(tag)[b:] if e.get('kind') in ('down', 'up')]
        sc = r.get('structuredContent') or {}
        code = sc.get('code') or (sc.get('refusal') or {}).get('code')
        outcome = (code or 'error') if r.get('isError') else 'delivered'
        want, where = EXPECTED[case]
        if want == REFUSED: ok = bool(r.get('isError')) and not seen
        elif want == 'delivered': ok = outcome == 'delivered' and (where is None or seen == where)
        else: ok = outcome == want and not seen
        if want != 'delivered' and took > SLOW: ok = False
        report['cases'][case] = {'tool': tool, 'args': full, 'is_error': bool(r.get('isError')), 'code': code,
            'text': ''.join(c.get('text', '') for c in r.get('content', []))[:300], 'input': seen, 'expected': [want, where], 'ok': ok, 'seconds': took}
        print('PASS' if ok else 'FAIL', case, outcome, code, seen[:4], flush=True)
        return r
    def release(): call('mouse_button_up', {'pid': RX['r']['pid'], 'window_id': RX['r']['S'], 'delivery_mode': 'background'}); time.sleep(.3)
    def zoom(win, region):
        z = call('zoom', {'pid': RX['r']['pid'], 'window_id': RX['r'][win], **dict(zip(('x1', 'y1', 'x2', 'y2'), region))})
        sc = z.get('structuredContent') or {}; assert not z.get('isError'), z
        return sc['width'], sc['height']

    # ── E ──
    s_id, size = cap('S'); report['e_size'] = size
    W, H = size
    for tool in ('click', 'double_click', 'right_click', 'scroll', 'mouse_button_down'):
        extra = {'direction': 'down', 'amount': 1} if tool == 'scroll' else {}
        act(f'e_{tool}_x_eq_width', tool, 'S', {'x': W, 'y': 10, 'screenshot_id': s_id, **extra})
        if tool == 'mouse_button_down': release()
        act(f'e_{tool}_y_eq_height', tool, 'S', {'x': 10, 'y': H, 'screenshot_id': s_id, **extra})
        if tool == 'mouse_button_down': release()
    dg = {'from_x': 10, 'from_y': 10, 'to_x': 40, 'to_y': 10, 'duration_ms': 50, 'steps': 2, 'screenshot_id': s_id}
    act('e_drag_to_x_eq_width', 'drag', 'S', {**dg, 'to_x': W})
    act('e_drag_from_y_eq_height', 'drag', 'S', {**dg, 'from_y': H})
    act('e_click_fraction_below_edge', 'click', 'S', {'x': W - 0.4, 'y': H - 0.4, 'screenshot_id': s_id})

    # ── V ── (agent's own capture from E, unbound)
    send('r', 'move S 700 50'); report['v_moved'] = True
    act('v_click_after_move', 'click', 'S', {'x': 50, 'y': 50})
    send('r', 'resize S 500 400'); send('r', 'resize S 400 300')
    cap('S')  # a resize since the capture refuses; the probe recaptures as a model would, then resizes back
    send('r', 'resize S 500 400'); send('r', 'resize S 400 300')
    act('v_click_after_resize_and_back', 'click', 'S', {'x': 60, 'y': 60})

    # ── Z ── full size
    cap('S', 4000)
    zw, zh = zoom('S', (100, 100, 300, 200)); report['z_full_zoom'] = (zw, zh)
    act('z_control_full_from_zoom_click', 'click', 'S', {'x': zw / 2, 'y': zh / 2, 'from_zoom': True})
    act('z_down_from_zoom_full', 'mouse_button_down', 'S', {'x': zw / 2, 'y': zh / 2, 'from_zoom': True}); release()
    # half size
    half, hsize = cap('S', 200); report['z_half_size'] = hsize
    zw, zh = zoom('S', (50, 50, 150, 100)); report['z_half_zoom'] = (zw, zh)
    act('z_half_from_zoom_click', 'click', 'S', {'x': zw / 2, 'y': zh / 2, 'from_zoom': True})
    act('z_half_from_zoom_down', 'mouse_button_down', 'S', {'x': zw / 2, 'y': zh / 2, 'from_zoom': True}); release()
    # ── X ──
    for tool in ('double_click', 'right_click', 'scroll'):
        extra = {'direction': 'down', 'amount': 1} if tool == 'scroll' else {}
        act(f'x_{tool}_from_zoom', tool, 'S', {'x': 10, 'y': 10, 'from_zoom': True, **extra})
    # ── R ── a new process under the old XID
    old_pid, old_s = RX['r']['pid'], RX['r']['S']
    r_id, _ = cap('S')
    os.killpg(recv_proc.pid, signal.SIGKILL); recv_proc.wait(); time.sleep(1)
    launch(['/usr/bin/python3', str(recv), str(work), 'r2'], 'recv-r2')
    wait_for(lambda: (work/'r2-ready.json').exists(), 8, 'receiver r2')
    RX['r2'] = ready('r2'); report['r_same_xid'] = RX['r2']['S'] == old_s; report['r_pids'] = [old_pid, RX['r2']['pid']]
    act('r_old_id_new_pid', 'click', 'S', {'x': 50, 'y': 50, 'screenshot_id': r_id}, tag='r2')
    act('r_unbound_new_pid', 'click', 'S', {'x': 50, 'y': 50}, tag='r2')
    act('r_old_pid_dead', 'click', 'S', {'x': 50, 'y': 50, 'screenshot_id': r_id}, tag='r2', pid=old_pid)
    RX2_pid = RX['r2']['pid']
    r2_id = call('get_window_state', {'pid': RX2_pid, 'window_id': RX['r2']['S'], 'include_screenshot': True, 'include_accessibility_tree': False, 'max_dimension': 4000})['structuredContent']['screenshot_id']
    act('r_fresh_capture_lands', 'click', 'S', {'x': 50, 'y': 50, 'screenshot_id': r2_id}, tag='r2')
    report['done'] = True
except Exception as e:
    report.update(error=str(e), traceback=traceback.format_exc())
finally:
    signal.alarm(0)
    for p in reversed(procs):
        if p.poll() is None:
            try: os.killpg(p.pid, signal.SIGTERM)
            except ProcessLookupError: pass
    for p in reversed(procs):
        try: p.wait(timeout=3)
        except subprocess.TimeoutExpired: os.killpg(p.pid, signal.SIGKILL)
    for l in logs: l.close()
    failures = sorted(k for k, v in report['cases'].items() if not v['ok']) + sorted(set(EXPECTED) - set(report['cases']))
    if report.get('r_same_xid') is False: failures.append('precondition_r_same_xid_not_reused')
    report['failures'] = failures; report['ok'] = bool(report.get('done')) and not failures
    (work/'result.json').write_text(json.dumps(report, indent=1) + '\n')
    print('RESULT', 'ok' if report['ok'] else f'fail {failures}', report.get('error', ''))
    sys.exit(0 if report['ok'] else 1)
