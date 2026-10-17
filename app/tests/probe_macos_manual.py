"""Installed Copilot.app manual tile input on a disposable macOS session.

The probe is the user: it clicks, wheels and types into Copilot's live tile
with real HID events while Copilot stays frontmost. The app forwards each
gesture to a background AppKit receiver through its owned driver. Samples
check that Copilot keeps the foreground, its own keyboard focus and the
user's pointer, and that the receiver is never activated.

  probe_macos_manual.py <work> <Copilot.app> <minicalc_mac> <macos_ui>
"""
import base64, hashlib, json, os, pathlib, signal, struct, subprocess, sys, time, traceback
sys.path.insert(0, str(pathlib.Path(__file__).resolve().parent))
from macos_probe import Session, wait_for, center, HARNESS_CAPTURE_PROMPT

if sys.platform != 'darwin': raise SystemExit('macOS-only probe')
work = pathlib.Path(sys.argv[1])
report = {'source_commit': os.environ.get('ICU_TEST_APP_REVISION', 'unknown'),
          'scope': 'installed Copilot.app (LaunchServices), owned AppKit receiver in the background, HID input into the live tile',
          'probe_sha256': hashlib.sha256(pathlib.Path(__file__).read_bytes()).hexdigest()}
s = Session(work, *sys.argv[2:5], report)
sampler = None
ABC = 'com.apple.keylayout.ABC'
HIRAGANA = 'com.apple.inputmethod.Kotoeri.RomajiTyping.Japanese'


def receiver_state():
    return json.loads((work / 'receiver-state.json').read_text())


def jsonl(name):
    path = work / name
    return [json.loads(l) for l in path.read_text().splitlines() if l.strip()] if path.exists() else []


def tile_image(aspect):
    """The tile's frame image rectangle, or None until a frame has loaded.

    alt="" keeps the <img> out of the AX tree. The control hint is pinned to
    the bottom of .shot-wrap, whose width is the image width (100%) and whose
    height is the image height (auto) once a frame is decoded.
    """
    nodes = s.tree()
    hint = next((n for n in nodes if n.get('role') == 'AXGroup' and n.get('name', '').startswith('controlling · click') and n.get('frame', {}).get('w', 0) > 100), None)
    control = next((n for n in nodes if n.get('role') == 'AXButton' and n.get('name', '').casefold() == 'control'), None)
    if not hint or not control: return None
    f = hint['frame']; bottom = f['y'] + f['h']; h = f['w'] * aspect
    if not (h - 2 <= bottom - (control['frame']['y'] + control['frame']['h']) <= h + 24): return None
    return {'frame': {'x': f['x'], 'y': bottom - h, 'w': f['w'], 'h': h}}


def to_tile(img, window, x, y):
    """Screen point on the real window -> the same point on the tile image."""
    f = img['frame']
    return f['x'] + (x - window['x']) / window['w'] * f['w'], f['y'] + (y - window['y']) / window['h'] * f['h']


def start_sampling(tag):
    global sampler
    baseline = s.ui('state', s.driver_pid)
    path = work / f'ownership-{tag}.jsonl'
    sampler = subprocess.Popen([str(s.ui_bin), 'sample', str(path), str(s.driver_pid)], stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    time.sleep(.2)
    return baseline, path


def stop_sampling(tag, baseline, path, keys):
    sampler.send_signal(signal.SIGTERM); sampler.wait(5)
    samples = [json.loads(l) for l in path.read_text().splitlines() if l.strip()]
    bad = [x for x in samples if any(x.get(k) != baseline.get(k) for k in keys) or x.get('owned_onscreen_windows')]
    report.setdefault('phases', {})[tag] = {'samples': len(samples), 'violations': bad[:5], 'keys': keys}
    assert len(samples) >= 10, f'{tag}: ownership observation too short'
    assert not bad, f'{tag}: forwarding changed ownership'


def deadline(_s, _f): raise TimeoutError('overall macOS manual probe deadline')
signal.signal(signal.SIGALRM, deadline); signal.signal(signal.SIGTERM, deadline); signal.alarm(240)
try:
    report['trust'] = s.ui('trusted')
    report['input_source_before'] = s.ui('input-source', 'current')['id']
    receiver, ready = s.fixture_proc('receiver'); report['receiver_pid'] = receiver.pid
    window = ready['windows'][0]['frame']; pad = ready['pad']; scroller = ready['scroll']
    report.update(window_frame=window, pad=pad, scroll=scroller)
    assert receiver_state()['text'] == 'seed' and receiver_state()['draft_first_responder'], 'receiver not initialised'
    # Direction reference: the same physical wheel over the real window.
    y0 = receiver_state()['scroll_y']
    s.ui('scroll', scroller['x'] + scroller['w'] / 2, scroller['y'] + scroller['h'] / 2, -3)
    wait_for(lambda: receiver_state()['scroll_y'] != y0, 5, 'direct wheel did not scroll the receiver')
    time.sleep(.4); direct_delta = receiver_state()['scroll_y'] - y0
    report['direct_wheel_delta'] = direct_delta
    s.start_app()
    s.pick_window('ICU Manual Receiver')
    # The tile shows the driver's capture; measure that image, as the app does.
    receiver_window = next(w for w in s.windows() if w.get('pid') == receiver.pid and w.get('title') == 'ICU Manual Receiver')
    shot = s.call('get_window_state', {'pid': receiver.pid, 'window_id': receiver_window['window_id'], 'include_screenshot': True, 'include_accessibility_tree': False, 'max_dimension': 640})
    png = base64.b64decode(next(c['data'] for c in shot['content'] if c.get('type') == 'image'))
    capture = struct.unpack('>II', png[16:24]); report['capture_px'] = capture; report['driver_window_bounds'] = receiver_window['bounds']
    aspect = capture[1] / capture[0]
    img = wait_for(lambda: tile_image(aspect), 12, 'tile frame never loaded with the capture aspect')
    s.ui('click', 960, 110)  # the user brings Copilot forward by its title bar
    wait_for(lambda: s.ui('state')['front_pid'] == s.app_pid, 5, 'Copilot did not come forward')
    # Locate the rendered image top from screen pixels: the dark tile chrome
    # ends where the receiver's light title bar begins.
    f = img['frame']; col_x = int(f['x'] + f['w'] / 2); y0 = int(f['y'] - 40)
    strip = work / 'tile-column.png'
    subprocess.run(['/usr/sbin/screencapture', '-x', '-R', f'{col_x},{y0},1,80', str(strip)], check=True, timeout=20)
    # On a fresh image this first harness `screencapture` raises macOS 15's
    # capture prompt for the ssh session's process, and the alert sits over the
    # tile's pad. Answer it as setup, before the click it would swallow.
    report['harness_capture_prompt'] = s.approve_capture_prompt(prompt=HARNESS_CAPTURE_PROMPT)
    column = s.ui('pngcolumn', strip)['rgb']
    assert len(column) == 80, 'screen strip is not 1x'
    top = next(y0 + i for i, (r, g, b) in enumerate(column) if min(r, g, b) > 150)
    report['tile_image_layout_estimate'] = dict(f)
    img = {'frame': {'x': f['x'], 'y': top, 'w': f['w'], 'h': f['h']}}
    report['tile_image'] = img['frame']
    wait_for(lambda: receiver_state()['active'] is False or s.ui('state')['front_pid'] == s.app_pid, 3, 'receiver still active')

    report['stage'] = 'pointer'
    target = (pad['x'] + pad['w'] * .3, pad['y'] + pad['h'] * .6)  # an off-centre point on the pad
    tx, ty = to_tile(img, window, *target)
    # Alerts and panels sit at layers 1-19; the Dock's full-screen layer-20 window and the
    # menu bar are always there and take no tile clicks.
    covering = [w for w in s.ui('onscreen')['windows'] if 0 < w['layer'] < 20 and w['owner'] not in (s.app_pid, s.driver_pid)
                and w['bounds'].get('X', 0) <= tx <= w['bounds'].get('X', 0) + w['bounds'].get('Width', 0)
                and w['bounds'].get('Y', 0) <= ty <= w['bounds'].get('Y', 0) + w['bounds'].get('Height', 0)]
    assert not covering, f'a window above Copilot covers the tile click point: {covering}'
    s.ui('move', tx, ty); time.sleep(.2)
    baseline, path = start_sampling('pointer')
    s.ui('click', tx, ty)
    down = wait_for(lambda: next((e for e in jsonl('pointer-events.jsonl') if e['kind'] == 'down'), None), 8, 'tile click did not reach the receiver')
    time.sleep(.8)
    stop_sampling('pointer', baseline, path, ['front_pid', 'menubar_pid', 'pointer'])
    expected = (target[0] - pad['x'], target[1] - pad['y'])
    report['pointer'] = {'tile_point': [tx, ty], 'expected_local': expected, 'received': down,
                         'error_px': [down['x'] - expected[0], down['y'] - expected[1]]}
    scale = window['w'] / img['frame']['w']  # one tile pixel in window points
    assert abs(down['x'] - expected[0]) <= 2 * scale + 1 and abs(down['y'] - expected[1]) <= 2 * scale + 1, 'tile click landed at the wrong window point'
    assert down['button'] == 0 and down['clicks'] == 1, 'tile click changed shape'
    report['receiver_active_at_click'] = down['active']
    wait_for(lambda: s.find(lambda n: n.get('role') == 'AXTextArea' and 'Type into' in n.get('name', '')), 5, 'control editor missing after tile click')

    report['stage'] = 'wheel'
    baseline, path = start_sampling('wheel')
    y1 = receiver_state()['scroll_y']
    sx, sy = to_tile(img, window, scroller['x'] + scroller['w'] / 2, scroller['y'] + scroller['h'] / 2)
    s.ui('scroll', sx, sy, -3)
    wait_for(lambda: receiver_state()['scroll_y'] != y1, 8, 'tile wheel did not scroll the receiver')
    time.sleep(.6)
    tile_delta = receiver_state()['scroll_y'] - y1
    report['wheel'] = {'direct_delta': direct_delta, 'tile_delta': tile_delta, 'events': [e for e in jsonl('scroll-events.jsonl') if e['time'] > baseline['time']][:5]}
    assert (tile_delta > 0) == (direct_delta > 0), 'tile wheel scrolled opposite to the same wheel on the window'
    report['scroll_events_while_appkit_active'] = sum(bool(e['active']) for e in jsonl('scroll-events.jsonl') if e['time'] > baseline['time'])
    # The wheel moves the user's pointer onto the tile; compare from there.
    baseline['pointer'] = s.ui('state')['pointer']
    stop_sampling('wheel', baseline, path, ['front_pid', 'menubar_pid'])

    report['stage'] = 'keyboard'
    editor = s.find(lambda n: n.get('role') == 'AXTextArea' and 'Type into' in n.get('name', ''))
    assert editor and editor.get('focused'), 'control editor does not own keyboard focus'
    baseline, path = start_sampling('keyboard'); report['keyboard_started'] = baseline['time']
    s.ui('type', 'A')
    wait_for(lambda: receiver_state()['text'] == 'seedA', 8, 'ASCII input did not reach receiver')
    s.ui('key', 36)  # Return goes through the key path
    wait_for(lambda: receiver_state()['text'] == 'seedA\n', 8, 'Return did not reach receiver')
    report['ascii'] = receiver_state()

    report['stage'] = 'ime'
    report['ime_method'] = s.ui('input-source', 'com.apple.inputmethod.Kotoeri.RomajiTyping', check=False)
    report['ime_select'] = s.ui('input-source', HIRAGANA, check=False)
    assert report['ime_select'].get('ok'), 'Japanese input source could not be selected'
    ime = wait_for(lambda: s.ui('input-source', 'current')['id'] == HIRAGANA, 3, 'Japanese input source not selected')
    if ime:
        s.ui('key', 0)  # 'a' -> marked あ in Copilot's editor
        time.sleep(.8)
        report['before_commit'] = receiver_state()
        assert report['before_commit']['text'] == 'seedA\n', 'provisional composition escaped to the receiver'
        s.ui('key', 36)  # Return commits the composition
        wait_for(lambda: receiver_state()['text'] == 'seedA\nあ', 8, 'committed IME text did not reach receiver')
        report['ime_commit'] = receiver_state()
        s.ui('input-source', report['input_source_before'])
        wait_for(lambda: s.ui('input-source', 'current')['id'] == report['input_source_before'], 3, 'keyboard layout not restored')
    expected_text = 'seedA\n' + ('あ' if ime else '')

    report['stage'] = 'paste'
    pasted = '\nsecond\t🙂  '
    s.ui('paste-set', pasted)
    s.ui('key', 9, 'cmd')
    expected_text += pasted
    wait_for(lambda: receiver_state()['text'] == expected_text, 8, 'paste did not reach receiver exactly')
    report['paste'] = receiver_state(); report['expected_text'] = expected_text
    time.sleep(.5)
    stop_sampling('keyboard', baseline, path, ['front_pid', 'menubar_pid', 'focused_window', 'focused_element', 'pointer'])
    events = jsonl('receiver-events.jsonl')
    report['receiver_event_count'] = len(events)
    assert events, 'receiver recorded no insertion events'
    # WindowServer ownership is asserted above. AppKit activation of the
    # receiver (focus without raise) is reported, not hidden.
    report['receiver_events_while_appkit_active'] = sum(bool(e['active']) for e in events if e['time'] >= report['keyboard_started'])
    report['activation_timeline'] = jsonl('activation-events.jsonl')
    assert all(e['private_unchanged'] for e in events), 'manual text reached the private field'
    assert not any(e['text'].startswith('seedA\na') for e in events), 'IME romaji leaked into receiver'
    s.screenshot('completed.png')
    report['ime_committed_text'] = bool(ime)
    report['ok'] = True
except Exception as error:
    report.update(ok=False, error=str(error), error_type=type(error).__name__, traceback=traceback.format_exc())
    try: report['receiver_at_failure'] = receiver_state()
    except Exception: pass
    try: report['ui_nodes'] = [{k: n.get(k) for k in ('role', 'name', 'value', 'focused')} for n in s.tree()][:300]
    except Exception: pass
    s.screenshot('failure.png')
finally:
    signal.alarm(0)
    if sampler and sampler.poll() is None: sampler.send_signal(signal.SIGTERM)
    try: s.ui('input-source', report.get('input_source_before', ABC), check=False)
    except Exception: pass
    s.close()
    tap_log = pathlib.Path(str(s.sock) + '.requests.jsonl') if s.sock else None
    if tap_log and tap_log.exists():
        (work / 'driver-requests.jsonl').write_bytes(tap_log.read_bytes()); tap_log.unlink()
    (work / 'result.json').write_text(json.dumps(report, indent=2, ensure_ascii=False) + '\n')
    print(json.dumps({k: v for k, v in report.items() if k not in ('ui_nodes', 'traceback')}, indent=1, ensure_ascii=False)[:5000])
    if report.get('error'): print(report.get('traceback', ''))
    sys.exit(0 if report.get('ok') else 1)
