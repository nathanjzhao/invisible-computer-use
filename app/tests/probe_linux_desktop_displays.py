"""The full-desktop facade on several outputs and on a scaled one, against a real X11 or Wayland server.

    probe_linux_desktop_displays.py WORK MODE NATIVE_DRIVER ICU FIXTURES_DIR

MODE is one of
  x11-two        Xorg (dummy video driver), two RandR monitors side by side: 1280x1024 at +0+0 and 1920x1080 at +1280+0
  xvfb-two       Xvfb 3200x1000 with two RandR monitors made by `xrandr --setmonitor`: 1600x1000 at +0+0 and at +1600+0
  x11-scale-hidpi  Xorg with the modesetting driver on a vkms KMS device (`modprobe vkms`; the dummy driver, Xvfb, Xvnc and Xephyr refuse a CRTC
                 transform, modesetting accepts it): one 1600x1000 mode scaled `xrandr --scale 0.5x0.5`, so the desktop is 800x500 logical pixels, each
                 shown as 2x2 of the output's (an output scale of 2)
  x11-scale-over   the same output at `--scale 2x2`: the desktop is 3200x2000 logical pixels shown on a 1600x1000 mode (a root larger than the mode)
  sway-scale2    headless sway, one output 1600x1000 at scale 2 (800x500 logical)
  sway-two       headless sway, two outputs 1600x1000 at scale 1, the second right of the first
  sway-mixed     headless sway, 1600x1000 at scale 1 and a second 1600x1000 output at scale 2 right of it
  sway-first-right  headless sway, two outputs, the first one at x=1600 and the second at x=0
  sway-first-hidpi  headless sway, the first output at scale 2 (800x500 logical) and a second 1600x1000 output at scale 1 right of it

One receiver window per output (fixtures/wayland_output_receiver.py, fixtures/x11_monitor_receiver.py)
paints a coloured 60x60 square at the centre of its output and logs what the server delivers to it. Each
case calls `icu call desktop_*` over the shipped driver and reads the answer from those logs, never from the
driver's own reply:

  geometry  what the screenshot is: its size, the screen the driver reports, and where every receiver's marker is in it.
  click     for every marker found in the screenshot: a click at the centre of that marker, taken from the PNG pixels, is
            delivered to that receiver, at the centre of its marker, and to no other receiver.
  edges     a click at the last pixel of the screenshot is delivered somewhere on the desktop; one past it is refused.
  scroll    a wheel turn at a marker is delivered to that receiver.
Exit 0 only when every case holds; the geometry case records facts and does not fail.
"""
import base64, hashlib, json, os, pathlib, select, signal, struct, subprocess, sys, time, traceback, zlib

work = pathlib.Path(sys.argv[1]); work.mkdir(parents=True)
MODE, NATIVE, ICU, FIX = sys.argv[2], pathlib.Path(sys.argv[3]).resolve(), pathlib.Path(sys.argv[4]).resolve(), pathlib.Path(sys.argv[5]).resolve()
X11 = MODE.startswith(('x11', 'xvfb'))
procs, logs = [], []
report = {'mode': MODE, 'driver_sha256': hashlib.sha256(NATIVE.read_bytes()).hexdigest(), 'icu_sha256': hashlib.sha256(ICU.read_bytes()).hexdigest(),
          'probe_sha256': hashlib.sha256(pathlib.Path(__file__).read_bytes()).hexdigest(), 'cases': {}, 'facts': {}}
env = {k: v for k, v in os.environ.items() if k not in ('DISPLAY', 'WAYLAND_DISPLAY')}
env.update(XDG_RUNTIME_DIR=str(work / 'runtime'), XDG_SESSION_TYPE='x11' if X11 else 'wayland')
(work / 'runtime').mkdir(); (work / 'runtime').chmod(0o700)

OUTPUTS = {  # tag, colour, output name, (x, y, w, h) in logical coordinates
    'x11-two': [('A', 'ff0000', 'DUMMY1', (0, 0, 1280, 1024)), ('B', '0000ff', 'DUMMY0', (1280, 0, 1920, 1080))],
    'xvfb-two': [('A', 'ff0000', 'left', (0, 0, 1600, 1000)), ('B', '0000ff', 'right', (1600, 0, 1600, 1000))],
    'x11-scale-hidpi': [('A', 'ff0000', 'Virtual-1', (0, 0, 800, 500))],
    'x11-scale-over': [('A', 'ff0000', 'Virtual-1', (0, 0, 3200, 2000))],
    'sway-scale2': [('A', 'ff0000', 'HEADLESS-1', (0, 0, 800, 500))],
    'sway-two': [('A', 'ff0000', 'HEADLESS-1', (0, 0, 1600, 1000)), ('B', '0000ff', 'HEADLESS-2', (1600, 0, 1600, 1000))],
    'sway-mixed': [('A', 'ff0000', 'HEADLESS-1', (0, 0, 1600, 1000)), ('B', '0000ff', 'HEADLESS-2', (1600, 0, 800, 500))],
    'sway-first-right': [('A', 'ff0000', 'HEADLESS-1', (1600, 0, 1600, 1000)), ('B', '0000ff', 'HEADLESS-2', (0, 0, 1600, 1000))],
    'sway-first-hidpi': [('A', 'ff0000', 'HEADLESS-1', (0, 0, 800, 500)), ('B', '0000ff', 'HEADLESS-2', (800, 0, 1600, 1000))],
}[MODE]
SWAY_OUT = {'sway-scale2': [('HEADLESS-1', '1600x1000', 0, 0, 2)],
            'sway-two': [('HEADLESS-1', '1600x1000', 0, 0, 1), ('HEADLESS-2', '1600x1000', 1600, 0, 1)],
            'sway-mixed': [('HEADLESS-1', '1600x1000', 0, 0, 1), ('HEADLESS-2', '1600x1000', 1600, 0, 2)],
            'sway-first-right': [('HEADLESS-1', '1600x1000', 1600, 0, 1), ('HEADLESS-2', '1600x1000', 0, 0, 1)],
            'sway-first-hidpi': [('HEADLESS-1', '1600x1000', 0, 0, 2), ('HEADLESS-2', '1600x1000', 800, 0, 1)]}.get(MODE, [])
XORG_CONF = '''Section "Device"\n Identifier "d"\n Driver "dummy"\n VideoRam 256000\nEndSection\nSection "Screen"\n Identifier "s"\n Device "d"\n DefaultDepth 24\n SubSection "Display"\n  Depth 24\n  Virtual 3600 2200\n EndSubSection\nEndSection\n'''
MS_CONF = '''Section "ServerFlags"\n Option "AutoAddDevices" "false"\n Option "AutoEnableDevices" "false"\n Option "DontVTSwitch" "true"\nEndSection\nSection "Device"\n Identifier "vkms"\n Driver "modesetting"\n Option "kmsdev" "/dev/dri/card0"\n Option "AccelMethod" "none"\nEndSection\nSection "Screen"\n Identifier "s"\n Device "vkms"\nEndSection\n'''
MODES = {'1920x1080': '173.00 1920 2048 2248 2576 1080 1083 1088 1120 -hsync +vsync'.split(), '1280x1024': '109.00 1280 1368 1496 1712 1024 1027 1034 1063 -hsync +vsync'.split(),
         '1600x1000': '131.00 1600 1704 1872 2144 1000 1003 1013 1050 -hsync +vsync'.split()}
CONTROL = 4


def launch(args, name, stdout=None, extra=None):
    log = (work / (name + '.log')).open('wb'); logs.append(log)
    p = subprocess.Popen(args, env={**env, **(extra or {})}, stdin=subprocess.DEVNULL, stdout=stdout or log, stderr=log, start_new_session=True); procs.append(p); return p


def run(*args, check=True):
    r = subprocess.run(list(args), env=env, capture_output=True, text=True, timeout=20)
    if check and r.returncode: raise RuntimeError(f'{args}: {r.stderr[-300:]}')
    return r.stdout


def wait_for(pred, s, msg):
    d = time.monotonic() + s
    while time.monotonic() < d:
        r = pred()
        if r: return r
        time.sleep(.05)
    raise RuntimeError(msg)


def png_rows(png):
    assert png[:8] == b'\x89PNG\r\n\x1a\n'
    pos, chunks, idat = 8, {}, b''
    while pos < len(png):
        n, t = struct.unpack('>I4s', png[pos:pos + 8]); body = png[pos + 8:pos + 8 + n]; pos += 12 + n
        if t == b'IDAT': idat += body
        else: chunks[t] = body
    w, h, depth, ctype, _, _, interlace = struct.unpack('>IIBBBBB', chunks[b'IHDR'])
    assert depth == 8 and interlace == 0 and ctype in (2, 6)
    bpp = 3 if ctype == 2 else 4
    raw, stride, rows, prev = zlib.decompress(idat), w * bpp, [], bytearray(w * bpp)
    for y in range(h):
        f, line = raw[y * (stride + 1)], bytearray(raw[y * (stride + 1) + 1:(y + 1) * (stride + 1)])
        if f == 1:
            for i in range(bpp, stride): line[i] = (line[i] + line[i - bpp]) & 255
        elif f == 2:
            for i in range(stride): line[i] = (line[i] + prev[i]) & 255
        elif f == 3:
            for i in range(stride): line[i] = (line[i] + ((line[i - bpp] if i >= bpp else 0) + prev[i]) // 2) & 255
        elif f == 4:
            for i in range(stride):
                a = line[i - bpp] if i >= bpp else 0; b = prev[i]; c = prev[i - bpp] if i >= bpp else 0
                p = a + b - c; pa, pb, pc = abs(p - a), abs(p - b), abs(p - c)
                line[i] = (line[i] + (a if pa <= pb and pa <= pc else b if pb <= pc else c)) & 255
        rows.append(bytes(line)); prev = line
    return w, h, bpp, rows


def find_marker(png, rgb):
    """The bounding box in PNG pixels of the exact colour `rgb`, scanning every 2nd pixel and refining."""
    w, h, bpp, rows = png_rows(png)
    want = bytes(rgb); hits = [(x, y) for y in range(0, h, 2) for x in range(0, w, 2) if rows[y][x * bpp:x * bpp + 3] == want]
    if not hits: return None
    x0, y0, x1, y1 = min(p[0] for p in hits), min(p[1] for p in hits), max(p[0] for p in hits), max(p[1] for p in hits)
    return x0, y0, x1, y1


def icu(tool, args):
    e = {**env, 'ICU_DRIVER': str(NATIVE), 'ICU_DRIVER_DAEMON': '0', 'ICU_HOME': str(work / 'icu-home'), 'ICU_STATE_DIR': str(work / 'icu-state'),
         'ICU_REQUIRE_APP_APPROVAL': '0', 'ICU_CURSOR': 'off', 'ICU_POST_ACTION_SLEEP_MS': '0'}
    if not X11: e['CUA_DRIVER_RS_ENABLE_WAYLAND'] = '1'
    r = subprocess.run([str(ICU), 'call', tool, json.dumps(args)], env=e, capture_output=True, timeout=120)
    try: out = json.loads(r.stdout)
    except Exception: out = {'_raw': r.stdout.decode(errors='replace')[:400]}
    return r.returncode, out, r.stderr.decode(errors='replace')[-300:]


def events(tag):
    p = work / f'events-{tag}.jsonl'
    return [json.loads(l) for l in p.read_text().splitlines()] if p.exists() else []


def all_events(): return {t: events(t) for t, *_ in OUTPUTS}


def record(name, ok, detail, **more):
    report['cases'][name] = {'ok': bool(ok), 'detail': detail, **more}
    print('PASS' if ok else 'FAIL', name, detail, flush=True)


signal.signal(signal.SIGALRM, lambda *_: (_ for _ in ()).throw(TimeoutError('deadline'))); signal.alarm(900)
try:
    if MODE == 'xvfb-two':
        x = launch(['Xvfb', '-displayfd', '1', '-screen', '0', '3200x1000x24', '+extension', 'RANDR', '-nolisten', 'tcp'], 'xvfb', subprocess.PIPE)
        assert select.select([x.stdout], [], [], 10)[0]; env['DISPLAY'] = ':' + x.stdout.readline().decode().strip()
        run('xrandr', '--setmonitor', 'left', '1600/423x1000/265+0+0', 'screen'); run('xrandr', '--setmonitor', 'right', '1600/423x1000/265+1600+0', 'none')
        report['facts']['xrandr_monitors'] = run('xrandr', '--listmonitors'); report['facts']['xrandr_root'] = run('xdpyinfo').split('dimensions:')[1].split('pixels')[0].strip()
        for tag, color, _o, (x0, y0, w, h) in OUTPUTS:
            launch(['/usr/bin/python3', str(FIX / 'x11_monitor_receiver.py'), str(work), tag, color, str(x0), str(y0), str(w), str(h)], f'receiver-{tag}')
    elif X11:
        scaled = MODE.startswith('x11-scale')
        (work / 'xorg.conf').write_text(MS_CONF if scaled else XORG_CONF)
        xorg = launch(['Xorg', '-displayfd', '1', '-config', str(work / 'xorg.conf'), '-noreset', '-nolisten', 'tcp', '-logfile', str(work / 'xorg-server.log')] + (['-novtswitch', '-sharevts'] if scaled else []), 'xorg', subprocess.PIPE)
        assert select.select([xorg.stdout], [], [], 20)[0], 'Xorg startup'
        env['DISPLAY'] = ':' + xorg.stdout.readline().decode().strip()
        for mode, timings in MODES.items(): run('xrandr', '--newmode', mode, *timings, check=False)
        if scaled:
            sc = '0.5x0.5' if MODE == 'x11-scale-hidpi' else '2x2'
            run('xrandr', '--addmode', 'Virtual-1', '1600x1000')
            # PROBE_X11_SCALE_CONTROL=1 leaves the output unscaled: the next check has to go red (the control for "the scale is really in force")
            run('xrandr', '--output', 'Virtual-1', '--mode', '1600x1000', *([] if os.environ.get('PROBE_X11_SCALE_CONTROL') else ['--scale', sc]))
            verbose = run('xrandr', '--verbose'); report['facts']['xrandr_verbose'] = verbose[:1500]
            want = 0.5 if MODE == 'x11-scale-hidpi' else 2.0
            tr = [float(l.split(':')[1].split()[0]) for l in verbose.splitlines() if l.strip().startswith('Transform:')]
            root = run('xdpyinfo').split('dimensions:')[1].split('pixels')[0].strip()
            record('scale_is_in_force', bool(tr) and tr[0] == want and root == ('800x500' if want == 0.5 else '3200x2000'), f'CRTC transform scale {tr[:1]}, want {want}; root window {root} on a 1600x1000 mode')
        if MODE == 'x11-two':
            lay = []
            for name, mode, x, y in (('DUMMY1', '1280x1024', 0, 0), ('DUMMY0', '1920x1080', 1280, 0)):
                run('xrandr', '--addmode', name, mode); lay += ['--output', name, '--mode', mode, '--pos', f'{x}x{y}']
            run('xrandr', *lay, '--output', 'DUMMY0', '--primary')
        report['facts']['xrandr_monitors'] = run('xrandr', '--listmonitors'); report['facts']['xrandr_root'] = run('xdpyinfo').split('dimensions:')[1].split('pixels')[0].strip()
        for tag, color, _o, (x, y, w, h) in OUTPUTS:
            launch(['/usr/bin/python3', str(FIX / 'x11_monitor_receiver.py'), str(work), tag, color, str(x), str(y), str(w), str(h)], f'receiver-{tag}')
    else:
        rules = ''.join(f'for_window [title="icu receiver {tag}"] move to output {out}, fullscreen enable\n' for tag, _c, out, _g in OUTPUTS)
        (work / 'sway.conf').write_text(f'output HEADLESS-1 resolution 1600x1000 position 0 0\n{rules}')
        launch(['sway', '-c', str(work / 'sway.conf')], 'sway', extra={'WLR_BACKENDS': 'headless', 'WLR_LIBINPUT_NO_DEVICES': '1', 'WLR_RENDERER': 'pixman'})
        wait_for(lambda: any(not str(p).endswith('.lock') for p in (work / 'runtime').glob('wayland-*')), 15, 'sway socket')
        env['WAYLAND_DISPLAY'] = next(p for p in (work / 'runtime').glob('wayland-*') if not str(p).endswith('.lock')).name
        sock = next(p for p in (work / 'runtime').glob('sway-ipc.*.sock')); env['SWAYSOCK'] = str(sock)
        if len(SWAY_OUT) > 1: run('swaymsg', 'create_output'); time.sleep(.5)
        for name, mode, x, y, scale in SWAY_OUT: run('swaymsg', 'output', name, 'mode', mode, 'position', str(x), str(y), 'scale', str(scale))
        time.sleep(.5)
        report['facts']['sway_outputs'] = json.loads(run('swaymsg', '-t', 'get_outputs', '-r'))
        report['facts']['sway_outputs'] = [{k: o.get(k) for k in ('name', 'scale', 'rect', 'current_mode', 'active')} for o in report['facts']['sway_outputs']]
        for tag, color, out, _g in OUTPUTS:
            launch(['/usr/bin/python3', str(FIX / 'wayland_output_receiver.py'), str(work), tag, color], f'receiver-{tag}')
            time.sleep(1.2)
    for t, *_ in OUTPUTS: wait_for(lambda t=t: (work / f'ready-{t}.json').exists(), 25, f'receiver {t} ready')
    time.sleep(3)  # the compositor places and fullscreens the windows; each receiver rewrites its ready file every 500 ms
    ready = {t: json.loads((work / f'ready-{t}.json').read_text()) for t, *_ in OUTPUTS}
    for t, *_ in OUTPUTS: (work / f'events-{t}.jsonl').unlink(missing_ok=True)
    report['facts']['receivers'] = ready
    time.sleep(1)

    # ---- geometry --------------------------------------------------------------------------------
    code, shot, err = icu('desktop_screenshot', {'mouse_size_px': 0})
    assert code == 0, (code, str(shot)[:300], err)
    png = base64.b64decode(shot['screenshots'][0]['url'].split('base64,', 1)[1]); (work / 'shot.png').write_bytes(png)
    pw, ph, *_ = png_rows(png)
    report['facts']['screenshot'] = {'png': [pw, ph], 'screen': shot.get('screen'), 'reply_keys': sorted(k for k in shot if k != 'screenshots')}
    rgb = lambda c: tuple(int(c[i:i + 2], 16) for i in (0, 2, 4))
    markers = {}
    for tag, color, out, geo in OUTPUTS:
        box = find_marker(png, rgb(color)); markers[tag] = box
        r = ready[tag]; m = r['marker']
        # where the marker is in desktop coordinates (the receiver's own, logical ones): X11 reports root coordinates, Wayland window ones
        want_logical = (m[0] if X11 else geo[0] + m[0], m[1] if X11 else geo[1] + m[1])
        report['facts'][f'marker_{tag}'] = {'png_box': box, 'logical_origin': want_logical, 'output_logical_geometry': geo, 'receiver': r}
    record('geometry_png_size_and_markers', True, f'png {pw}x{ph}, screen {shot.get("screen")}, markers in the png: {markers}')

    # ---- click at every marker found in the png -------------------------------------------------
    def logical_center(tag):
        m = ready[tag]['marker']; geo = next(g for t, _c, _o, g in OUTPUTS if t == tag)
        return ((m[0] if X11 else geo[0] + m[0]) + 30, (m[1] if X11 else geo[1] + m[1]) + 30)
    for tag, color, out, geo in OUTPUTS:
        box = markers[tag]
        if not box and not X11 and tag != OUTPUTS[0][0]:
            # on Wayland the desktop is the first output (the driver's `display_id: primary`): the others are not in the screenshot, and the
            # pointer is bound to the first output, so no desktop click can land on them. Stated, and checked below that none does.
            record(f'output_{tag}_is_not_in_the_screenshot_and_nothing_reaches_it', True, f'the {color} marker of output {out} is not in the screenshot (png {pw}x{ph}); the desktop is the first output', in_png=False); continue
        if not box:
            record(f'click_marker_{tag}', False, f'the {color} marker of output {out} is not in the screenshot (png {pw}x{ph}): the desktop verbs cannot reach it', in_png=False); continue
        cx, cy = (box[0] + box[2]) // 2, (box[1] + box[3]) // 2
        before = all_events()
        code, out_, err = icu('desktop_click', {'x': cx, 'y': cy}); time.sleep(.9)
        after = all_events(); got = {t: after[t][len(before[t]):] for t in after}
        mine = [e for e in got[tag] if e['k'] in ('down', 'up')]; others = {t: len([e for e in g if e['k'] in ('down', 'up')]) for t, g in got.items() if t != tag}
        want = ready[tag]['marker']; wl = (want[0] + 30, want[1] + 30)
        # the receiver's coordinates are window-local on Wayland (marker centre = want + 30) and root coordinates on X11 (the same, offset by the window origin in `marker`)
        d = [e for e in mine if e['k'] == 'down']
        ok = code == 0 and [e['k'] for e in mine] == ['down', 'up'] and abs(d[0]['x'] - wl[0]) <= 2 and abs(d[0]['y'] - wl[1]) <= 2 and not any(others.values())
        record(f'click_marker_{tag}', ok, f'click at png ({cx}, {cy}) -> {[(e["k"], round(e["x"], 1), round(e["y"], 1)) for e in mine]} want ({wl[0]}, {wl[1]}); other receivers {others}; exit {code} {str(out_)[:160] if code else ""}', png_click=[cx, cy], delivered=mine)
        # a wheel turn at the same pixel reaches the same receiver
        before = all_events()
        code, out_, err = icu('desktop_scroll', {'direction': 'down', 'pixels': 80, 'x': cx, 'y': cy}); time.sleep(.9)
        after = all_events(); got = {t: after[t][len(before[t]):] for t in after}
        wheel = [e for e in got[tag] if (e['k'] == 'scroll') or (e['k'] == 'down' and e.get('b') in (4, 5))]
        record(f'scroll_marker_{tag}', code == 0 and bool(wheel) and not any(len([e for e in g if e['k'] == 'scroll' or (e['k'] == 'down' and e.get('b') in (4, 5))]) for t, g in got.items() if t != tag), f'{len(wheel)} wheel events to {tag}, others {[(t, len(g)) for t, g in got.items() if t != tag]}')

    # ---- the edges of the screenshot ------------------------------------------------------------
    code, out_, err = icu('desktop_click', {'x': pw, 'y': 5})
    record('edge_one_past_the_right_is_refused', code == 2 and isinstance(out_, dict) and out_.get('reason') == 'point_off_screen', f'exit {code}, reason {out_.get("reason") if isinstance(out_, dict) else out_}')
    first = OUTPUTS[0][0]
    corners = {'top_left': (0, 0), 'top_right': (pw - 1, 0), 'bottom_left': (0, ph - 1), 'bottom_right': (pw - 1, ph - 1), 'right_middle': (pw - 1, ph // 2), 'bottom_middle': (pw // 2, ph - 1)}
    for cname, (ex, ey) in corners.items():
        before = all_events()
        code, out_, err = icu('desktop_click', {'x': ex, 'y': ey}); time.sleep(.8)
        after = all_events()
        n = {t: len(after[t]) - len(before[t]) for t in after}
        # On Wayland the receiver window is a sway window with a title bar and borders (its surface starts at about 2,25), so it does not
        # cover every corner of its output: a click there may hit no receiver. What must hold is that it never reaches another output; the
        # two corners that the window does cover (and the centre, above) prove delivery.
        if X11 or cname == 'top_left': ok = code == 0 and sum(n.values()) > 0
        else: ok = code == 0 and not any(v for t, v in n.items() if t != first)
        record(f'edge_{cname}_is_delivered' + ('' if X11 else '_to_the_screenshots_output_or_nowhere'), ok, f'click at png ({ex}, {ey}): exit {code}, events per receiver {n}')
    report['done'] = True
except Exception as e:
    report.update(error=str(e), traceback=traceback.format_exc())
finally:
    signal.alarm(0)
    (work / 'quit').write_text('1')
    time.sleep(.3)
    for p in reversed(procs):
        if p.poll() is None:
            try: os.killpg(p.pid, signal.SIGTERM)
            except ProcessLookupError: pass
    for p in reversed(procs):
        try: p.wait(timeout=3)
        except subprocess.TimeoutExpired: os.killpg(p.pid, signal.SIGKILL)
    for l in logs: l.close()
    failures = sorted(k for k, v in report['cases'].items() if not v['ok'])
    report['failures'] = failures; report['ok'] = bool(report.get('done')) and not failures
    (work / 'result.json').write_text(json.dumps(report, indent=1) + '\n')
    print('RESULT', 'ok' if report['ok'] else 'FAILED', 'cases', len(report['cases']), 'failures', failures, report.get('error', ''), flush=True)
sys.exit(0 if report['ok'] else 1)
