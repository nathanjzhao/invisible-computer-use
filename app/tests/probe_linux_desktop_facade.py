"""The full-desktop facade (desktop_screenshot, desktop_click, desktop_drag, desktop_move) on a real Linux desktop.

    probe_linux_desktop_facade.py WORK MODE NATIVE_DRIVER STOCK_DRIVER ICU RECEIVER

MODE is `x11` (Xvfb 1600x1000, no window manager; RECEIVER is fixtures/x11_desktop_receiver.py) or
`sway` (headless sway, output 1600x1000, `CUA_DRIVER_RS_ENABLE_WAYLAND=1`; RECEIVER is
fixtures/wayland_desktop_receiver.py). NATIVE_DRIVER is the patched cua-driver with the desktop-scope
hold, path and modifier support; STOCK_DRIVER is the same build without it. ICU is the layer binary.

Every case calls `icu call desktop_*` (the shipped entry point, the same code path as `icu mcp`) over
a real driver, and reads the result from what the display server delivered to a whole-screen
receiver window (server timestamps, root coordinates, button and modifier state), never from the
driver's own reply. Cases:

  S  screenshot: the whole desktop at the screen size; the receiver's red marker square is found in
     the PNG at exactly the pixels it was drawn at, and a click at the marker's centre (a PNG
     pixel) is delivered at that desktop pixel; the agent cursor is drawn into the next screenshot
     with its tip at the click pixel.
  C  click: position and button; click_count 2 is two press-release pairs; `duration` holds the
     button down (server timestamps), a click without it does not; a held `key` chord is down
     before the press and in the press's modifier state (x11).
  D  drag: one press at the first point, motion through every corner of a four-point path in order
     with the button held, one release at the last point; the same under a held Shift (x11); the
     contract's own two-point drag on the stock driver.
  M  move: pointer motion to the point with no button; under a held Alt (x11).
  O  a point outside the screen refuses invalid_arguments on every verb with nothing delivered.
  K  stock driver: duration, a longer path and a held key refuse desktopUnavailable with nothing
     delivered; a plain click, move and two-point drag still work.
Exit 0 only when every case holds.
"""
import base64, hashlib, json, os, pathlib, select, signal, struct, subprocess, sys, time, traceback, zlib

work = pathlib.Path(sys.argv[1]); work.mkdir(parents=True)
MODE, NATIVE, STOCK, ICU, RECV = sys.argv[2], *(pathlib.Path(a).resolve() for a in sys.argv[3:7])
assert MODE in ('x11', 'sway'), MODE
X11 = MODE == 'x11'
W, H = 1600, 1000
procs = []; logs = []
report = {'mode': MODE, 'driver_sha256': {n: hashlib.sha256(p.read_bytes()).hexdigest() for n, p in (('native', NATIVE), ('stock', STOCK))},
          'icu_sha256': hashlib.sha256(ICU.read_bytes()).hexdigest(), 'probe_sha256': hashlib.sha256(pathlib.Path(__file__).read_bytes()).hexdigest(), 'cases': {}}
env = {k: v for k, v in os.environ.items() if k not in ('DISPLAY', 'WAYLAND_DISPLAY')}
env.update(XDG_RUNTIME_DIR=str(work / 'runtime'), XDG_SESSION_TYPE='x11' if X11 else 'wayland')
(work / 'runtime').mkdir(); (work / 'runtime').chmod(0o700)
SHIFT, CONTROL, MOD1, BUTTON1 = 1, 4, 8, 0x100


def launch(args, name, stdout=None, extra=None):
    log = (work / (name + '.log')).open('wb'); logs.append(log)
    p = subprocess.Popen(args, env={**env, **(extra or {})}, stdin=subprocess.DEVNULL, stdout=stdout or log, stderr=log, start_new_session=True); procs.append(p); return p


def wait_for(pred, s, msg):
    d = time.monotonic() + s
    while time.monotonic() < d:
        r = pred()
        if r: return r
        time.sleep(.05)
    raise RuntimeError(msg)


def png_pixels(png):
    assert png[:8] == b'\x89PNG\r\n\x1a\n'
    pos, chunks, idat = 8, {}, b''
    while pos < len(png):
        n, t = struct.unpack('>I4s', png[pos:pos + 8]); body = png[pos + 8:pos + 8 + n]; pos += 12 + n
        if t == b'IDAT': idat += body
        else: chunks[t] = body
    w, h, depth, ctype, _, _, interlace = struct.unpack('>IIBBBBB', chunks[b'IHDR'])
    assert depth == 8 and interlace == 0 and ctype in (2, 6), (depth, ctype, interlace)
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


def shot_pixels(result):
    url = result['screenshots'][0]['url']
    return png_pixels(base64.b64decode(url.split('base64,', 1)[1]))


def start_daemon(driver, name, extra=None):
    """A persistent driver (`cua-driver serve --socket S`): a drag handle keeps its button across calls only in a process that lives."""
    sock = work / f'{name}.sock'
    launch([str(driver), 'serve', '--no-overlay', '--socket', str(sock)], name, extra={**({} if X11 else {'CUA_DRIVER_RS_ENABLE_WAYLAND': '1'}), **(extra or {})})
    wait_for(lambda: sock.exists(), 20, f'{name} socket')
    time.sleep(.5)
    return sock


def icu(driver, tool, args, socket=None, state='icu-state'):
    e = {**env, 'ICU_DRIVER': str(driver), 'ICU_DRIVER_DAEMON': '0', 'ICU_HOME': str(work / 'icu-home'), 'ICU_STATE_DIR': str(work / state),
         'ICU_REQUIRE_APP_APPROVAL': '0', 'ICU_CURSOR': 'off', 'ICU_POST_ACTION_SLEEP_MS': '0'}
    if socket: e['ICU_SOCKET'] = str(socket)
    if not X11: e['CUA_DRIVER_RS_ENABLE_WAYLAND'] = '1'
    t0 = time.monotonic()
    r = subprocess.run([str(ICU), 'call', tool, json.dumps(args)], env=e, capture_output=True, timeout=120)
    try: out = json.loads(r.stdout)
    except Exception: out = {'_raw': r.stdout.decode(errors='replace')[:400]}
    return r.returncode, out, r.stderr.decode(errors='replace')[-300:], round(time.monotonic() - t0, 2)


class McpSession:
    """`icu mcp` over stdio: the entry a model client uses. The layer starts one driver child (`<driver> mcp`) that lives as long as this session, so a
    drag handle's button, held by that driver process, survives from one tools/call to the next. With restart_each the session is torn down and started
    again before every call, which is the process lifetime of `icu call` (the control that has to lose the release)."""
    def __init__(self, driver, state, restart_each=False):
        self.driver, self.state, self.restart_each, self.p, self.n = driver, state, restart_each, None, 0
        self.log = (work / f'mcp-{state}.log').open('ab'); logs.append(self.log)

    def open(self):
        e = {**env, 'ICU_DRIVER': str(self.driver), 'ICU_DRIVER_DAEMON': '0', 'ICU_HOME': str(work / 'icu-home'), 'ICU_STATE_DIR': str(work / self.state),
             'ICU_REQUIRE_APP_APPROVAL': '0', 'ICU_CURSOR': 'off', 'ICU_POST_ACTION_SLEEP_MS': '0'}
        if not X11: e['CUA_DRIVER_RS_ENABLE_WAYLAND'] = '1'
        self.p = subprocess.Popen([str(ICU), 'mcp'], env=e, stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=self.log, start_new_session=True)
        self.rpc('initialize', {'protocolVersion': '2024-11-05', 'capabilities': {}, 'clientInfo': {'name': 'probe', 'version': '1'}})
        self.p.stdin.write(b'{"jsonrpc":"2.0","method":"notifications/initialized"}\n'); self.p.stdin.flush()

    def rpc(self, method, params, timeout=60):
        self.n += 1
        self.p.stdin.write((json.dumps({'jsonrpc': '2.0', 'id': self.n, 'method': method, 'params': params}) + '\n').encode()); self.p.stdin.flush()
        d = time.monotonic() + timeout
        while time.monotonic() < d:
            if not select.select([self.p.stdout], [], [], .2)[0]: continue
            line = self.p.stdout.readline()
            if not line: raise RuntimeError('icu mcp closed its output')
            m = json.loads(line)
            if m.get('id') == self.n: return m
        raise TimeoutError(method)

    def close(self):
        if self.p and self.p.poll() is None:
            try: self.p.stdin.close()
            except OSError: pass
            try: self.p.wait(timeout=5)
            except subprocess.TimeoutExpired: os.killpg(self.p.pid, signal.SIGKILL)
        self.p = None

    def call(self, tool, args):
        if self.restart_each: self.close()
        if self.p is None: self.open()
        r = self.rpc('tools/call', {'name': tool, 'arguments': args})
        res = r.get('result', {})
        text = next((c.get('text') for c in res.get('content', []) if c.get('type') == 'text'), '')
        if res.get('isError') and isinstance(res.get('structuredContent'), dict): out = dict(res['structuredContent'])  # a refusal is carried in structuredContent
        else:
            try: out = json.loads(text)
            except Exception: out = {'_raw': (text or '')[:300]}
        if res.get('isError') and isinstance(out, dict): out['_is_error'] = True
        return out


def events():
    p = work / 'events.jsonl'
    return [json.loads(l) for l in p.read_text().splitlines()] if p.exists() else []


def near(ev, x, y, tol=2): return abs(ev['x'] - x) <= tol and abs(ev['y'] - y) <= tol


ONLY = tuple(x for x in os.environ.get('PROBE_ONLY', '').split(',') if x)  # case-name prefixes to run, for a mutant run; empty runs everything


def case(name, driver, tool, args, check, expect_exit=0, socket=None, state='icu-state', settle=.8):
    """Run one call; `check(events, reply)` returns (ok, detail)."""
    if ONLY and not name.startswith(ONLY): return {}
    before = len(events())
    code, out, err, took = icu(driver, tool, args, socket=socket, state=state)
    time.sleep(settle)
    got = events()[before:]
    try: ok, detail = check(got, out)
    except Exception as e: ok, detail = False, f'check raised {e!r}'
    ok = bool(ok) and code == expect_exit
    report['cases'][name] = {'tool': tool, 'args': args, 'exit': code, 'expected_exit': expect_exit, 'reply': {k: v for k, v in out.items() if k != 'screenshots'} if isinstance(out, dict) else out,
                             'events': got[:40], 'n_events': len(got), 'ok': ok, 'detail': detail, 'seconds': took, 'stderr': err}
    print('PASS' if ok else 'FAIL', name, code, detail, flush=True)
    return out


def pairs(got): return [(e['k'], e.get('b')) for e in got if e['k'] in ('down', 'up')]


signal.signal(signal.SIGALRM, lambda *_: (_ for _ in ()).throw(TimeoutError('deadline'))); signal.alarm(1500)
try:
    if X11:
        x = launch(['Xvfb', '-displayfd', '1', '-screen', '0', f'{W}x{H}x24', '-nolisten', 'tcp'], 'xvfb', subprocess.PIPE)
        assert select.select([x.stdout], [], [], 8)[0]; env['DISPLAY'] = ':' + x.stdout.readline().decode().strip()
        launch(['/usr/bin/python3', str(RECV), str(work)], 'receiver')
    else:
        (work / 'sway.conf').write_text(f'output HEADLESS-1 resolution {W}x{H} position 0 0\n')
        launch(['sway', '-c', str(work / 'sway.conf')], 'sway', extra={'WLR_BACKENDS': 'headless', 'WLR_LIBINPUT_NO_DEVICES': '1', 'WLR_RENDERER': 'pixman'})
        wait_for(lambda: any((work / 'runtime').glob('wayland-*')) and not str(next((work / 'runtime').glob('wayland-*'))).endswith('.lock'), 15, 'sway socket')
        env['WAYLAND_DISPLAY'] = next(p for p in (work / 'runtime').glob('wayland-*') if not str(p).endswith('.lock')).name
        launch(['/usr/bin/python3', str(RECV), str(work)], 'receiver')
    ready = wait_for(lambda: (work / 'ready.json').exists() and json.loads((work / 'ready.json').read_text()), 20, 'receiver ready')
    report['receiver'] = ready; MARK = tuple(ready['marker'])
    time.sleep(1.5)

    # ---- S: screenshot, the marker, the cursor ------------------------------------------------
    sizes = {}
    def shot_check(got, out):
        w, h, bpp, rows = shot_pixels(out)
        sizes['shot'] = (w, h)
        red = [(x, y) for y in range(MARK[1] - 3, MARK[1] + MARK[3] + 3) for x in range(MARK[0] - 3, MARK[0] + MARK[2] + 3) if rows[y][x * bpp:x * bpp + 3] == b'\xff\x00\x00']
        box = (min(p[0] for p in red), min(p[1] for p in red), max(p[0] for p in red), max(p[1] for p in red)) if red else None
        want = (MARK[0], MARK[1], MARK[0] + MARK[2] - 1, MARK[1] + MARK[3] - 1)
        report['marker_box'] = box
        sizes['rows1'] = (w, h, bpp, rows)
        (work / 'shot1.png').write_bytes(base64.b64decode(out['screenshots'][0]['url'].split('base64,', 1)[1]))
        return (w, h) == (W, H) and box == want and out['screen']['width'] == W and out['target'] == 'desktop', f'png {w}x{h}, marker box {box} want {want}'
    case('s_screenshot_size_and_marker', NATIVE, 'desktop_screenshot', {'mouse_size_px': 0}, shot_check)
    cx, cy = MARK[0] + MARK[2] // 2, MARK[1] + MARK[3] // 2
    case('s_click_at_a_png_pixel', NATIVE, 'desktop_click', {'x': cx, 'y': cy}, lambda g, o: (pairs(g) == [('down', 1), ('up', 1)] and near(g[0], cx, cy, 1), f'{pairs(g)} at {g[0]["x"], g[0]["y"]}' if g else 'no events'))
    def cursor_check(got, out):
        w, h, bpp, rows = shot_pixels(out); w0, h0, bpp0, rows0 = sizes['rows1']
        diff = [(x, y) for y in range(cy - 4, cy + 30) for x in range(cx - 4, cx + 20) if rows[y][x * bpp:x * bpp + 3] != rows0[y][x * bpp0:x * bpp0 + 3]]
        box = (min(p[0] for p in diff), min(p[1] for p in diff), max(p[0] for p in diff), max(p[1] for p in diff)) if diff else None
        # The driver's own ARGB overlay window (its agent-cursor surface, one per driver process) can be mapped at (0, 0) for
        # a moment after a one-shot `icu call` and land in a capture: measured, it turns pixel (0, 0) to (90, 140, 53) in 2 of
        # 3 unmutated runs. It is the driver's, not the desktop's and not the layer's sprite, so the 32x32 corner is not compared.
        far = [(x, y) for y in range(0, h, 7) for x in range(0, w, 7) if not (cx - 6 <= x <= cx + 24 and cy - 6 <= y <= cy + 32) and not (x < 32 and y < 32) and rows[y][x * bpp:x * bpp + 3] != rows0[y][x * bpp0:x * bpp0 + 3]]
        cur = out.get('_cursor_in_screenshot') or {}
        (work / 'shot2.png').write_bytes(base64.b64decode(out['screenshots'][0]['url'].split('base64,', 1)[1]))
        if far: report['stray_pixels'] = [{'at': p, 'before': list(rows0[p[1]][p[0] * bpp0:p[0] * bpp0 + 3]), 'after': list(rows[p[1]][p[0] * bpp:p[0] * bpp + 3])} for p in far[:6]]
        return bool(box) and abs(box[0] - cx) <= 1 and abs(box[1] - cy) <= 1 and box[3] - box[1] <= 24 and not far and cur.get('size_px') == 20, f'cursor box {box}, stray {len(far)} {far[:6]}, reply {cur}'
    time.sleep(2)  # the previous one-shot driver's own overlay window (pixel 0,0) can still be going away; it is not part of the desktop
    case('s_cursor_drawn_at_the_click_pixel', NATIVE, 'desktop_screenshot', {'mouse_size_px': 20}, cursor_check)

    # ---- C: click ----------------------------------------------------------------------------
    case('c_left_click', NATIVE, 'desktop_click', {'x': 300, 'y': 200}, lambda g, o: (pairs(g) == [('down', 1), ('up', 1)] and near(g[0], 300, 200, 1) and near(g[1], 300, 200, 1), f'{pairs(g)}'))
    case('c_right_double_click', NATIVE, 'desktop_click', {'x': 310, 'y': 210, 'mouse_button': 'r', 'click_count': 2}, lambda g, o: (pairs(g) == [('down', 3), ('up', 3)] * 2 and all(near(e, 310, 210, 1) for e in g if e['k'] in ('down', 'up')), f'{pairs(g)}'))
    def hold(ms_min, ms_max):
        def check(g, o):
            d = [e for e in g if e['k'] == 'down']; u = [e for e in g if e['k'] == 'up']
            held = (u[0]['t'] - d[0]['t']) if d and u else None
            return held is not None and ms_min <= held <= ms_max, f'held {held} ms, want {ms_min}..{ms_max}'
        return check
    case('c_duration_holds_the_button', NATIVE, 'desktop_click', {'x': 320, 'y': 220, 'duration': 600}, hold(550, 1500))
    case('c_no_duration_is_a_quick_click', NATIVE, 'desktop_click', {'x': 320, 'y': 220}, hold(0, 250))
    # A held key: on X11 the XTest key press; on a wlroots compositor the virtual pointer carries no modifier state, so the key is held on the
    # seat's virtual keyboard and the focused client reads it from wl_keyboard.modifiers (before gaps-linux-3 this was refused on sway).
    def chord_click(g, o):
        # a wlroots virtual keyboard leaves a sacrificial Shift tap first (the first event of a fresh seat is dropped); it is not the held key
        held = [(i, e) for i, e in enumerate(g) if e['k'] in ('kdown', 'kup') and e['sym'] in (0xffe3, 'Control_L')]
        order = [e['k'] if e['k'] not in ('kdown', 'kup') else e['k'] + ':' + str(e['sym']) for e in g]
        dn = [i for i, e in enumerate(g) if e['k'] == 'down']
        ok = len(held) == 2 and held[0][1]['k'] == 'kdown' and held[1][1]['k'] == 'kup' and bool(dn) and held[0][0] < dn[0] < held[1][0] and bool(g[dn[0]]['s'] & CONTROL)
        return ok, f'order {order}, press state {g[dn[0]]["s"] if dn else None}'
    case('c_key_chord_held_during_the_click', NATIVE, 'desktop_click', {'x': 330, 'y': 230, 'key': 'Control_L'}, chord_click)

    # ---- D: drag ----------------------------------------------------------------------------
    corners = [(100, 100), (500, 100), (500, 400), (100, 400)]
    path = [{'x': x, 'y': y} for x, y in corners]
    def drag_check(held_mask=BUTTON1, extra_mask=0):
        def check(g, o):
            d = [i for i, e in enumerate(g) if e['k'] == 'down']; u = [i for i, e in enumerate(g) if e['k'] == 'up']
            if len(d) != 1 or len(u) != 1 or not d[0] < u[0]: return False, f'press/release {pairs(g)}'
            if not (near(g[d[0]], *corners[0], 1) and near(g[u[0]], *corners[-1], 1)): return False, f'ends {g[d[0]]["x"], g[d[0]]["y"]} {g[u[0]]["x"], g[u[0]]["y"]}'
            mv = [e for e in g[d[0]:u[0]] if e['k'] == 'move']
            at = -1; seen = []
            for c in corners[1:]:
                hit = next((i for i, e in enumerate(mv) if i > at and near(e, *c, 2)), None)
                if hit is None: return False, f'corner {c} not visited in order; moves {[(e["x"], e["y"]) for e in mv][:30]}'
                at = hit; seen.append(c)
            if X11 and not all((e['s'] & BUTTON1) for e in mv): return False, 'a move during the drag lacks the button-1 state'
            if extra_mask and not all((e['s'] & extra_mask) for e in mv): return False, f'a move lacks the held key state {extra_mask}'
            after = [e for e in g[u[0] + 1:] if e['k'] == 'move' and X11 and (e['s'] & BUTTON1)]
            return not after, f'{len(mv)} moves through {seen} in order, one press, one release'
        return check
    case('d_four_point_path', NATIVE, 'desktop_drag', {'path': path}, drag_check())
    case('d_path_under_a_held_shift', NATIVE, 'desktop_drag', {'path': path, 'key': 'Shift_L'}, drag_check(extra_mask=SHIFT))
    case('d_two_point_drag_on_the_stock_driver', STOCK, 'desktop_drag', {'path': path[:2]}, lambda g, o: (len([e for e in g if e['k'] == 'down']) == 1 and near([e for e in g if e['k'] == 'up'][0], 500, 100, 1) and len([e for e in g if e['k'] == 'move']) >= 5, f'{pairs(g)} moves {len([e for e in g if e["k"] == "move"])}'))

    # ---- M: move ------------------------------------------------------------------------------
    # A lone move reaches a client on both: on a headless sway the driver's virtual pointer used to be destroyed at the end of the call, the seat
    # lost its pointer capability and no client was told (measured in gaps-linux-2: 0 enter and 0 motion events); the pointer now stays.
    def moved_to(x, y, mask=0):
        return lambda g, o: (not [e for e in g if e['k'] in ('down', 'up')] and bool([e for e in g if e['k'] in ('move', 'enter') and near(e, x, y, 1) and (e['s'] & mask) == mask]), f'{len(g)} events, {[(e["k"], round(e["x"]), round(e["y"]), e["s"]) for e in g if e["k"] in ("move", "enter")][:6]}')
    for n, (x, y) in enumerate([(900, 300), (1000, 310), (1100, 320)]):
        case(f'm_move_{n}_reaches_the_client', NATIVE, 'desktop_move', {'x': x, 'y': y}, moved_to(x, y))
    # under a held Alt: X11 puts it in the motion's state mask; on Wayland a pointer enter carries no modifier state in GTK, so the proof is the
    # client's own key events: Alt goes down before the pointer arrives at the point and comes up after
    def moved_under_alt(g, o):
        alt = [(i, e) for i, e in enumerate(g) if e['k'] in ('kdown', 'kup') and e['sym'] in (0xffe9, 'Alt_L')]
        at = [i for i, e in enumerate(g) if e['k'] in ('move', 'enter') and near(e, 910, 310, 1)]
        masked = all((g[i]['s'] & MOD1) for i in at) if X11 else True
        return len(alt) == 2 and alt[0][1]['k'] == 'kdown' and alt[1][1]['k'] == 'kup' and bool(at) and alt[0][0] < at[0] < alt[1][0] and masked, f'{[(e["k"], e.get("sym")) for e in g]}'
    case('m_move_under_a_held_alt', NATIVE, 'desktop_move', {'x': 910, 'y': 310, 'key': 'Alt_L'}, moved_under_alt)

    # ---- P and T: press_key and type_text, read from the key events the receiver gets ------------------
    RET, CTRL_SYM, SPACE = (0xff0d, 0xffe3, 0x20) if X11 else ('Return', 'Control_L', 'space')
    # (the wlroots virtual keyboard leaves a sacrificial Shift_L tap before the key: a fresh seat drops its first event; not part of the key pressed)
    keyev = lambda g: [(e['k'], e['sym']) for e in g if e['k'] in ('kdown', 'kup') and e['sym'] not in (0xffe1, 'Shift_L')]
    case('p_return', NATIVE, 'desktop_press_key', {'key': 'Return'}, lambda g, o: (keyev(g) == [('kdown', RET), ('kup', RET)], f'{keyev(g)}'))
    def ctrl_a(g, o):
        k = keyev(g); a = [e for e in g if e['k'] == 'kdown' and e.get('ch') == 'a']
        return k[:1] == [('kdown', CTRL_SYM)] and k[-1:] == [('kup', CTRL_SYM)] and len(k) == 4 and bool(a) and bool(a[0]['s'] & CONTROL), f'{k}, a-down state {a[0]["s"] if a else None}'
    case('p_chord_control_a', NATIVE, 'desktop_press_key', {'key': 'Control_L+a'}, ctrl_a)
    def held_key(g, o):
        d = [e for e in g if e['k'] == 'kdown' and e['sym'] not in (0xffe1, 'Shift_L')]; u = [e for e in g if e['k'] == 'kup' and e['sym'] not in (0xffe1, 'Shift_L')]
        held = (u[-1]['t'] - d[0]['t']) if d and u else None   # X11 autorepeats a held key: the first press to the last release
        return held is not None and 550 <= held <= 1500 and all(e['sym'] == SPACE for e in d + u), f'held {held} ms over {len(d)} presses'
    case('p_duration_holds_the_key', NATIVE, 'desktop_press_key', {'key': 'space', 'duration': 600}, held_key)
    nos = lambda g, k: [e for e in g if e['k'] == k and e['sym'] not in (0xffe1, 'Shift_L')]
    case('p_no_duration_is_a_tap', NATIVE, 'desktop_press_key', {'key': 'space'}, lambda g, o: (len(nos(g, 'kdown')) == 1 and len(nos(g, 'kup')) == 1 and nos(g, 'kup')[0]['t'] - nos(g, 'kdown')[0]['t'] <= 250, f'{keyev(g)}'))
    # X11 types a character the layout lacks (ö on a US keymap) through a spare keycode it restores a few ms later, so a client that reads the
    # keymap when it gets to the event can see NoSymbol: an X11 limit of every XTest typist, not of this layer. Wayland builds a keymap for the text.
    TEXT = 'Hello, World 42!' if X11 else 'Hello, wörld 42!'
    case('t_type_text', NATIVE, 'desktop_type_text', {'text': TEXT}, lambda g, o: (''.join(e['ch'] for e in g if e['k'] == 'kdown' and e.get('ch')) == TEXT, f'got {"".join(e["ch"] for e in g if e["k"] == "kdown" and e.get("ch"))!r}'), settle=1.5)
    case('p_stock_plain_key_works', STOCK, 'desktop_press_key', {'key': 'Return'}, lambda g, o: (keyev(g) == [('kdown', RET), ('kup', RET)], f'{keyev(g)}'))
    case('t_stock_type_text_works', STOCK, 'desktop_type_text', {'text': 'abc'}, lambda g, o: (''.join(e['ch'] for e in g if e['k'] == 'kdown' and e.get('ch')) == 'abc', f'{keyev(g)}'), settle=1.2)
    case('p_stock_duration_refused', STOCK, 'desktop_press_key', {'key': 'space', 'duration': 300}, lambda g, o: (not g and isinstance(o, dict) and o.get('needs') == 'press_key.hold_ms', f'{o.get("needs") if isinstance(o, dict) else o} events={len(g)}'), expect_exit=3)

    # ---- W: scroll -----------------------------------------------------------------------------
    def wheel(g):
        out = []
        for e in g:
            if X11 and e['k'] == 'down' and e.get('b') in (4, 5, 6, 7): out.append(({4: 'up', 5: 'down', 6: 'left', 7: 'right'}[e['b']], e['x'], e['y'], e['s']))
            if not X11 and e['k'] == 'scroll':
                d = e['dir'] if e['dir'] != 'smooth' else ('down' if e['dy'] > 0 else 'up' if e['dy'] < 0 else 'right' if e['dx'] > 0 else 'left')
                out.append((d, e['x'], e['y'], e['s']))
        return out
    def wheel_check(direction, x, y, clicks=3, mask=0):
        def check(g, o):
            w = wheel(g)
            ok = bool(w) and all(d == direction and abs(wx - x) <= 1 and abs(wy - y) <= 1 and (s & mask) == mask for d, wx, wy, s in w) and (len(w) == clicks if X11 else len(w) >= 1)
            return ok, f'{len(w)} wheel events {w[:4]}, want {clicks} x {direction} at ({x}, {y}) mask {mask}'
        return check
    for d in ('down', 'up', 'left', 'right'):
        case(f'w_scroll_{d}_from_a_point', NATIVE, 'desktop_scroll', {'direction': d, 'pixels': 120, 'x': 800, 'y': 400}, wheel_check(d, 800, 400))
    case('w_scroll_pixels_are_whole_clicks', NATIVE, 'desktop_scroll', {'direction': 'd', 'pixels': 200, 'x': 800, 'y': 400}, wheel_check('down', 800, 400, clicks=5))
    case('w_scroll_origin_is_where_the_pointer_was_left', NATIVE, 'desktop_click', {'x': 640, 'y': 420}, lambda g, o: (pairs(g) == [('down', 1), ('up', 1)], f'{pairs(g)}'))
    case('w_scroll_default_origin', NATIVE, 'desktop_scroll', {'direction': 'down'}, wheel_check('down', 640, 420))
    case('w_scroll_under_a_held_control', NATIVE, 'desktop_scroll', {'direction': 'down', 'x': 800, 'y': 400, 'key': 'Control_L'}, wheel_check('down', 800, 400, mask=CONTROL))
    case('w_stock_scroll_works', STOCK, 'desktop_scroll', {'direction': 'down', 'x': 820, 'y': 410}, wheel_check('down', 820, 410))
    case('w_stock_held_key_refused', STOCK, 'desktop_scroll', {'direction': 'down', 'key': 'Control_L'}, lambda g, o: (not g and isinstance(o, dict) and o.get('needs') == 'scroll.modifier', f'{o.get("needs") if isinstance(o, dict) else o} events={len(g)}'), expect_exit=3)

    # ---- H: the drag handle (a persistent driver: the press has to outlive the call that made it) -----------
    hsock = start_daemon(NATIVE, 'daemon-handle')
    hs = dict(socket=hsock, state='icu-state-handle')
    HX, HY, HX2, HY2 = 300, 650, 700, 760
    case('h_start_presses_and_holds', NATIVE, 'desktop_drag_handle', {'action': 'start', 'x': HX, 'y': HY}, lambda g, o: (pairs(g) == [('down', 1)] and near(g[0], HX, HY, 1), f'{pairs(g)}'), **hs)
    def shot_while_held(g, o):
        w, h, bpp, rows = shot_pixels(o)
        return (w, h) == (W, H) and not [e for e in g if e['k'] == 'up'] and (o.get('_cursor_in_screenshot') or {}).get('x') == float(HX), f'png {w}x{h}, events during the screenshot {pairs(g)}, cursor {o.get("_cursor_in_screenshot")}'
    case('h_screenshot_between_steps_keeps_the_button_down', NATIVE, 'desktop_screenshot', {'mouse_size_px': 12}, shot_while_held, **hs)
    def handle_move(g, o):
        mv = [e for e in g if e['k'] == 'move']
        held = (not X11) or all(e['s'] & BUTTON1 for e in mv)
        return bool(mv) and near(mv[-1], HX2, HY2, 1) and held and not [e for e in g if e['k'] in ('down', 'up')], f'{len(mv)} moves, last {(mv[-1]["x"], mv[-1]["y"]) if mv else None}, buttons {pairs(g)}'
    case('h_move_to_moves_the_held_button', NATIVE, 'desktop_drag_handle', {'action': 'move_to', 'x': HX2, 'y': HY2}, handle_move, **hs)
    case('h_end_releases_where_it_is', NATIVE, 'desktop_drag_handle', {'action': 'end'}, lambda g, o: (pairs(g) == [('up', 1)] and near(g[0], HX2, HY2, 1), f'{pairs(g)}'), **hs)
    case('h_end_again_is_refused', NATIVE, 'desktop_drag_handle', {'action': 'end'}, lambda g, o: (not g and isinstance(o, dict) and o.get('reason') == 'drag_not_started', f'{o.get("reason") if isinstance(o, dict) else o} events={len(g)}'), expect_exit=2, **hs)
    case('h_a_second_handle_after_the_first_ended', NATIVE, 'desktop_drag_handle', {'action': 'start', 'x': HX, 'y': HY}, lambda g, o: (pairs(g) == [('down', 1)], f'{pairs(g)}'), **hs)
    case('h_start_twice_is_refused', NATIVE, 'desktop_drag_handle', {'action': 'start', 'x': HX2, 'y': HY2}, lambda g, o: (not g and isinstance(o, dict) and o.get('reason') == 'drag_already_started', f'{o.get("reason") if isinstance(o, dict) else o} events={len(g)}'), expect_exit=2, **hs)
    case('h_end_the_second', NATIVE, 'desktop_drag_handle', {'action': 'end'}, lambda g, o: (pairs(g) == [('up', 1)], f'{pairs(g)}'), **hs)
    case('h_off_screen_start_is_refused', NATIVE, 'desktop_drag_handle', {'action': 'start', 'x': W, 'y': 5}, lambda g, o: (not g and isinstance(o, dict) and o.get('reason') == 'point_off_screen', f'{g}'), expect_exit=2, **hs)
    case('h_stock_driver_has_no_handle', STOCK, 'desktop_drag_handle', {'action': 'start', 'x': HX, 'y': HY}, lambda g, o: (not g and isinstance(o, dict) and o.get('needs') == 'drag.hold', f'{o.get("needs") if isinstance(o, dict) else o} events={len(g)}'), expect_exit=3, state='icu-state-stock-handle')
    # a driver that exits after each call (no daemon): on X11 the press lives in the X server and the next process finds the hold (a file beside the
    # display) and ends it; on native Wayland the press is the process's virtual pointer and goes with it, so the layer refuses to press at all
    # (`driver_session`): the client would otherwise see a press with no release (measured before the guard: a down, then nothing)
    os1 = dict(state='icu-state-oneshot')
    if X11:
        case('h_oneshot_start', NATIVE, 'desktop_drag_handle', {'action': 'start', 'x': HX, 'y': HY}, lambda g, o: (pairs(g)[:1] == [('down', 1)], f'{pairs(g)} (the press outlives the process)'), **os1)
        case('h_oneshot_end', NATIVE, 'desktop_drag_handle', {'action': 'end'}, lambda g, o: (pairs(g) == [('up', 1)] and near(g[0], HX, HY, 1), f'{pairs(g)} (released by the next process)'), **os1)
    else:
        case('h_oneshot_start_is_refused_not_lost', NATIVE, 'desktop_drag_handle', {'action': 'start', 'x': HX, 'y': HY},
             lambda g, o: (not g and isinstance(o, dict) and o.get('needs') == 'driver_session', f'needs={o.get("needs") if isinstance(o, dict) else o} events={len(g)} (no press reached the client)'), expect_exit=3, **os1)
        case('h_oneshot_end_has_nothing_to_end', NATIVE, 'desktop_drag_handle', {'action': 'end'},
             lambda g, o: (not g and isinstance(o, dict) and o.get('reason') == 'drag_not_started', f'{o.get("reason") if isinstance(o, dict) else o} events={len(g)}'), expect_exit=2, **os1)
    if not X11:
        # whatever the compositor thinks of that lost press, the next click must still reach the client as a down and an up
        case('h_oneshot_next_click_still_clicks', NATIVE, 'desktop_click', {'x': HX, 'y': HY}, lambda g, o: (pairs(g)[-2:] == [('down', 1), ('up', 1)], f'{pairs(g)}'), **os1)
    # a button nobody comes back for is let go by the driver (CUA_DRIVER_RS_DRAG_HOLD_IDLE_MS, short here)
    isock = start_daemon(NATIVE, 'daemon-idle', extra={'CUA_DRIVER_RS_DRAG_HOLD_IDLE_MS': '1500'})
    ihs = dict(socket=isock, state='icu-state-idle')
    case('h_idle_start', NATIVE, 'desktop_drag_handle', {'action': 'start', 'x': HX, 'y': HY}, lambda g, o: (pairs(g)[:1] == [('down', 1)], f'{pairs(g)} (the release, if it came, came with no end call)'), settle=4.5, **ihs)
    up = [e for e in events() if e['k'] == 'up']
    report['cases']['h_idle_button_is_released_by_the_driver'] = {'ok': bool(up) and events()[-1]['k'] == 'up', 'detail': f'last events {pairs(events()[-3:])} (the release came with no end call)'}
    print('PASS' if report['cases']['h_idle_button_is_released_by_the_driver']['ok'] else 'FAIL', 'h_idle_button_is_released_by_the_driver', report['cases']['h_idle_button_is_released_by_the_driver']['detail'], flush=True)
    case('h_idle_end_after_the_release_clears_the_handle', NATIVE, 'desktop_drag_handle', {'action': 'end'}, lambda g, o: (not [e for e in g if e['k'] in ('down', 'up')], f'{pairs(g)}'), **ihs)

    # ---- the entry a model client uses: `icu mcp`, one session, one driver child that lives across the calls --------------------------------------
    # PROBE_MCP_RESTART=1 is the control: the session is restarted before every call (the lifetime of `icu call`), and these cases must go red on Wayland.
    ms = McpSession(NATIVE, 'icu-state-mcp', restart_each=bool(os.environ.get('PROBE_MCP_RESTART')))
    def mcase(name, tool, args, check, settle=.8):
        if ONLY and not name.startswith(ONLY): return {}
        before = len(events())
        try: out = ms.call(tool, args)
        except Exception as e: out = {'_error': repr(e)}
        time.sleep(settle)
        got = events()[before:]
        try: ok, detail = check(got, out)
        except Exception as e: ok, detail = False, f'check raised {e!r}'
        report['cases'][name] = {'tool': tool, 'args': args, 'reply': {k: v for k, v in out.items() if k != 'screenshots'} if isinstance(out, dict) else out,
                                 'events': got[:40], 'n_events': len(got), 'ok': bool(ok), 'detail': detail}
        print('PASS' if ok else 'FAIL', name, detail, flush=True)
        return out
    mcase('m_session_start_presses_and_holds', 'desktop_drag_handle', {'action': 'start', 'x': HX, 'y': HY}, lambda g, o: (pairs(g) == [('down', 1)] and near(g[0], HX, HY, 1), f'{pairs(g)}'))
    mcase('m_session_screenshot_keeps_the_button_down', 'desktop_screenshot', {'mouse_size_px': 12}, lambda g, o: (not [e for e in g if e['k'] == 'up'] and 'screenshots' in o, f'events during the screenshot {pairs(g)}'))
    mcase('m_session_move_to_moves_the_held_button', 'desktop_drag_handle', {'action': 'move_to', 'x': HX2, 'y': HY2}, handle_move)
    mcase('m_session_end_releases_where_it_is', 'desktop_drag_handle', {'action': 'end'}, lambda g, o: (pairs(g) == [('up', 1)] and near(g[0], HX2, HY2, 1), f'{pairs(g)}'))
    mcase('m_session_end_again_is_refused', 'desktop_drag_handle', {'action': 'end'}, lambda g, o: (not g and o.get('reason') == 'drag_not_started', f'{o.get("reason")} events={len(g)}'))
    mcase('m_session_next_click_is_one_press_and_release', 'desktop_click', {'x': HX, 'y': HY}, lambda g, o: (pairs(g) == [('down', 1), ('up', 1)], f'{pairs(g)}'))
    ms.close()

    for d in ('daemon-handle', 'daemon-idle'):  # the persistent drivers go: a daemon keeps a virtual pointer, which would change the lone-move cases below
        for p in list(procs):
            if p.args[:2] == [str(NATIVE), 'serve'] and d in ' '.join(p.args):
                try: os.killpg(p.pid, signal.SIGTERM)
                except ProcessLookupError: pass
    time.sleep(1)

    # ---- O: off the screen -------------------------------------------------------------------
    for name, tool, args in [('o_click_x_eq_width', 'desktop_click', {'x': W, 'y': 10}), ('o_click_y_eq_height', 'desktop_click', {'x': 10, 'y': H}), ('o_click_negative', 'desktop_click', {'x': -1, 'y': 10}),
                             ('o_move_far', 'desktop_move', {'x': 99999, 'y': 5}), ('o_drag_last_point_off', 'desktop_drag', {'path': [{'x': 5, 'y': 5}, {'x': 50, 'y': 50}, {'x': W + 5, 'y': 9}]})]:
        case(name, NATIVE, tool, args, lambda g, o: (not g and isinstance(o, dict) and o.get('reason') == 'point_off_screen', f'reason={o.get("reason") if isinstance(o, dict) else o} events={len(g)}'), expect_exit=2)

    # ---- K: the stock driver ------------------------------------------------------------------
    stock_refusal = lambda needs: (lambda g, o: (not g and isinstance(o, dict) and o.get('needs') == needs, f'needs={o.get("needs") if isinstance(o, dict) else o} events={len(g)}'))
    case('k_stock_duration_refused', STOCK, 'desktop_click', {'x': 340, 'y': 240, 'duration': 300}, stock_refusal('click.hold_ms'), expect_exit=3)
    case('k_stock_three_point_path_refused', STOCK, 'desktop_drag', {'path': path[:3]}, stock_refusal('drag.path'), expect_exit=3)
    case('k_stock_held_key_move_refused', STOCK, 'desktop_move', {'x': 920, 'y': 320, 'key': 'Alt_L'}, stock_refusal('move_cursor.modifier'), expect_exit=3)
    case('k_stock_plain_click_works', STOCK, 'desktop_click', {'x': 350, 'y': 250}, lambda g, o: (pairs(g) == [('down', 1), ('up', 1)] and near(g[0], 350, 250, 1), f'{pairs(g)}'))
    case('k_stock_plain_move_works', STOCK, 'desktop_move', {'x': 930, 'y': 330}, (lambda g, o: (bool([e for e in g if e['k'] == 'move' and near(e, 930, 330, 1)]), f'{len(g)} events')) if X11 else (lambda g, o: (not g and o.get('_desktop_pointer') == {'x': 930.0, 'y': 330.0}, f'{len(g)} events (headless sway delivers none), pointer {o.get("_desktop_pointer")}')))
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
