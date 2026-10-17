"""Round-3 review items on X11: an unbound pixel is read against the calling
session's own capture, never another session's (C9, as unified in c9-unify), the hold tools bind like the other pixel
tools, mouse_button_up without a point releases where it pressed (N1), the hold
tools refuse a closed window and drop the hold (N2), scroll coordinates must be
two numbers (N3, N5), and the documented limit of the XID generation mark (O3:
another X client can copy it).

    probe_linux_review3_x11.py WORK CUA_DRIVER RECEIVER

RECEIVER is fixtures/x11_edge_receiver.py, run twice as processes R and F
(target T 800x600, sibling S 400x300 each), logging every button event with
its window and window-local point. The driver runs as its own daemon on a
private socket; every call is background. Two sessions capture: `agent` at
max_dimension 4000 (T unscaled) and `tile` at 200 (T returned 200x150, ratio
4). The probe exits 0 only when every case matches EXPECTED.
"""
import hashlib, json, os, pathlib, select, signal, socket, subprocess, sys, time, traceback

if sys.platform != 'linux': raise SystemExit('Linux-only probe')
work = pathlib.Path(sys.argv[1]); work.mkdir(parents=True)
driver = pathlib.Path(sys.argv[2]).resolve(); recv = pathlib.Path(sys.argv[3]).resolve()
procs = []; logs = []
report = {'driver_sha256': hashlib.sha256(driver.read_bytes()).hexdigest(),
          'probe_sha256': hashlib.sha256(pathlib.Path(__file__).read_bytes()).hexdigest(),
          'receiver_sha256': hashlib.sha256(recv.read_bytes()).hexdigest(), 'cases': {}}
env = dict(os.environ); env.update({'XDG_RUNTIME_DIR': str(work/'runtime'), 'XDG_SESSION_TYPE': 'x11'}); env.pop('WAYLAND_DISPLAY', None)
(work/'runtime').mkdir(); (work/'runtime').chmod(0o700)
# case -> (outcome, input). outcome: a refusal code, 'delivered', or 'error'
# (an error with no code). input: the exact (receiver, window, kind, button,
# x, y) button events, window named 'T'/'S', or None for "none at all".
# 'held' cases also require the reply's hold state.
EXPECTED = {
    # C9: the agent's (100,100) read off its 800x600 image; the tile's 200px
    # capture must not rescale it to (400,400). Since c9-unify an unbound pixel
    # is read against the calling session's own capture, so the agent's lands
    # where it read (it used to refuse ambiguous_capture).
    'c9_unbound_click_after_tile': ('delivered', [('r', 'T', 'down', 1, 100, 100), ('r', 'T', 'up', 1, 100, 100)]),
    'c9_bound_click_after_tile': ('delivered', [('r', 'T', 'down', 1, 100, 100), ('r', 'T', 'up', 1, 100, 100)]),
    'c9_unbound_scroll_after_tile': ('delivered', [('r', 'T', 'down', 5, 100, 100), ('r', 'T', 'up', 5, 100, 100)]),
    'c9_bound_scroll_after_tile': ('delivered', [('r', 'T', 'down', 5, 100, 100), ('r', 'T', 'up', 5, 100, 100)]),
    'c9_unbound_mouse_button_down_after_tile': ('delivered', [('r', 'T', 'down', 1, 100, 100)]),
    'c9_bound_mouse_button_down_after_tile': ('delivered', [('r', 'T', 'down', 1, 100, 100)]),
    'c9_unbound_mouse_drag_after_tile': ('delivered', []),
    'c9_bound_mouse_drag_after_tile': ('delivered', []),
    'c9_mouse_button_up_no_point': ('delivered', [('r', 'T', 'up', 1, 150, 100)]),
    # N1: S captured only at max_dimension 100 (ratio 4). (25,25) presses at
    # (100,100); a release with no point is at the press, not scaled again.
    'n1_mouse_button_down_scaled': ('delivered', [('r', 'S', 'down', 1, 100, 100)]),
    'n1_mouse_button_up_no_point': ('delivered', [('r', 'S', 'up', 1, 100, 100)]),
    # N2: the held window closes; drag and up refuse typed and drop the hold.
    'n2_hold_down_on_sibling': ('delivered', [('r', 'S', 'down', 1, 40, 40)]),
    'n2_drag_after_window_closed': ('stale_window_target', None),
    'n2_down_after_dropped_hold': ('delivered', [('r', 'T', 'down', 1, 10, 10)]),
    'n2_up_after_dropped_hold': ('delivered', [('r', 'T', 'up', 1, 10, 10)]),
    'n2_hold_down_on_f_sibling': ('delivered', [('f', 'S', 'down', 1, 40, 40)]),
    'n2_up_after_window_closed': ('stale_window_target', None),
    # N3, N5: coordinates are two numbers or nothing.
    'n3_scroll_string_coords_no_id': ('invalid_arguments', None),
    'n3_scroll_string_coords_with_id': ('invalid_arguments', None),
    'n5_scroll_x_only': ('invalid_arguments', None),
    'n3_mouse_button_up_string_coords': ('invalid_arguments', None),
    # O3, documented X11 limit: another client copies the old mark onto the
    # recreated window and the old id is accepted. The control refuses.
    'o3_old_id_on_recreated_window': ('stale_capture', None),
    'o3_old_id_after_mark_copied': ('delivered', [('r', 'T', 'down', 1, 100, 100), ('r', 'T', 'up', 1, 100, 100)]),
}
# A refusal on a closed window must report the hold dropped. (A delivered
# hold call's reply is the action record; the next press proves the state.)
HELD = {'n2_drag_after_window_closed': False, 'n2_up_after_window_closed': False}
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
        s.settimeout(20); s.connect(str(path)); s.sendall((json.dumps(req)+'\n').encode()); data = b''
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
def set_mark(wid, words):
    from Xlib import display, Xatom
    d = display.Display(env['DISPLAY']); w = d.create_resource_object('window', wid)
    w.change_property(d.intern_atom('_CUA_DRIVER_CAPTURE_GENERATION'), Xatom.CARDINAL, 32, words); d.sync(); d.close()
def get_mark(wid):
    from Xlib import display, Xatom
    d = display.Display(env['DISPLAY']); w = d.create_resource_object('window', wid)
    p = w.get_full_property(d.intern_atom('_CUA_DRIVER_CAPTURE_GENERATION'), Xatom.CARDINAL); d.close()
    return list(p.value) if p else None
signal.signal(signal.SIGALRM, lambda *_: (_ for _ in ()).throw(TimeoutError('deadline'))); signal.alarm(300)
try:
    x = launch(['Xvfb', '-displayfd', '1', '-screen', '0', '1600x1000x24', '-nolisten', 'tcp'], 'xvfb', subprocess.PIPE)
    assert select.select([x.stdout], [], [], 8)[0]; env['DISPLAY'] = ':' + x.stdout.readline().decode().strip()
    launch(['openbox'], 'openbox'); time.sleep(.5)
    launch(['/usr/bin/python3', str(recv), str(work), 'r'], 'recv-r'); launch(['/usr/bin/python3', str(recv), str(work), 'f'], 'recv-f')
    wait_for(lambda: (work/'r-ready.json').exists() and (work/'f-ready.json').exists(), 8, 'receivers')
    RX = {'r': ready('r'), 'f': ready('f')}; report['receivers'] = dict(RX)
    sock = work/'driver.sock'; launch([str(driver), 'serve', '--socket', str(sock)], 'driver')
    wait_for(sock.exists, 20, 'driver socket'); time.sleep(.3)
    def call(name, args, session='agent'): return protocol(sock, {'method': 'call', 'name': name, 'args': args, 'session_id': session, 'client_kind': 'cli'})
    for s in ('agent', 'tile'): call('set_agent_cursor_enabled', {'session': s, 'enabled': False}, s)
    def cap(tag, win, session='agent', maxdim=4000):
        r = call('get_window_state', {'pid': RX[tag]['pid'], 'window_id': RX[tag][win], 'include_screenshot': True,
                                      'include_accessibility_tree': False, 'max_dimension': maxdim}, session)
        sc = r.get('structuredContent') or {}; assert sc.get('screenshot_id'), r
        return sc['screenshot_id'], (sc.get('screenshot_width'), sc.get('screenshot_height'))
    def name_of(tag, wid): return next((k for k in ('T', 'S') if RX[tag][k] == wid), wid)
    def act(case, tool, tag, win, args):
        full = {'pid': RX[tag]['pid'], 'delivery_mode': 'background', **({'window_id': RX[tag][win]} if win else {}), **args}
        b = {t: len(events(t)) for t in ('r', 'f')}
        r = call(tool, full); time.sleep(.7)
        seen = [(t, name_of(t, e['window']), e['kind'], e.get('button'), e.get('x'), e.get('y'))
                for t in ('r', 'f') for e in events(t)[b[t]:] if e.get('kind') in ('down', 'up')]
        sc = r.get('structuredContent') or {}
        code = sc.get('code') or (sc.get('refusal') or {}).get('code')
        outcome = (code or 'error') if r.get('isError') else 'delivered'
        want, where = EXPECTED[case]
        ok = outcome == want and seen == (where or [])
        if want in ('ambiguous_capture', 'stale_window_target', 'stale_capture'): ok = ok and sc.get('status') == 'refused'
        held = sc.get('held', (sc.get('mouse_hold') or {}).get('held'))
        if case in HELD: ok = ok and held is HELD[case]
        report['cases'][case] = {'tool': tool, 'args': full, 'is_error': bool(r.get('isError')), 'code': code, 'status': sc.get('status'),
            'held': held, 'text': ''.join(c.get('text', '') for c in r.get('content', []))[:400], 'input': seen,
            'expected': [want, where], 'ok': ok}
        print('PASS' if ok else 'FAIL', case, outcome, seen[:4], flush=True)
    # C9: two sessions capture T at two scales.
    agent_id, agent_size = cap('r', 'T', 'agent', 4000)
    tile_id, tile_size = cap('r', 'T', 'tile', 200)
    report['c9_sizes'] = {'agent': agent_size, 'tile': tile_size}
    pt = {'x': 100, 'y': 100}; wheel = {**pt, 'direction': 'down', 'amount': 1}
    act('c9_unbound_click_after_tile', 'click', 'r', 'T', pt)
    act('c9_bound_click_after_tile', 'click', 'r', 'T', {**pt, 'screenshot_id': agent_id})
    act('c9_unbound_scroll_after_tile', 'scroll', 'r', 'T', wheel)
    act('c9_bound_scroll_after_tile', 'scroll', 'r', 'T', {**wheel, 'screenshot_id': agent_id})
    act('c9_unbound_mouse_button_down_after_tile', 'mouse_button_down', 'r', 'T', pt)
    # release that press (not a case) so the bound press starts a new hold
    call('mouse_button_up', {'pid': RX['r']['pid'], 'window_id': RX['r']['T'], 'delivery_mode': 'background'}); time.sleep(.5)
    act('c9_bound_mouse_button_down_after_tile', 'mouse_button_down', 'r', 'T', {**pt, 'screenshot_id': agent_id})
    act('c9_unbound_mouse_drag_after_tile', 'mouse_drag', 'r', 'T', {'x': 150, 'y': 100, 'duration_ms': 50, 'steps': 2})
    act('c9_bound_mouse_drag_after_tile', 'mouse_drag', 'r', 'T', {'x': 150, 'y': 100, 'duration_ms': 50, 'steps': 2, 'screenshot_id': agent_id})
    act('c9_mouse_button_up_no_point', 'mouse_button_up', 'r', 'T', {})
    # N1: S's only capture is downscaled 4x.
    s_id, s_size = cap('r', 'S', 'agent', 100); report['n1_size'] = s_size
    act('n1_mouse_button_down_scaled', 'mouse_button_down', 'r', 'S', {'x': 25, 'y': 25})
    act('n1_mouse_button_up_no_point', 'mouse_button_up', 'r', 'S', {})
    # N2: hold on S, close S, then drag; the hold must be gone afterwards.
    act('n2_hold_down_on_sibling', 'mouse_button_down', 'r', 'S', {'x': 10, 'y': 10})
    send('r', 'destroy_sibling')
    act('n2_drag_after_window_closed', 'mouse_drag', 'r', 'S', {'x': 20, 'y': 20, 'duration_ms': 50, 'steps': 2})
    t_fresh, _ = cap('r', 'T', 'agent', 4000)
    act('n2_down_after_dropped_hold', 'mouse_button_down', 'r', 'T', {'x': 10, 'y': 10, 'screenshot_id': t_fresh})
    act('n2_up_after_dropped_hold', 'mouse_button_up', 'r', 'T', {})
    cap('f', 'S', 'agent', 100)
    act('n2_hold_down_on_f_sibling', 'mouse_button_down', 'f', 'S', {'x': 10, 'y': 10})
    send('f', 'destroy_sibling')
    act('n2_up_after_window_closed', 'mouse_button_up', 'f', 'S', {})
    # N3, N5.
    act('n3_scroll_string_coords_no_id', 'scroll', 'r', 'T', {'x': '300', 'y': '300', 'direction': 'down', 'amount': 1})
    act('n3_scroll_string_coords_with_id', 'scroll', 'r', 'T', {'x': '300', 'y': '300', 'direction': 'down', 'amount': 1, 'screenshot_id': t_fresh})
    act('n5_scroll_x_only', 'scroll', 'r', 'T', {'x': 100, 'direction': 'down', 'amount': 1})
    # mouse_button_up with string coordinates needs a hold to reach the point check.
    call('mouse_button_down', {'pid': RX['r']['pid'], 'window_id': RX['r']['T'], 'x': 10, 'y': 10, 'screenshot_id': t_fresh, 'delivery_mode': 'background'})
    time.sleep(.5)
    act('n3_mouse_button_up_string_coords', 'mouse_button_up', 'r', 'T', {'x': '10', 'y': '10'})
    call('mouse_button_up', {'pid': RX['r']['pid'], 'delivery_mode': 'background'}); time.sleep(.5)
    # O3: capture T, recreate it under the same XID, copy the old mark.
    old_id, _ = cap('r', 'T', 'agent', 4000); old_mark = get_mark(RX['r']['T']); old = RX['r']['T']
    send('r', 'recreate'); RX['r'] = ready('r')
    report['recreate'] = {'old_xid': old, 'new_xid': RX['r']['T'], 'same_xid': old == RX['r']['T'], 'old_mark': old_mark}
    act('o3_old_id_on_recreated_window', 'click', 'r', 'T', {**pt, 'screenshot_id': old_id})
    set_mark(RX['r']['T'], old_mark)
    act('o3_old_id_after_mark_copied', 'click', 'r', 'T', {**pt, 'screenshot_id': old_id})
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
    if not (report.get('recreate') or {}).get('same_xid'): failures.append('precondition_xid_not_reused')
    if tuple(report.get('c9_sizes', {}).get('tile') or ()) != (200, 150): failures.append('precondition_tile_not_200x150')
    report['failures'] = failures; report['ok'] = bool(report.get('done')) and not failures
    (work/'result.json').write_text(json.dumps(report, indent=1) + '\n')
    print('RESULT', 'ok' if report['ok'] else f'fail {failures}', report.get('error', ''))
    sys.exit(0 if report['ok'] else 1)
