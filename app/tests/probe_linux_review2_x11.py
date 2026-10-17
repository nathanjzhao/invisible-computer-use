"""Round-2 review items on X11: every pixel tool checks the window and pid
(B2), a screenshot_id does not survive its window's XID being reused (O3), and
a point scroll is bound like the other pixel tools.

    probe_linux_review2_x11.py WORK CUA_DRIVER RECEIVER

RECEIVER is fixtures/x11_edge_receiver.py, run twice as separate processes R
and F. Each has a target window T (800x600) and a sibling S (400x300) and logs
every button event with the window it arrived on. The driver runs as its own
daemon on a private socket; every call is background. Each case records what
the driver returned and what R and F received; the probe exits 0 only when
every case matches EXPECTED.
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
DRAG = {'from_x': 100, 'from_y': 100, 'to_x': 150, 'to_y': 100}
SCROLL = {'x': 100, 'y': 100, 'direction': 'down', 'amount': 1}
PIXEL_TOOLS = ('click', 'double_click', 'right_click', 'drag', 'scroll')
# case -> (outcome, receiver that must get input or None). An outcome is a
# refusal code (no input anywhere) or 'delivered'.
EXPECTED = {
    **{f'foreign_window_wrong_pid_{t}': ('stale_window_target', None) for t in PIXEL_TOOLS},
    'foreign_window_wrong_pid_mouse_button_down': ('stale_window_target', None),
    **{f'closed_window_bound_{t}': ('stale_window_target', None) for t in PIXEL_TOOLS},
    **{f'closed_window_unbound_{t}': ('stale_window_target', None) for t in PIXEL_TOOLS},
    'recreated_window_old_id_click': ('stale_capture', None),
    'recreated_window_old_id_drag': ('stale_capture', None),
    'recreated_window_unbound_click': ('stale_capture', None),
    'recreated_window_fresh_id_click': ('delivered', 'r'),
    'scroll_point_after_resize_no_id': ('stale_capture', None),
    'scroll_point_after_resize_stale_id': ('stale_capture', None),
    'scroll_point_fresh_id': ('delivered', 'r'),
    'scroll_id_without_point': ('invalid_arguments', None),
    'scroll_point_other_window_id': ('invalid_arguments', None),
    'own_window_click_control': ('delivered', 'r'),
}
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
signal.signal(signal.SIGALRM, lambda *_: (_ for _ in ()).throw(TimeoutError('deadline'))); signal.alarm(300)
try:
    x = launch(['Xvfb', '-displayfd', '1', '-screen', '0', '1600x1000x24', '-nolisten', 'tcp'], 'xvfb', subprocess.PIPE)
    assert select.select([x.stdout], [], [], 8)[0]; env['DISPLAY'] = ':' + x.stdout.readline().decode().strip()
    launch(['openbox'], 'openbox'); time.sleep(.5)
    launch(['/usr/bin/python3', str(recv), str(work), 'r'], 'recv-r'); launch(['/usr/bin/python3', str(recv), str(work), 'f'], 'recv-f')
    wait_for(lambda: (work/'r-ready.json').exists() and (work/'f-ready.json').exists(), 8, 'receivers')
    R, F = ready('r'), ready('f'); report['receivers'] = {'r': R, 'f': F}
    sock = work/'driver.sock'; launch([str(driver), 'serve', '--socket', str(sock)], 'driver')
    wait_for(sock.exists, 20, 'driver socket'); time.sleep(.3)
    def call(name, args): return protocol(sock, {'method': 'call', 'name': name, 'args': args, 'session_id': 'review2', 'client_kind': 'cli'})
    call('set_agent_cursor_enabled', {'session': 'review2', 'enabled': False})
    def cap(pid, wid):
        r = call('get_window_state', {'pid': pid, 'window_id': wid, 'include_screenshot': True, 'include_accessibility_tree': False, 'max_dimension': 4000})
        sid = (r.get('structuredContent') or {}).get('screenshot_id'); assert sid, r; return sid
    def xdo(*a): subprocess.run(['xdotool', *a], env=env, timeout=5); time.sleep(.5)
    def act(case, tool, pid, wid, sid=None, extra=None):
        args = {'pid': pid, 'window_id': wid, 'delivery_mode': 'background'}
        args.update(extra if extra is not None else DRAG if tool == 'drag' else SCROLL if tool == 'scroll' else {'x': 100, 'y': 100})
        if sid is not None: args['screenshot_id'] = sid
        b = {t: len(events(t)) for t in ('r', 'f')}
        r = call(tool, args); time.sleep(.7)
        if tool == 'mouse_button_down' and not r.get('isError'):
            call('mouse_button_up', {'session': 'review2'}); time.sleep(.3)
        seen = {t: [e for e in events(t)[b[t]:] if e.get('kind') in ('down', 'up')] for t in ('r', 'f')}
        sc = r.get('structuredContent') or {}
        code = sc.get('code') or (sc.get('refusal') or {}).get('code')
        outcome = code if r.get('isError') else 'delivered'
        want, where = EXPECTED[case]
        got_input = [t for t, v in seen.items() if v]
        ok = outcome == want and got_input == ([where] if where else [])
        if want == 'stale_window_target': ok = ok and sc.get('status') == 'refused'
        report['cases'][case] = {'tool': tool, 'args': args, 'is_error': bool(r.get('isError')), 'code': code, 'status': sc.get('status'),
            'text': ''.join(c.get('text', '') for c in r.get('content', []))[:400], 'events': seen,
            'expected': [want, where], 'ok': ok}
        print('PASS' if ok else 'FAIL', case, outcome, {t: [(e['window'], e['kind'], e.get('button')) for e in v][:3] for t, v in seen.items()}, flush=True)
    # B2: pid R addressed at process F's window, no screenshot_id.
    for tool in PIXEL_TOOLS: act(f'foreign_window_wrong_pid_{tool}', tool, R['pid'], F['T'])
    act('foreign_window_wrong_pid_mouse_button_down', 'mouse_button_down', R['pid'], F['T'], extra={'x': 100, 'y': 100})
    # the control reads its own capture: since c9-unify an unbound pixel on a window
    # this session never captured refuses ambiguous_capture
    cap(R['pid'], R['T'])
    act('own_window_click_control', 'click', R['pid'], R['T'])
    # B2: R's sibling, captured then destroyed, with and without its id.
    sibling_id = cap(R['pid'], R['S']); send('r', 'destroy_sibling')
    for tool in PIXEL_TOOLS: act(f'closed_window_bound_{tool}', tool, R['pid'], R['S'], sibling_id)
    for tool in PIXEL_TOOLS: act(f'closed_window_unbound_{tool}', tool, R['pid'], R['S'])
    # O3: capture T, destroy it and create a same-size window that reuses the XID.
    old_id = cap(R['pid'], R['T']); old = R['T']; send('r', 'recreate'); R = ready('r')
    report['recreate'] = {'old_xid': old, 'new_xid': R['T'], 'same_xid': old == R['T']}
    act('recreated_window_old_id_click', 'click', R['pid'], R['T'], old_id)
    act('recreated_window_old_id_drag', 'drag', R['pid'], R['T'], old_id)
    act('recreated_window_unbound_click', 'click', R['pid'], R['T'])
    act('recreated_window_fresh_id_click', 'click', R['pid'], R['T'], cap(R['pid'], R['T']))
    # Point scroll: bound like click.
    before_resize = cap(R['pid'], R['T']); xdo('windowsize', str(R['T']), '500', '600')
    act('scroll_point_after_resize_no_id', 'scroll', R['pid'], R['T'])
    act('scroll_point_after_resize_stale_id', 'scroll', R['pid'], R['T'], before_resize)
    act('scroll_point_fresh_id', 'scroll', R['pid'], R['T'], cap(R['pid'], R['T']))
    act('scroll_id_without_point', 'scroll', R['pid'], R['T'], cap(R['pid'], R['T']), {'direction': 'down', 'amount': 1})
    act('scroll_point_other_window_id', 'scroll', R['pid'], R['T'], cap(F['pid'], F['T']))
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
    report['failures'] = failures; report['ok'] = bool(report.get('done')) and not failures
    (work/'result.json').write_text(json.dumps(report, indent=1) + '\n')
    print('RESULT', 'ok' if report['ok'] else f'fail {failures}', report.get('error', ''))
    sys.exit(0 if report['ok'] else 1)
