"""Round-2 review items on sway: a crop is refused while any other Wayland
client is connected, whether or not it owns a container (B1), and readiness
names the crop route unproven whenever the crop would be refused (O2). Since
round 3 (R3-B1) every sway window capture is refused, the target alone
included, for one reason that readiness also gives.

    probe_sway_layer_census.py WORK CUA_DRIVER GRID_FIXTURE LAYER_FIXTURE

GRID_FIXTURE is fixtures/wayland_grid_gtk.py (the target, floated 600x360 at
100,100). LAYER_FIXTURE is fixtures/layer_overlay_gtk.py: one GTK3 process with
a toplevel and a 300x150 magenta overlay layer surface at (150,150), which lies
over the target. Headless sway, driver with CUA_DRIVER_RS_ENABLE_WAYLAND=1.
Each case captures the target and reads health_report's window_capture route.
The probe also dumps sway's IPC replies while the overlay is up, to record
whether any of them places the layer surface. Exits 0 only when every case
matches EXPECTED.
"""
import base64, glob, hashlib, io, json, os, pathlib, signal, socket, subprocess, sys, time, traceback

if sys.platform != 'linux': raise SystemExit('Linux-only probe')
work = pathlib.Path(sys.argv[1]); work.mkdir(parents=True)
driver, grid_fx, layer_fx = (pathlib.Path(a).resolve() for a in sys.argv[2:5])
procs = []; logs = []
report = {'driver_sha256': hashlib.sha256(driver.read_bytes()).hexdigest(),
          'probe_sha256': hashlib.sha256(pathlib.Path(__file__).read_bytes()).hexdigest(),
          'layer_fixture_sha256': hashlib.sha256(layer_fx.read_bytes()).hexdigest(), 'cases': {}}
ATTESTED, UNPROVEN = 'compositor_attested_output_crop', 'surface_identity_unproven'
# case -> (image returned, readiness window_capture, process named in the refusal)
EXPECTED = {
    'target_alone': (False, UNPROVEN, None),
    'swaybg_connected': (False, UNPROVEN, 'swaybg'),
    'layer_overlay_from_client_with_a_toplevel': (False, UNPROVEN, 'layer'),
    'layer_overlay_from_client_without_a_toplevel': (False, UNPROVEN, 'layer'),
    'second_windowed_client_on_another_workspace': (False, UNPROVEN, 'grid2'),
    'target_alone_again': (False, UNPROVEN, None),
}
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
def magenta(png):
    from PIL import Image
    im = Image.open(io.BytesIO(png)).convert('RGB')
    return sum(1 for p in im.getdata() if p[0] > 200 and p[1] < 60 and p[2] > 200)
signal.signal(signal.SIGALRM, lambda *_: (_ for _ in ()).throw(TimeoutError('deadline'))); signal.alarm(300)
try:
    (work/'sway.conf').write_text('output HEADLESS-1 resolution 1920x1080 position 0 0\nfocus_on_window_activation none\n')
    launch(['sway', '-c', str(work/'sway.conf')], 'sway', {'WLR_BACKENDS': 'headless', 'WLR_RENDERER': 'pixman', 'WLR_LIBINPUT_NO_DEVICES': '1'})
    env['SWAYSOCK'] = wait_for(lambda: glob.glob(str(work/'runtime'/'sway-ipc.*.sock')), 15, 'sway ipc')[0]
    env['WAYLAND_DISPLAY'] = wait_for(lambda: [p.name for p in (work/'runtime').glob('wayland-*') if not p.name.endswith('.lock')], 10, 'wl')[0]
    subprocess.run(['dbus-update-activation-environment', 'WAYLAND_DISPLAY', 'XDG_RUNTIME_DIR', 'XDG_SESSION_TYPE', 'XDG_CURRENT_DESKTOP'], env=env, timeout=5)
    grid = launch(['/usr/bin/python3', str(grid_fx), str(work), 'WL'], 'grid', {'GDK_BACKEND': 'wayland'})
    wait_for(lambda: (work/f'fixture-{grid.pid}.json').exists(), 15, 'grid')
    sway(f'[pid={grid.pid}] floating enable, resize set 600 360, move position 100 100'); time.sleep(.8)
    sock = work/'driver.sock'; launch([str(driver), 'serve', '--socket', str(sock)], 'driver', {'CUA_DRIVER_RS_ENABLE_WAYLAND': '1'})
    wait_for(sock.exists, 20, 'driver'); time.sleep(.3)
    def call(n, a): return protocol(sock, {'method': 'call', 'name': n, 'args': a, 'session_id': 'layers', 'client_kind': 'cli'})
    call('set_agent_cursor_enabled', {'session': 'layers', 'enabled': False})
    wid = next(w['window_id'] for w in call('list_windows', {})['structuredContent']['windows'] if w.get('pid') == grid.pid)
    def readiness():
        for c in (call('health_report', {}).get('structuredContent') or {}).get('checks', []):
            if c.get('name') == 'wayland_backend':
                return ((c.get('data') or {}).get('wayland_routes') or {}).get('window_capture'), c.get('message')
        return None, None
    def case(name, foreign=None):
        r = call('get_window_state', {'pid': grid.pid, 'window_id': wid, 'include_screenshot': True, 'include_accessibility_tree': False})
        sc = r.get('structuredContent') or {}
        img = next((c for c in r.get('content', []) if c.get('type') == 'image'), None)
        err = sc.get('screenshot_error') or {}
        route, message = readiness()
        entry = {'image': img is not None, 'screenshot_error': err, 'readiness_window_capture': route, 'readiness_message': message}
        if img:
            png = base64.b64decode(img['data']); (work/f'{name}.png').write_bytes(png); entry['magenta_px'] = magenta(png)
        want_image, want_route, named = EXPECTED[name]
        entry['ok'] = (entry['image'] == want_image and route == want_route
            and (want_image or (err.get('code') == UNPROVEN and REASON in json.dumps(err) and REASON in (message or '')))
            and (not want_image or entry.get('magenta_px') == 0))
        report['cases'][name] = entry
        print('PASS' if entry['ok'] else 'FAIL', name, 'image' if img else 'no-image', entry.get('magenta_px'), err.get('code'), route, flush=True)
    case('target_alone')
    bg = launch(['swaybg', '-c', '#00ff00'], 'swaybg'); time.sleep(1.5); case('swaybg_connected', bg); stop(bg)
    lw = launch(['/usr/bin/python3', str(layer_fx), str(work/'layer-ready.json')], 'layer', {'GDK_BACKEND': 'wayland'})
    wait_for(lambda: (work/'layer-ready.json').exists(), 15, 'layer'); time.sleep(1)
    report['move_toplevel'] = sway(f'[pid={lw.pid}] move container to workspace 2').stdout; time.sleep(1)
    ipc = {kind: sway('-r', '-t', kind).stdout for kind in ('get_tree', 'get_outputs', 'get_workspaces', 'get_seats')}
    (work/'ipc-with-overlay.json').write_text(json.dumps({k: json.loads(v) if v.strip() else None for k, v in ipc.items()}, indent=1))
    # Does any IPC reply place the overlay? Its pid appears once (the toplevel's
    # container); look for a node of the overlay's size or position.
    report['ipc_places_overlay'] = any('"width": 300' in v and '"height": 150' in v for v in ipc.values()) or any('"x": 150' in v and '"y": 150' in v for v in ipc.values())
    report['ipc_mentions_layer_pid'] = {k: v.count(f'"pid": {lw.pid}') for k, v in ipc.items()}
    case('layer_overlay_from_client_with_a_toplevel', lw); stop(lw)
    (work/'layer-ready.json').unlink()
    lw = launch(['/usr/bin/python3', str(layer_fx), str(work/'layer-ready.json'), 'nowindow'], 'layer2', {'GDK_BACKEND': 'wayland'})
    wait_for(lambda: (work/'layer-ready.json').exists(), 15, 'layer2'); time.sleep(1)
    case('layer_overlay_from_client_without_a_toplevel', lw); stop(lw)
    g2 = launch(['/usr/bin/python3', str(grid_fx), str(work), 'W2'], 'grid2', {'GDK_BACKEND': 'wayland'})
    wait_for(lambda: (work/f'fixture-{g2.pid}.json').exists(), 15, 'grid2')
    sway(f'[pid={g2.pid}] move container to workspace 2'); time.sleep(1)
    case('second_windowed_client_on_another_workspace', g2); stop(g2)
    case('target_alone_again')
    report['done'] = True
except Exception as e:
    report.update(error=str(e), traceback=traceback.format_exc())
finally:
    signal.alarm(0)
    for p in reversed(procs):
        if p.poll() is None:
            try: os.killpg(p.pid, signal.SIGTERM)
            except ProcessLookupError: pass
    for l in logs: l.close()
    failures = sorted(k for k, v in report['cases'].items() if not v['ok']) + sorted(set(EXPECTED) - set(report['cases']))
    report['failures'] = failures; report['ok'] = bool(report.get('done')) and not failures
    (work/'result.json').write_text(json.dumps(report, indent=1) + '\n')
    print('RESULT', 'ok' if report['ok'] else f'fail {failures}', report.get('error', ''))
    sys.exit(0 if report['ok'] else 1)
