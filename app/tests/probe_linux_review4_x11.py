"""Round-4 Linux review fixes, X11: R4-1 (C9 after eviction), R4-2 (a held window
recreated under the same XID), R4-3 (coordinates off the window), R4-4 (from_zoom on
another window) and R4-5 (drag with non-number coordinates), plus the round-3 C9, N3
and N5 cases the review re-ran. Built on the review's probe_r4_x11.py; EXPECTED is
the fixed contract.

    probe_linux_review4_x11.py WORK CUA_DRIVER RECEIVER [FLOOD]

RECEIVER is app/tests/fixtures/x11_edge_receiver.py, run as processes R and F
(T 800x600, S 400x300). The driver is its own daemon; every call is background.
FLOOD (default 4096, the capture log's size) is the number of captures in each
eviction flood.

Sections, each on a window no earlier section touched at another scale:
  H  F.T: a held window recreated under the same XID, twice: the release, then the
     drag, refuse and drop the hold; a press on the new window works each time.
  C  R.S: string, bool and array coordinates on every pixel tool, and on drag.
  M  R.T: captured at two scales by two sessions; every pixel and hold tool,
     unbound (the agent's own capture) and bound.
  Z  R: a zoom of S does not translate a from_zoom click on T; a zoom of T does.
  E  F.S: the agent's capture evicted by FLOOD tile captures of the same window
     (unbound still lands at the agent's point, the old id is stale), then by FLOOD
     captures of another window, so no frame of F.S is left at all (still lands).
  W  R.S, last: points off the window (past 16 bits, negative, one past the
     edge) on every pixel tool, and the window's edge itself.
Each case records the reply, its duration and the exact button events. The probe
exits 0 only when every case matches EXPECTED.
"""
import hashlib, json, os, pathlib, select, signal, socket, subprocess, sys, time, traceback

work = pathlib.Path(sys.argv[1]); work.mkdir(parents=True)
driver = pathlib.Path(sys.argv[2]).resolve(); recv = pathlib.Path(sys.argv[3]).resolve()
FLOOD = int(sys.argv[4]) if len(sys.argv) > 4 else 4096
procs = []; logs = []
report = {'driver_sha256': hashlib.sha256(driver.read_bytes()).hexdigest(),
          'probe_sha256': hashlib.sha256(pathlib.Path(__file__).read_bytes()).hexdigest(),
          'receiver_sha256': hashlib.sha256(recv.read_bytes()).hexdigest(), 'flood': FLOOD, 'cases': {}}
env = dict(os.environ); env.update({'XDG_RUNTIME_DIR': str(work/'runtime'), 'XDG_SESSION_TYPE': 'x11'}); env.pop('WAYLAND_DISPLAY', None)
(work/'runtime').mkdir(); (work/'runtime').chmod(0o700)
REFUSED = 'refused'   # any typed refusal: status refused, or a code, and no input
EXPECTED = {}
def expect(case, outcome, where=None): EXPECTED[case] = (outcome, where)
# H (R4-2): after a same-XID recreate, the release and the drag refuse and drop the hold.
expect('h_down_on_ft', 'delivered', [('f', 'T', 'down', 1, 10, 10)])
expect('h_up_no_point_after_recreate', 'stale_window_target')
expect('h_down_on_new_ft_after_drop', 'delivered', [('f', 'T', 'down', 1, 30, 30)])
expect('h_drag_bound_old_id_after_recreate', 'stale_window_target')
expect('h_up_after_drag_dropped_hold', 'error')
expect('h_down_on_newest_ft', 'delivered', [('f', 'T', 'down', 1, 40, 40)])
expect('h_up_on_newest_ft', 'delivered', [('f', 'T', 'up', 1, 40, 40)])
# C (N3, N5, R4-5): coordinates are numbers or a typed refusal.
for tool in ('click', 'double_click', 'right_click', 'scroll', 'mouse_button_down'):
    for label in ('string', 'bool', 'array'):
        expect(f'c_{tool}_{label}', REFUSED)
expect('c_mouse_drag_string', REFUSED)
expect('c_mouse_button_up_bool', REFUSED)
for label in ('string', 'bool', 'missing'):
    expect(f'c_drag_{label}', 'invalid_arguments')
# W (R4-3): a point off the window refuses invalid_arguments, fast, with no input.
for tool in ('click', 'double_click', 'right_click', 'scroll', 'mouse_button_down'):
    for label in ('wrap_65636', 'negative', 'past_edge'):
        expect(f'w_{tool}_{label}', 'invalid_arguments')
expect('w_drag_to_wrap', 'invalid_arguments')
expect('w_click_edge', 'delivered', [('r', 'S', 'down', 1, 399, 299), ('r', 'S', 'up', 1, 399, 299)])
expect('w_mouse_drag_wrap', 'invalid_arguments')
expect('w_mouse_button_up_wrap', 'invalid_arguments')
# M (C9): two sessions, two scales; unbound reads the agent session's own capture
# (c9-unify; it used to refuse), bound lands at the agent's pixel.
for tool, where in (('click', [('r', 'T', 'down', 1, 100, 100), ('r', 'T', 'up', 1, 100, 100)]),
                    ('double_click', None),
                    ('right_click', [('r', 'T', 'down', 3, 100, 100), ('r', 'T', 'up', 3, 100, 100)]),
                    ('drag', [('r', 'T', 'down', 1, 100, 100), ('r', 'T', 'up', 1, 140, 100)]),
                    ('scroll', [('r', 'T', 'down', 5, 100, 100), ('r', 'T', 'up', 5, 100, 100)])):
    expect(f'm_{tool}_unbound', 'delivered', where)
    expect(f'm_{tool}_bound', 'delivered', where)
expect('m_down_bound', 'delivered', [('r', 'T', 'down', 1, 100, 100)])
expect('m_up_point_unbound', 'delivered', [('r', 'T', 'up', 1, 120, 120)])
expect('m_down_bound_2', 'delivered', [('r', 'T', 'down', 1, 100, 100)])
expect('m_up_point_bound', 'delivered', [('r', 'T', 'up', 1, 120, 120)])
# Z (R4-4): a zoom belongs to its window.
expect('z_from_zoom_on_t_after_zoom_of_s', 'invalid_arguments')
expect('z_from_zoom_on_t_after_zoom_of_t', 'delivered', [('r', 'T', 'down', 1, 190, 140), ('r', 'T', 'up', 1, 190, 140)])
# E (R4-1): the agent session's own capture outlives the capture log (c9-unify;
# before, the refusal did), so its unbound pixel lands where it read.
expect('e_unbound_click_after_flood', 'delivered', [('f', 'S', 'down', 1, 50, 50), ('f', 'S', 'up', 1, 50, 50)])
expect('e_bound_click_old_id_after_flood', 'stale_capture')
expect('e_unbound_click_after_all_frames_evicted', 'delivered', [('f', 'S', 'down', 1, 50, 50), ('f', 'S', 'up', 1, 50, 50)])
# No refused case may take long: before R4-3 the wrap took 190 s.
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
        s.settimeout(400); s.connect(str(path)); s.sendall((json.dumps(req)+'\n').encode()); data = b''
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
signal.signal(signal.SIGALRM, lambda *_: (_ for _ in ()).throw(TimeoutError('deadline'))); signal.alarm(3600)
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
        t0 = time.monotonic()
        try:
            r = call(tool, full)
        except (socket.timeout, TimeoutError) as e:
            r = {'isError': True, 'structuredContent': {'code': 'probe_timeout'}, 'content': [{'text': f'no reply in 400 s: {e}'}]}
        took = round(time.monotonic() - t0, 2); time.sleep(.7)
        seen = [(t, name_of(t, e['window']), e['kind'], e.get('button'), e.get('x'), e.get('y'))
                for t in ('r', 'f') for e in events(t)[b[t]:] if e.get('kind') in ('down', 'up')]
        sc = r.get('structuredContent') or {}
        code = sc.get('code') or (sc.get('refusal') or {}).get('code')
        outcome = (code or 'error') if r.get('isError') else 'delivered'
        want, where = EXPECTED[case]
        if want == REFUSED:
            ok = bool(r.get('isError')) and bool(code or sc.get('status') == 'refused') and not seen
        elif want == 'delivered':
            ok = outcome == 'delivered' and (where is None or seen == where)
        else:
            ok = outcome == want and not seen
        if want != 'delivered' and took > SLOW:
            ok = False
        held = sc.get('held', (sc.get('mouse_hold') or {}).get('held'))
        report['cases'][case] = {'tool': tool, 'args': full, 'is_error': bool(r.get('isError')), 'code': code, 'status': sc.get('status'),
            'held': held, 'text': ''.join(c.get('text', '') for c in r.get('content', []))[:400], 'input': seen,
            'expected': [want, where], 'ok': ok, 'seconds': took}
        print('PASS' if ok else 'FAIL', case, outcome, code, seen[:4], flush=True)
        return r
    def release(tag, win):
        # window_id is needed: a pid with two windows is ambiguous_window_target.
        r = call('mouse_button_up', {'pid': RX[tag]['pid'], 'window_id': RX[tag][win], 'delivery_mode': 'background'}); time.sleep(.4)
        report.setdefault('releases', []).append({'window': win, 'is_error': r.get('isError'), 'text': ''.join(c.get('text', '') for c in r.get('content', []))[:160]})

    # ── H: F.T captured once; press; F recreates T under the same XID. ──
    f_id, _ = cap('f', 'T', 'agent', 4000)
    act('h_down_on_ft', 'mouse_button_down', 'f', 'T', {'x': 10, 'y': 10, 'screenshot_id': f_id})
    old = RX['f']['T']; send('f', 'recreate'); RX['f'] = ready('f')
    report['h_recreate'] = {'old_xid': old, 'new_xid': RX['f']['T'], 'same_xid': old == RX['f']['T']}
    act('h_up_no_point_after_recreate', 'mouse_button_up', 'f', 'T', {})
    # The hold was dropped, so a new press on the new window is accepted.
    f_new, _ = cap('f', 'T', 'agent', 4000)
    act('h_down_on_new_ft_after_drop', 'mouse_button_down', 'f', 'T', {'x': 30, 'y': 30, 'screenshot_id': f_new})
    old = RX['f']['T']; send('f', 'recreate'); RX['f'] = ready('f')
    report['h_recreate_2'] = {'old_xid': old, 'new_xid': RX['f']['T'], 'same_xid': old == RX['f']['T']}
    act('h_drag_bound_old_id_after_recreate', 'mouse_drag', 'f', 'T', {'x': 20, 'y': 20, 'duration_ms': 50, 'steps': 2, 'screenshot_id': f_new})
    act('h_up_after_drag_dropped_hold', 'mouse_button_up', 'f', 'T', {})
    f_newest, _ = cap('f', 'T', 'agent', 4000)
    act('h_down_on_newest_ft', 'mouse_button_down', 'f', 'T', {'x': 40, 'y': 40, 'screenshot_id': f_newest})
    act('h_up_on_newest_ft', 'mouse_button_up', 'f', 'T', {})

    # ── C: R.S, one capture at one scale. ──
    s_id, s_size = cap('r', 'S', 'agent', 4000); report['c_size'] = s_size
    bad = {'string': ('10', '10'), 'bool': (True, True), 'array': ([10], [10])}
    for tool in ('click', 'double_click', 'right_click', 'scroll', 'mouse_button_down'):
        for label, (bx, by) in bad.items():
            extra = {'direction': 'down', 'amount': 1} if tool == 'scroll' else {}
            act(f'c_{tool}_{label}', tool, 'r', 'S', {'x': bx, 'y': by, 'screenshot_id': s_id, **extra})
            if tool == 'mouse_button_down': release('r', 'S')
    call('mouse_button_down', {'pid': RX['r']['pid'], 'window_id': RX['r']['S'], 'x': 5, 'y': 5, 'screenshot_id': s_id, 'delivery_mode': 'background'}); time.sleep(.4)
    act('c_mouse_drag_string', 'mouse_drag', 'r', 'S', {'x': '10', 'y': '10', 'duration_ms': 50, 'steps': 2, 'screenshot_id': s_id})
    act('c_mouse_button_up_bool', 'mouse_button_up', 'r', 'S', {'x': True, 'y': True})
    release('r', 'S')
    dg = {'from_x': 10, 'from_y': 10, 'to_x': 40, 'to_y': 10, 'duration_ms': 50, 'steps': 2, 'screenshot_id': s_id}
    act('c_drag_string', 'drag', 'r', 'S', {**dg, 'from_x': '10'})
    act('c_drag_bool', 'drag', 'r', 'S', {**dg, 'to_y': True})
    act('c_drag_missing', 'drag', 'r', 'S', {k: v for k, v in dg.items() if k != 'to_x'})

    # ── M: R.T at two scales. ──
    agent_id, agent_size = cap('r', 'T', 'agent', 4000)
    tile_id, tile_size = cap('r', 'T', 'tile', 200)
    report['m_sizes'] = {'agent': agent_size, 'tile': tile_size}
    pt = {'x': 100, 'y': 100}
    drag = {'from_x': 100, 'from_y': 100, 'to_x': 140, 'to_y': 100, 'duration_ms': 50, 'steps': 2}
    for tool, args in (('click', pt), ('double_click', pt), ('right_click', pt), ('drag', drag),
                       ('scroll', {**pt, 'direction': 'down', 'amount': 1})):
        act(f'm_{tool}_unbound', tool, 'r', 'T', args)
        act(f'm_{tool}_bound', tool, 'r', 'T', {**args, 'screenshot_id': agent_id})
    act('m_down_bound', 'mouse_button_down', 'r', 'T', {**pt, 'screenshot_id': agent_id})
    act('m_up_point_unbound', 'mouse_button_up', 'r', 'T', {'x': 120, 'y': 120})
    act('m_down_bound_2', 'mouse_button_down', 'r', 'T', {**pt, 'screenshot_id': agent_id})
    act('m_up_point_bound', 'mouse_button_up', 'r', 'T', {'x': 120, 'y': 120, 'screenshot_id': agent_id})

    # ── Z: zoom S, from_zoom on T; then zoom T, from_zoom on T. ──
    z = call('zoom', {'pid': RX['r']['pid'], 'window_id': RX['r']['S'], 'x1': 200, 'y1': 150, 'x2': 300, 'y2': 250})
    report['z_zoom_s'] = {'is_error': z.get('isError'), 'structured': z.get('structuredContent')}
    act('z_from_zoom_on_t_after_zoom_of_s', 'click', 'r', 'T', {'x': 10, 'y': 10, 'from_zoom': True})
    z = call('zoom', {'pid': RX['r']['pid'], 'window_id': RX['r']['T'], 'x1': 200, 'y1': 150, 'x2': 300, 'y2': 250})
    report['z_zoom_t'] = {'is_error': z.get('isError'), 'structured': z.get('structuredContent')}
    act('z_from_zoom_on_t_after_zoom_of_t', 'click', 'r', 'T', {'x': 10, 'y': 10, 'from_zoom': True})

    # ── E: F.S, agent at 4000, then FLOOD tile captures at 100. ──
    e_agent, e_agent_size = cap('f', 'S', 'agent', 4000)
    t0 = time.monotonic()
    for i in range(FLOOD):
        cap('f', 'S', 'tile', 100)
    report['e_flood'] = {'agent_size': e_agent_size, 'captures': FLOOD, 'seconds': round(time.monotonic() - t0, 1)}
    act('e_unbound_click_after_flood', 'click', 'f', 'S', {'x': 50, 'y': 50})
    act('e_bound_click_old_id_after_flood', 'click', 'f', 'S', {'x': 50, 'y': 50, 'screenshot_id': e_agent})
    # FLOOD captures of F.T: the log now holds no frame of F.S at all.
    t0 = time.monotonic()
    for i in range(FLOOD):
        cap('f', 'T', 'tile', 4000)
    report['e_flood_other'] = {'captures': FLOOD, 'seconds': round(time.monotonic() - t0, 1)}
    act('e_unbound_click_after_all_frames_evicted', 'click', 'f', 'S', {'x': 50, 'y': 50})
    # ── W, last: R.S at one scale; points off the window. Before R4-3 each wrap
    # case took about 190 s, long enough for the idle 'tile' session to end, so
    # nothing that needs 'tile' runs after it. ──
    # A fresh capture: the E floods evicted C's frame of R.S from the log.
    s_id, _ = cap('r', 'S', 'agent', 4000)
    dg = {**dg, 'screenshot_id': s_id}
    off = {'wrap_65636': (65636, 65636), 'negative': (-5, 10), 'past_edge': (401, 10)}
    for tool in ('click', 'double_click', 'right_click', 'scroll', 'mouse_button_down'):
        for label, (ox, oy) in off.items():
            extra = {'direction': 'down', 'amount': 1} if tool == 'scroll' else {}
            act(f'w_{tool}_{label}', tool, 'r', 'S', {'x': ox, 'y': oy, 'screenshot_id': s_id, **extra})
            if tool == 'mouse_button_down': release('r', 'S')
    act('w_drag_to_wrap', 'drag', 'r', 'S', {**dg, 'to_x': 65636})
    act('w_click_edge', 'click', 'r', 'S', {'x': 399, 'y': 299, 'screenshot_id': s_id})
    call('mouse_button_down', {'pid': RX['r']['pid'], 'window_id': RX['r']['S'], 'x': 5, 'y': 5, 'screenshot_id': s_id, 'delivery_mode': 'background'}); time.sleep(.4)
    act('w_mouse_drag_wrap', 'mouse_drag', 'r', 'S', {'x': 65636, 'y': 10, 'duration_ms': 50, 'steps': 2, 'screenshot_id': s_id})
    act('w_mouse_button_up_wrap', 'mouse_button_up', 'r', 'S', {'x': 10, 'y': 65636, 'screenshot_id': s_id})
    release('r', 'S')

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
    for key in ('h_recreate', 'h_recreate_2'):
        if not (report.get(key) or {}).get('same_xid'): failures.append(f'precondition_{key}_xid_not_reused')
    report['failures'] = failures; report['ok'] = bool(report.get('done')) and not failures
    (work/'result.json').write_text(json.dumps(report, indent=1) + '\n')
    print('RESULT', 'ok' if report['ok'] else f'fail {failures}', report.get('error', ''))
    sys.exit(0 if report['ok'] else 1)
