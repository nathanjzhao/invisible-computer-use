"""C9 unified (study/audit/c9-unify-2026-09-27), X11: a pixel without screenshot_id
is read only against the CALLING SESSION's own latest capture of that exact window.
Other sessions' captures never rescale it and never make it refuse; no own capture
refuses ambiguous_capture; a window that changed or was replaced since the
session's own capture refuses stale_capture.

    probe_c9_unify_x11.py WORK CUA_DRIVER RECEIVER [FLOOD]

RECEIVER is app/tests/fixtures/x11_edge_receiver.py, run as processes R and F
(T 800x600, S 400x300). The driver is its own daemon on a private socket; every
call is background and names its session with the request's session_id, as the
MCP proxy does. FLOOD (default 4096, the capture log's size) is the size of each
eviction flood; the other-window flood is FLOOD + 904 (5000 at the default).

Sections:
  A  R.T: agent at full size, tile at 200 px (ratio 4), a second client at 400 px
     (ratio 2). Each session's unbound pixel lands at the point it read, on every
     pixel and hold tool; a bound id works from any session.
  B  R.S: only a tile captured it. A session that never captured it, and a call
     with no session at all, refuse; the tile's id still binds.
  F  F.S: the agent at full size, then FLOOD tile captures of F.S at 100 px, then
     FLOOD + 904 captures of other windows (review R4-1, mac-r5 R5-2). The
     agent's unbound pixel still lands where it read; its old id is evicted.
  I  F.T: the agent captures, F recreates T under the same XID, a tile captures
     the new window (review windows-r3 R3-5 / W11 on X11). The agent's unbound
     pixel refuses stale_capture; the tile's lands.
  Z  F.S: the agent captures, the window is resized, a tile captures the new
     size. The agent's unbound pixel refuses stale_capture.
  W  F.T: a new session's first capture of the window is in flight while the
     agent clicks (windows-r3 R3-1 / W8), 20 trials over 0 to 95 ms.
  L  R.S: the agent captures, then 256 other sessions do; the agent's entry is
     evicted and its unbound pixel refuses, never scaled by someone else's.
Each case records the reply and the exact button events. Exit 0 only when every
case matches EXPECTED.
"""
import hashlib, json, os, pathlib, select, signal, socket, subprocess, sys, threading, time, traceback

work = pathlib.Path(sys.argv[1]); work.mkdir(parents=True)
driver = pathlib.Path(sys.argv[2]).resolve(); recv = pathlib.Path(sys.argv[3]).resolve()
FLOOD = int(sys.argv[4]) if len(sys.argv) > 4 else 4096
OTHER_FLOOD = FLOOD + 904
SESSIONS = 256
procs = []; logs = []
report = {'driver_sha256': hashlib.sha256(driver.read_bytes()).hexdigest(),
          'probe_sha256': hashlib.sha256(pathlib.Path(__file__).read_bytes()).hexdigest(),
          'receiver_sha256': hashlib.sha256(recv.read_bytes()).hexdigest(), 'flood': FLOOD, 'cases': {}}
env = dict(os.environ); env.update({'XDG_RUNTIME_DIR': str(work/'runtime'), 'XDG_SESSION_TYPE': 'x11'}); env.pop('WAYLAND_DISPLAY', None)
(work/'runtime').mkdir(); (work/'runtime').chmod(0o700)
EXPECTED = {}
def expect(case, outcome, where=None): EXPECTED[case] = (outcome, where)
def at(tag, win, x, y, button=1): return [(tag, win, 'down', button, x, y), (tag, win, 'up', button, x, y)]
# A: three sessions, three scales, one window.
for s in ('agent', 'tile', 'other'):
    expect(f'a_{s}_click', 'delivered', at('r', 'T', 100, 100))
expect('a_agent_double_click', 'delivered', at('r', 'T', 100, 100) * 2)
expect('a_agent_right_click', 'delivered', at('r', 'T', 100, 100, 3))
expect('a_agent_drag', 'delivered', [('r', 'T', 'down', 1, 100, 100), ('r', 'T', 'up', 1, 140, 100)])
expect('a_agent_scroll', 'delivered', at('r', 'T', 100, 100, 5))
expect('a_tile_scroll', 'delivered', at('r', 'T', 100, 100, 5))
expect('a_agent_down', 'delivered', [('r', 'T', 'down', 1, 100, 100)])
expect('a_agent_mouse_drag', 'delivered', [])
expect('a_agent_up_point', 'delivered', [('r', 'T', 'up', 1, 140, 100)])
expect('a_agent_bound_with_tile_id', 'delivered', at('r', 'T', 100, 100))
# B: nobody but the tile captured R.S.
expect('b_stranger_click', 'ambiguous_capture')
expect('b_stranger_drag', 'ambiguous_capture')
expect('b_no_session_click', 'ambiguous_capture')
expect('b_stranger_bound_with_tile_id', 'delivered', at('r', 'S', 100, 100))
expect('b_stranger_after_own_capture', 'delivered', at('r', 'S', 100, 100))
# F: floods of the log.
expect('f_agent_click_after_same_window_flood', 'delivered', at('f', 'S', 50, 50))
expect('f_agent_click_after_other_window_flood', 'delivered', at('f', 'S', 60, 60))
expect('f_agent_old_id_after_floods', 'stale_capture')
expect('f_stranger_click_after_floods', 'ambiguous_capture')
# I: same XID, new window, laundering capture.
expect('i_agent_click_after_recreate_and_tile_capture', 'stale_capture')
expect('i_tile_click_on_new_window', 'delivered', at('f', 'T', 100, 100))
expect('i_agent_click_after_own_recapture', 'delivered', at('f', 'T', 100, 100))
# Z: resized since the agent's capture.
expect('z_agent_click_after_resize_and_tile_capture', 'stale_capture')
expect('z_agent_click_after_own_recapture', 'delivered', at('f', 'S', 80, 80))
# W: another session's capture in flight.
expect('w_agent_clicks_during_first_tile_captures', 'delivered')
# L: the agent's session evicted by 256 others.
expect('l_agent_click_after_eviction', 'ambiguous_capture')
expect('l_last_session_click', 'delivered', at('r', 'S', 100, 100))
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
        s.settimeout(120); s.connect(str(path)); s.sendall((json.dumps(req)+'\n').encode()); data = b''
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
    def call(name, args, session='agent'):
        req = {'method': 'call', 'name': name, 'args': args, 'client_kind': 'cli'}
        if session is not None: req['session_id'] = session
        return protocol(sock, req)
    for s in ('agent', 'tile', 'other', 'stranger', 'tile2'): call('set_agent_cursor_enabled', {'session': s, 'enabled': False}, s)
    def cap(tag, win, session='agent', maxdim=4000):
        r = call('get_window_state', {'pid': RX[tag]['pid'], 'window_id': RX[tag][win], 'include_screenshot': True,
                                      'include_accessibility_tree': False, 'max_dimension': maxdim}, session)
        sc = r.get('structuredContent') or {}; assert sc.get('screenshot_id'), r
        return sc['screenshot_id'], (sc.get('screenshot_width'), sc.get('screenshot_height'))
    def ping(session='agent'):
        call('list_windows', {'pid': RX['r']['pid']}, session)
    def name_of(tag, wid): return next((k for k in ('T', 'S') if RX[tag][k] == wid), wid)
    def seen_since(b):
        return [(t, name_of(t, e['window']), e['kind'], e.get('button'), e.get('x'), e.get('y'))
                for t in ('r', 'f') for e in events(t)[b[t]:] if e.get('kind') in ('down', 'up')]
    def act(case, tool, tag, win, args, session='agent'):
        full = {'pid': RX[tag]['pid'], 'delivery_mode': 'background', 'window_id': RX[tag][win], **args}
        b = {t: len(events(t)) for t in ('r', 'f')}
        t0 = time.monotonic()
        try:
            r = call(tool, full, session)
        except (socket.timeout, TimeoutError) as e:
            r = {'isError': True, 'structuredContent': {'code': 'probe_timeout'}, 'content': [{'text': f'no reply: {e}'}]}
        took = round(time.monotonic() - t0, 2); time.sleep(.7)
        seen = seen_since(b)
        sc = r.get('structuredContent') or {}
        code = sc.get('code') or (sc.get('refusal') or {}).get('code')
        outcome = (code or 'error') if r.get('isError') else 'delivered'
        want, where = EXPECTED[case]
        if want == 'delivered':
            ok = outcome == 'delivered' and (where is None or seen == where)
        else:
            ok = outcome == want and not seen and took <= SLOW
        report['cases'][case] = {'tool': tool, 'session': session, 'args': full, 'is_error': bool(r.get('isError')), 'code': code,
            'status': sc.get('status'), 'text': ''.join(c.get('text', '') for c in r.get('content', []))[:500], 'input': seen,
            'expected': [want, where], 'ok': ok, 'seconds': took}
        print('PASS' if ok else 'FAIL', case, outcome, code, seen[:4], flush=True)
        return r

    # ── A: R.T captured by three sessions at three scales. ──
    agent_t, agent_t_size = cap('r', 'T', 'agent', 4000)
    tile_t, tile_t_size = cap('r', 'T', 'tile', 200)
    other_t, other_t_size = cap('r', 'T', 'other', 400)
    report['a_sizes'] = {'agent': agent_t_size, 'tile': tile_t_size, 'other': other_t_size}
    act('a_agent_click', 'click', 'r', 'T', {'x': 100, 'y': 100}, 'agent')
    act('a_tile_click', 'click', 'r', 'T', {'x': 25, 'y': 25}, 'tile')
    act('a_other_click', 'click', 'r', 'T', {'x': 50, 'y': 50}, 'other')
    act('a_agent_double_click', 'double_click', 'r', 'T', {'x': 100, 'y': 100}, 'agent')
    act('a_agent_right_click', 'right_click', 'r', 'T', {'x': 100, 'y': 100}, 'agent')
    act('a_agent_drag', 'drag', 'r', 'T', {'from_x': 100, 'from_y': 100, 'to_x': 140, 'to_y': 100, 'duration_ms': 50, 'steps': 2}, 'agent')
    act('a_agent_scroll', 'scroll', 'r', 'T', {'x': 100, 'y': 100, 'direction': 'down', 'amount': 1}, 'agent')
    act('a_tile_scroll', 'scroll', 'r', 'T', {'x': 25, 'y': 25, 'direction': 'down', 'amount': 1}, 'tile')
    act('a_agent_down', 'mouse_button_down', 'r', 'T', {'x': 100, 'y': 100}, 'agent')
    act('a_agent_mouse_drag', 'mouse_drag', 'r', 'T', {'x': 120, 'y': 100, 'duration_ms': 50, 'steps': 2}, 'agent')
    act('a_agent_up_point', 'mouse_button_up', 'r', 'T', {'x': 140, 'y': 100}, 'agent')
    act('a_agent_bound_with_tile_id', 'click', 'r', 'T', {'x': 25, 'y': 25, 'screenshot_id': tile_t}, 'agent')

    # ── B: R.S captured only by the tile. ──
    tile_s, _ = cap('r', 'S', 'tile', 100)
    act('b_stranger_click', 'click', 'r', 'S', {'x': 25, 'y': 25}, 'stranger')
    act('b_stranger_drag', 'drag', 'r', 'S', {'from_x': 25, 'from_y': 25, 'to_x': 35, 'to_y': 25, 'duration_ms': 50, 'steps': 2}, 'stranger')
    act('b_no_session_click', 'click', 'r', 'S', {'x': 25, 'y': 25}, None)
    act('b_stranger_bound_with_tile_id', 'click', 'r', 'S', {'x': 25, 'y': 25, 'screenshot_id': tile_s}, 'stranger')
    cap('r', 'S', 'stranger', 4000)
    act('b_stranger_after_own_capture', 'click', 'r', 'S', {'x': 100, 'y': 100}, 'stranger')

    # ── F: F.S, the agent at full size, then the floods. ──
    f_agent, f_agent_size = cap('f', 'S', 'agent', 4000)
    t0 = time.monotonic()
    for i in range(FLOOD):
        cap('f', 'S', 'tile', 100)
        if i % 500 == 0: ping()
    report['f_flood_same_window'] = {'agent_size': f_agent_size, 'captures': FLOOD, 'seconds': round(time.monotonic() - t0, 1)}
    act('f_agent_click_after_same_window_flood', 'click', 'f', 'S', {'x': 50, 'y': 50}, 'agent')
    t0 = time.monotonic()
    for i in range(OTHER_FLOOD):
        cap('r', 'T' if i % 2 else 'S', 'tile', 200 if i % 2 else 100)
        if i % 500 == 0: ping()
    report['f_flood_other_windows'] = {'captures': OTHER_FLOOD, 'seconds': round(time.monotonic() - t0, 1)}
    act('f_agent_click_after_other_window_flood', 'click', 'f', 'S', {'x': 60, 'y': 60}, 'agent')
    act('f_agent_old_id_after_floods', 'click', 'f', 'S', {'x': 60, 'y': 60, 'screenshot_id': f_agent}, 'agent')
    act('f_stranger_click_after_floods', 'click', 'f', 'S', {'x': 60, 'y': 60}, 'stranger')

    # ── I: F.T replaced under the same XID, then captured by the tile. ──
    cap('f', 'T', 'agent', 4000)
    old = RX['f']['T']; send('f', 'recreate'); RX['f'] = ready('f')
    report['i_recreate'] = {'old_xid': old, 'new_xid': RX['f']['T'], 'same_xid': old == RX['f']['T']}
    cap('f', 'T', 'tile', 4000)
    act('i_agent_click_after_recreate_and_tile_capture', 'click', 'f', 'T', {'x': 100, 'y': 100}, 'agent')
    act('i_tile_click_on_new_window', 'click', 'f', 'T', {'x': 100, 'y': 100}, 'tile')
    cap('f', 'T', 'agent', 4000)
    act('i_agent_click_after_own_recapture', 'click', 'f', 'T', {'x': 100, 'y': 100}, 'agent')

    # ── Z: F.S resized after the agent's capture. ──
    cap('f', 'S', 'agent', 4000)
    subprocess.run(['xdotool', 'windowsize', str(RX['f']['S']), '500', '300'], env=env, check=True); time.sleep(.8)
    z_tile, z_size = cap('f', 'S', 'tile', 4000)
    report['z_resized_capture'] = z_size
    act('z_agent_click_after_resize_and_tile_capture', 'click', 'f', 'S', {'x': 80, 'y': 80}, 'agent')
    cap('f', 'S', 'agent', 4000)
    act('z_agent_click_after_own_recapture', 'click', 'f', 'S', {'x': 80, 'y': 80}, 'agent')

    # ── W: first captures of F.T by new sessions in flight while the agent clicks. ──
    trials = []
    b = {t: len(events(t)) for t in ('r', 'f')}
    for n in range(20):
        session = f'inflight-{n}'
        th = threading.Thread(target=lambda s=session: cap('f', 'T', s, 200))
        th.start(); time.sleep(n * 0.005)
        r = call('click', {'pid': RX['f']['pid'], 'window_id': RX['f']['T'], 'x': 100, 'y': 100, 'delivery_mode': 'background'}, 'agent')
        th.join(); time.sleep(.3)
        sc = r.get('structuredContent') or {}
        trials.append({'delay_ms': n * 5, 'is_error': bool(r.get('isError')), 'code': sc.get('code') or (sc.get('refusal') or {}).get('code')})
    seen = seen_since(b)
    ok = all(not t['is_error'] for t in trials) and seen == at('f', 'T', 100, 100) * 20
    report['cases']['w_agent_clicks_during_first_tile_captures'] = {'trials': trials, 'input': seen, 'ok': ok,
        'expected': ['delivered', '20 clicks at T (100,100)']}
    print('PASS' if ok else 'FAIL', 'w_agent_clicks_during_first_tile_captures', sorted({(t['is_error'], t['code']) for t in trials}), seen[:4], flush=True)

    # ── L: the agent's R.S entry evicted by 256 other sessions. ──
    cap('r', 'S', 'agent', 4000)
    t0 = time.monotonic()
    for n in range(SESSIONS):
        cap('r', 'S', f'lru-{n}', 100)
    report['l_sessions'] = {'sessions': SESSIONS, 'seconds': round(time.monotonic() - t0, 1)}
    act('l_agent_click_after_eviction', 'click', 'r', 'S', {'x': 100, 'y': 100}, 'agent')
    act('l_last_session_click', 'click', 'r', 'S', {'x': 25, 'y': 25}, f'lru-{SESSIONS - 1}')

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
    if not (report.get('i_recreate') or {}).get('same_xid'): failures.append('precondition_i_recreate_xid_not_reused')
    report['failures'] = failures; report['ok'] = bool(report.get('done')) and not failures
    (work/'result.json').write_text(json.dumps(report, indent=1) + '\n')
    print('RESULT', 'ok' if report['ok'] else f'fail {failures}', report.get('error', ''))
    sys.exit(0 if report['ok'] else 1)
