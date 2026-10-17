"""Round-3 review item R3-B1: sway window capture is refused outright.

    probe_linux_review3_sway.py WORK CUA_DRIVER GRID_FIXTURE LAYER_FIXTURE

The review's three setups each put another client's pixels into a crop the
driver attested:

- swaybar_overlay: sway's own `bar {}` (mode overlay, cyan), which sway spawns
  over a socketpair, over the target's top rows.
- launch_app_child_overlay: the magenta layer overlay started through the
  driver's own launch_app, so it is the driver's child.
- ancestor_overlay: the magenta overlay drawn by the process that launched a
  second driver daemon.

Plus the controls: the bar hidden, the target alone, and an overlay from an
unrelated process. Since no protocol lets the driver prove an output crop
holds only the target, every case must return no image, a
`surface_identity_unproven` screenshot_error for sway's reason, and readiness
`surface_identity_unproven` naming the same reason, which readiness must also
say before any capture. The accessibility tree still comes back. Exits 0 only
when every case holds.
"""
import base64, glob, hashlib, io, json, os, pathlib, signal, socket, subprocess, sys, time, traceback

if sys.platform != 'linux': raise SystemExit('Linux-only probe')
work = pathlib.Path(sys.argv[1]); work.mkdir(parents=True)
driver, grid_fx, layer_fx = (pathlib.Path(a).resolve() for a in sys.argv[2:5])
procs = []; logs = []
report = {'driver_sha256': hashlib.sha256(driver.read_bytes()).hexdigest(),
          'probe_sha256': hashlib.sha256(pathlib.Path(__file__).read_bytes()).hexdigest(),
          'fixture_sha256': {p.name: hashlib.sha256(p.read_bytes()).hexdigest() for p in (grid_fx, layer_fx)}, 'cases': {}}
CASES = ['swaybar_overlay', 'target_with_bar_invisible', 'launch_app_child_overlay', 'target_alone_after_child',
         'ancestor_overlay', 'normal_overlay_control']
REASON = 'sway has no per-window capture'
env = {k: v for k, v in os.environ.items() if k not in ('DISPLAY', 'WAYLAND_DISPLAY')}
env.update({'XDG_RUNTIME_DIR': str(work/'runtime'), 'XDG_SESSION_TYPE': 'wayland', 'XDG_CURRENT_DESKTOP': 'sway', 'GTK_MODULES': 'gail:atk-bridge', 'NO_AT_BRIDGE': '0'})
(work/'runtime').mkdir(); (work/'runtime').chmod(0o700)
def launch(args, name, extra=None):
    log = (work/(name+'.log')).open('wb'); logs.append(log)
    p = subprocess.Popen(args, env={**env, **(extra or {})}, stdin=subprocess.DEVNULL, stdout=log, stderr=log, start_new_session=True); procs.append(p); return p
def stop(p):
    try: os.killpg(p.pid, signal.SIGTERM)
    except ProcessLookupError: pass
    try: p.wait(timeout=3)
    except subprocess.TimeoutExpired: os.killpg(p.pid, signal.SIGKILL); p.wait()
    time.sleep(1)
def wait_for(pred, s, msg):
    d = time.monotonic() + s
    while time.monotonic() < d:
        r = pred()
        if r: return r
        time.sleep(.05)
    raise RuntimeError(msg)
def sway(*a): return subprocess.run(['swaymsg', *a], env=env, capture_output=True, text=True, timeout=5)
def protocol(path, req):
    with socket.socket(socket.AF_UNIX) as s:
        s.settimeout(30); s.connect(str(path)); s.sendall((json.dumps(req)+'\n').encode()); data = b''
        while b'\n' not in data:
            ch = s.recv(1 << 22)
            if not ch: raise ConnectionError('closed')
            data += ch
    r = json.loads(data.split(b'\n')[0]); assert r.get('ok'), r; return r['result']
def colors(png):
    from PIL import Image
    im = Image.open(io.BytesIO(png)).convert('RGB'); px = list(im.getdata())
    return {'magenta_px': sum(1 for p in px if p[0] > 200 and p[1] < 60 and p[2] > 200),
            'cyan_px': sum(1 for p in px if p[0] < 60 and p[1] > 200 and p[2] > 200), 'size': im.size}
def pids_of(comm):
    out = []
    for d in pathlib.Path('/proc').iterdir():
        if d.name.isdigit():
            try:
                if (d/'comm').read_text().strip() == comm: out.append(int(d.name))
            except OSError: pass
    return out
def wayland_fds(pid):
    """Which of pid's sockets are named (accepted on the listening path) vs anonymous."""
    try: return sorted(os.readlink(f'/proc/{pid}/fd/{f}') for f in os.listdir(f'/proc/{pid}/fd') if os.readlink(f'/proc/{pid}/fd/{f}').startswith('socket:'))
    except OSError as e: return str(e)
signal.signal(signal.SIGALRM, lambda *_: (_ for _ in ()).throw(TimeoutError('deadline'))); signal.alarm(360)
try:
    (work/'sway.conf').write_text(
        'output HEADLESS-1 resolution 1920x1080 position 0 0\n'
        'focus_on_window_activation none\n'
        'bar {\n  mode overlay\n  position top\n  height 60\n  status_command while :; do echo r3; sleep 5; done\n'
        '  colors {\n    background #00ffff\n    statusline #00ffff\n  }\n}\n')
    launch(['sway', '-c', str(work/'sway.conf')], 'sway', {'WLR_BACKENDS': 'headless', 'WLR_RENDERER': 'pixman', 'WLR_LIBINPUT_NO_DEVICES': '1'})
    env['SWAYSOCK'] = wait_for(lambda: glob.glob(str(work/'runtime'/'sway-ipc.*.sock')), 15, 'sway ipc')[0]
    env['WAYLAND_DISPLAY'] = wait_for(lambda: [p.name for p in (work/'runtime').glob('wayland-*') if not p.name.endswith('.lock')], 10, 'wl')[0]
    subprocess.run(['dbus-update-activation-environment', 'WAYLAND_DISPLAY', 'XDG_RUNTIME_DIR', 'XDG_SESSION_TYPE', 'XDG_CURRENT_DESKTOP'], env=env, timeout=5)
    grid = launch(['/usr/bin/python3', str(grid_fx), str(work), 'WL'], 'grid', {'GDK_BACKEND': 'wayland'})
    wait_for(lambda: (work/f'fixture-{grid.pid}.json').exists(), 15, 'grid')
    sock = work/'driver.sock'; drv = launch([str(driver), 'serve', '--socket', str(sock)], 'driver', {'CUA_DRIVER_RS_ENABLE_WAYLAND': '1'})
    wait_for(sock.exists, 20, 'driver'); time.sleep(.3)
    def call(n, a, s=sock): return protocol(s, {'method': 'call', 'name': n, 'args': a, 'session_id': 'r3', 'client_kind': 'cli'})
    call('set_agent_cursor_enabled', {'session': 'r3', 'enabled': False})
    wid = next(w['window_id'] for w in call('list_windows', {})['structuredContent']['windows'] if w.get('pid') == grid.pid)
    report['swaybar_pids'] = pids_of('swaybar'); report['swaybar_sockets'] = {p: wayland_fds(p) for p in report['swaybar_pids']}
    def readiness(s=sock):
        for c in (call('health_report', {}, s).get('structuredContent') or {}).get('checks', []):
            if c.get('name') == 'wayland_backend':
                return ((c.get('data') or {}).get('wayland_routes') or {}).get('window_capture'), c.get('message')
        return None, None
    def case(name, s=sock, note=None):
        r = call('get_window_state', {'pid': grid.pid, 'window_id': wid, 'include_screenshot': True, 'include_accessibility_tree': False}, s)
        sc = r.get('structuredContent') or {}
        img = next((c for c in r.get('content', []) if c.get('type') == 'image'), None)
        route, message = readiness(s)
        entry = {'image': img is not None, 'screenshot_error': sc.get('screenshot_error'), 'readiness_window_capture': route,
                 'readiness_message': message, 'note': note}
        if img:
            png = base64.b64decode(img['data']); (work/f'{name}.png').write_bytes(png); entry.update(colors(png))
        entry['leak'] = bool(img) and (entry.get('magenta_px', 0) > 0 or entry.get('cyan_px', 0) > 0)
        error = sc.get('screenshot_error') or {}
        entry['ok'] = (not img and error.get('code') == 'surface_identity_unproven' and REASON in json.dumps(error)
                       and route == 'surface_identity_unproven' and REASON in (message or ''))
        report['cases'][name] = entry
        print('PASS' if entry['ok'] else 'FAIL', 'LEAK' if entry['leak'] else '', name, 'image' if img else 'no-image', entry.get('magenta_px'), entry.get('cyan_px'),
              error.get('code'), route, flush=True)
    # Readiness says so before any capture; the tree still comes back.
    route, message = readiness()
    report['readiness_up_front'] = {'window_capture': route, 'message': message,
                                    'ok': route == 'surface_identity_unproven' and REASON in (message or '')}
    tree = call('get_window_state', {'pid': grid.pid, 'window_id': wid, 'include_screenshot': False}).get('structuredContent') or {}
    report['tree'] = {'element_count': tree.get('element_count'), 'ok': (tree.get('element_count') or 0) > 0}
    # 1. sway's own bar (layer overlay, spawned over WAYLAND_SOCKET) over the target's top 40 rows.
    sway(f'[pid={grid.pid}] floating enable, resize set 600 360, move position 100 20'); time.sleep(1.5)
    report['tree_bar'] = json.loads(sway('-r', '-t', 'get_bar_config').stdout or 'null')
    case('swaybar_overlay', note='bar spawned by sway, mode overlay, cyan, y 0..60; target at y 20')
    # 2. Remove the bar; later cases are about the magenta overlay at (150,150) 300x150.
    report['bar_invisible'] = sway('bar', 'mode', 'invisible').stdout; time.sleep(1)
    sway(f'[pid={grid.pid}] move position 100 100'); time.sleep(1)
    case('target_with_bar_invisible', note='control: bar hidden')
    # 3. The overlay launched through the driver's own launch_app (a child of the driver).
    ready = work/'lc-ready.json'
    r = call('launch_app', {'launch_path': f'/usr/bin/python3 {layer_fx} {ready} nowindow'})
    report['launch_app'] = {'is_error': r.get('isError'), 'structured': r.get('structuredContent')}
    wait_for(ready.exists, 20, 'launched overlay'); time.sleep(1.5)
    lpid = json.loads(ready.read_text())['pid']; report['launched_overlay'] = {'pid': lpid, 'ppid': int(pathlib.Path(f'/proc/{lpid}/stat').read_text().rsplit(')', 1)[1].split()[1]), 'driver_pid': drv.pid}
    case('launch_app_child_overlay', note='overlay started by the driver launch_app')
    os.kill(lpid, signal.SIGTERM); time.sleep(1.5)
    case('target_alone_after_child', note='control')
    # 4. The overlay drawn by the process that launched a second driver daemon.
    sock2 = work/'driver2.sock'; host_ready = work/'host-ready.json'
    host_src = work/'ancestor_host.py'
    host_src.write_text(
        'import subprocess, sys, os\n'
        f'subprocess.Popen([{str(driver)!r}, "serve", "--socket", {str(sock2)!r}], env={{**os.environ, "CUA_DRIVER_RS_ENABLE_WAYLAND": "1"}})\n'
        f'sys.argv = [{str(layer_fx)!r}, {str(host_ready)!r}, "nowindow"]\n'
        f'exec(open({str(layer_fx)!r}).read())\n')
    host = launch(['/usr/bin/python3', str(host_src)], 'ancestor-host', {'GDK_BACKEND': 'wayland'})
    wait_for(lambda: host_ready.exists() and sock2.exists(), 20, 'ancestor host'); time.sleep(1.5)
    call('set_agent_cursor_enabled', {'session': 'r3', 'enabled': False}, sock2)
    case('ancestor_overlay', sock2, note='overlay drawn by the process that spawned driver2; captured through driver2')
    stop(host); time.sleep(.5)
    # 5. Control: the same overlay launched by the probe (neither child nor ancestor).
    ready3 = work/'l3-ready.json'
    lw = launch(['/usr/bin/python3', str(layer_fx), str(ready3), 'nowindow'], 'layer3', {'GDK_BACKEND': 'wayland'})
    wait_for(ready3.exists, 15, 'layer3'); time.sleep(1.5)
    case('normal_overlay_control', note='overlay from an unrelated process; the fix must refuse this')
    stop(lw)
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
    failures = sorted(k for k in CASES if not report['cases'].get(k, {}).get('ok'))
    failures += [k for k in ('readiness_up_front', 'tree') if not (report.get(k) or {}).get('ok')]
    if not report.get('swaybar_pids'): failures.append('precondition_no_swaybar')
    report['failures'] = failures; report['ok'] = bool(report.get('done')) and not failures
    (work/'result.json').write_text(json.dumps(report, indent=1) + '\n')
    print('RESULT', 'ok' if report['ok'] else f'fail {failures}', {k: v.get('leak') for k, v in report['cases'].items()}, report.get('error', ''))
    sys.exit(0 if report['ok'] else 1)
