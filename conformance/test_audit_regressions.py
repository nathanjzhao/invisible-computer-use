"""Required offline guards through current workspace executables, never desktop input."""
import json, os, signal, subprocess, time
from pathlib import Path
import pytest

ROOT = Path(__file__).resolve().parents[1]
ICU = Path(os.environ.get('ICU_BIN', ROOT / 'impl/target/debug/icu'))
AXDIFF = Path(os.environ.get('AXDIFF_CUA_BIN', ROOT / 'impl/target/debug/axdiff-cua'))


def run_driver(tmp_path, mode, args, stdin=None, driver_timeout_ms=200):
    assert ICU.is_file(), f'required binary absent: {ICU}; build impl workspace'
    env = {k: v for k, v in os.environ.items() if not k.startswith(('ICU_', 'AUDIT_DRIVER_'))}
    pidfile = tmp_path / 'pid'
    # The fixture speaks MCP on stdio only; it has no `serve` daemon mode to start.
    env.update(ICU_DRIVER=str(ROOT / 'conformance/fixtures/audit_driver.py'), ICU_DRIVER_DAEMON='0', ICU_HOME=str(tmp_path),
               ICU_DRIVER_TIMEOUT_MS=str(driver_timeout_ms), AUDIT_DRIVER_MODE=mode, AUDIT_DRIVER_PID=str(pidfile))
    p = subprocess.Popen([str(ICU), *args], stdin=subprocess.PIPE, stdout=subprocess.PIPE,
                         stderr=subprocess.PIPE, text=True, env=env, start_new_session=True)
    start = time.monotonic()
    expired = False
    try: out, err = p.communicate(stdin, timeout=2)
    except subprocess.TimeoutExpired:
        expired = True
        os.killpg(p.pid, signal.SIGKILL)
        out, err = p.communicate()
    elapsed = time.monotonic() - start
    record = dict(mode=mode, args=args, exit=p.returncode, expired=expired, elapsed=elapsed, stdout=out, stderr=err)
    evidence = os.environ.get('AUDIT_FIX_EVIDENCE')
    if evidence:
        path = Path(evidence); path.mkdir(parents=True, exist_ok=True)
        (path / f'{mode}-{args[0]}.json').write_text(json.dumps(record, indent=2))
    assert not expired, record
    assert elapsed < 1.5, record
    if pidfile.exists():
        pid = int(pidfile.read_text())
        # icu sends SIGKILL and waits at most 250 ms; on a loaded host the killed
        # child can outlive that wait briefly before launchd reaps it. A child icu
        # never killed (the silent fixtures sleep 60 s) still fails this bound.
        gone = time.monotonic() + 1.0
        while time.monotonic() < gone:
            try: os.kill(pid, 0)
            except ProcessLookupError: break
            time.sleep(0.02)
        with pytest.raises(ProcessLookupError): os.kill(pid, 0)
    return p.returncode, out, err


@pytest.mark.parametrize('mode', ['silent_init', 'silent_health', 'garbage', 'notifications', 'wrong_id', 'partial', 'eof'])
def test_capabilities_transport_deadline(tmp_path, mode):
    code, out, _ = run_driver(tmp_path, mode, ['capabilities'])
    assert code == 0
    r = json.loads(out)
    assert r['features']['observe']['available'] != 'available'
    assert 'SkyComputerUseTransportError' in r['features']['observe']['blocked_by']


def test_initialization_failure_is_typed_on_cli(tmp_path):
    code, out, _ = run_driver(tmp_path, 'silent_init', ['call', 'list_apps'])
    assert code == 3
    assert json.loads(out)['code'] == 'SkyComputerUseTransportError'


@pytest.mark.parametrize('mode', ['silent_init', 'silent_health'])
def test_failed_request_keeps_mcp_capabilities_responsive(tmp_path, mode):
    requests = [dict(jsonrpc='2.0', id=1, method='initialize', params={'protocolVersion': '2024-11-05'}),
                dict(jsonrpc='2.0', id=2, method='tools/call', params={'name': 'host_state', 'arguments': {'padding': 'x' * 500000}}),
                dict(jsonrpc='2.0', id=3, method='tools/call', params={'name': 'capabilities', 'arguments': {}})]
    code, out, _ = run_driver(tmp_path, mode, ['mcp'], '\n'.join(map(json.dumps, requests)) + '\n')
    assert code == 0
    replies = {r['id']: r for r in map(json.loads, out.splitlines()) if 'id' in r}
    assert replies[2]['result']['isError'] is True
    assert replies[2]['result']['structuredContent']['code'] == 'SkyComputerUseTransportError'
    assert replies[3]['result']['structuredContent']['features']['observe']['available'] != 'available'


@pytest.mark.parametrize('mode,expected', [('healthy', 'available'), ('missing_click', 'unavailable'),
    ('old_version', 'unavailable'), ('prerelease_version', 'unavailable'), ('unknown_version', 'unknown'),
    ('malformed_version', 'unknown'), ('unknown_tools', 'unknown'), ('malformed_tools', 'unknown')])
def test_support_agrees_with_dispatch(tmp_path, mode, expected):
    # Responsive fixtures get a wider transport budget than the 200 ms deadline
    # guards: a loaded host can take longer than 200 ms just to start Python, and
    # that transport timeout is not the support/dispatch disagreement tested here.
    code, out, _ = run_driver(tmp_path, mode, ['capabilities'], driver_timeout_ms=1000)
    r = json.loads(out)
    assert code == 0
    for feature in ('act', 'observe'): assert r['features'][feature]['available'] == expected, r
    code, out, _ = run_driver(tmp_path, mode, ['call', 'list_apps'], driver_timeout_ms=1000)
    r = json.loads(out)
    if expected == 'unavailable':
        assert code == 3 and r['code'] == 'incompatibleClientVersion', r
    else: assert code == 0, r  # Unknown support is not a fabricated incompatibility.


def diff(tmp_path, before, after, cumulative=False):
    assert AXDIFF.is_file(), f'required binary absent: {AXDIFF}; build impl workspace'
    for name, elements in [('before', before), ('after', after)]:
        (tmp_path / f'{name}.json').write_text(json.dumps({'elements': elements}))
    cmd = [str(AXDIFF), str(tmp_path / 'before.json'), str(tmp_path / 'after.json')]
    if cumulative: cmd += ['--cumulative']
    p = subprocess.run(cmd, capture_output=True, text=True, timeout=2)
    assert p.returncode == 0, p.stderr
    return p.stdout


def rows(s):
    """The diff's row lines (after the one-line header), without the Removed line."""
    return [l for l in s.splitlines()[1:] if not l.startswith('Removed element IDs: ')]


def button(i, label, **kw):
    return dict(element_index=i, role='AXButton', label=label, **kw)


def test_cross_position_replacement_does_not_reuse_removed_id(tmp_path):
    s = diff(tmp_path, [button(0, 'A'), button(1, 'B')], [button(0, 'B'), button(1, 'C')])
    assert 'Removed element IDs: 0\n' in s and '+2 button C' in rows(s) and not any(l.startswith('~') for l in rows(s)), s


def test_snapshot_token_and_position_do_not_prove_identity(tmp_path):
    s = diff(tmp_path, [button(0, 'A', element_token='s1:0')], [button(0, 'C', element_token='s2:0')])
    assert 'Removed element IDs: 0\n' in s and '+1 button C' in rows(s), s


def test_nested_unlabelled_subtree_swap(tmp_path):
    def tree(labels):
        return [row for i, label in enumerate(labels) for row in (
            dict(element_index=2*i, role='AXGroup', label=''), button(2*i+1, label, parent_index=2*i))]
    s = diff(tmp_path, tree(['A', 'B']), tree(['B', 'C']))
    assert 'Removed element IDs: 0-1\n' in s and rows(s) == ['+4 container', '+\t5 button C'], s


@pytest.mark.parametrize('cumulative', [False, True])
def test_supported_relabel_across_snapshot_tokens(tmp_path, cumulative):
    s = diff(tmp_path, [button(0, 'Play', identifier='transport-play', element_token='s1:0')],
             [button(0, 'Pause', identifier='transport-play', element_token='s2:0')], cumulative)
    assert rows(s) == ['~0 button Pause, ID: transport-play'] and 'Removed element IDs' not in s, s


def test_conflicting_platform_identifiers_are_replacements(tmp_path):
    s = diff(tmp_path, [button(0, 'Save', identifier='old')], [button(0, 'Save', identifier='new')])
    assert 'Removed element IDs: 0\n' in s and rows(s) == ['+1 button Save, ID: new'], s


def test_every_native_action_uses_the_shared_cursor_policy():
    base = ROOT / 'vendor-cua/libs/cua-driver/rust/crates/platform-macos/src'
    assert (base/'tools/click.rs').is_file(), f'required native source absent: {base}'
    routes = [*sorted((base / 'tools').glob('*.rs')), base / 'browser/platform.rs']
    offenders = [str(p.relative_to(base)) for p in routes if 'overlay::animate_cursor_to(' in p.read_text()]
    assert not offenders, f'action routes bypass detached cursor policy: {offenders}'


def png_b64(width, height):
    import base64, struct, zlib
    def chunk(kind, data):
        return struct.pack('>I', len(data)) + kind + data + struct.pack('>I', zlib.crc32(kind + data))
    png = b'\x89PNG\r\n\x1a\n' + chunk(b'IHDR', struct.pack('>IIBBBBB', width, height, 8, 2, 0, 0, 0))
    png += chunk(b'IDAT', zlib.compress((b'\0' + b'\x12\x34\x56' * width) * height)) + chunk(b'IEND', b'')
    return base64.b64encode(png).decode()


def map_shots(tmp_path, shots):
    doc = tmp_path / 'shots.json'
    doc.write_text(json.dumps({'window_id': 1, 'window_title': 'Fixture', 'screenshots': shots}))
    return subprocess.run([str(ICU), 'window2', 'map', '--gws', str(doc), '--app', 'audit.fixture',
                           '--state', str(tmp_path/'state')], capture_output=True, text=True, timeout=2)


def test_legacy_pixel_dimensions_normalize_with_surface_scale(tmp_path):
    sc = dict(id='menu', zIndex=1, originX=-20, originY=12, width=40, height=60,
              scale=2, png_b64=png_b64(40, 60))
    p = map_shots(tmp_path, [sc])
    assert p.returncode == 0, p.stderr
    shot = json.loads(p.stdout)['screenshots'][0]
    assert (shot['originX'], shot['originY'], shot['width'], shot['height']) == (-20, 12, 20, 30)


@pytest.mark.parametrize('damage', ['png', 'pixel_width', 'aspect', 'scale'])
def test_bad_screenshot_geometry_is_rejected(tmp_path, damage):
    sc = dict(id='menu', zIndex=1, originX=-20, originY=12, width=20, height=30,
              coordinate_space='logical', pixel_width=40, pixel_height=60,
              scale_x=2, scale_y=2, png_b64=png_b64(40, 60))
    if damage == 'png': sc['png_b64'] = 'AAAA'
    elif damage == 'pixel_width': sc['pixel_width'] = 50
    elif damage == 'aspect': sc['height'] = 15
    else: sc['scale_x'] = 3
    p = map_shots(tmp_path, [sc])
    assert p.returncode != 0, p.stdout
    assert 'screenshot' in p.stderr.lower()


def test_logical_downscaled_screenshot_keeps_region(tmp_path):
    sc = dict(id='window', zIndex=0, originX=-100, originY=30, width=400, height=300,
              coordinate_space='logical', pixel_width=200, pixel_height=150,
              scale_x=.5, scale_y=.5, png_b64=png_b64(200, 150))
    p = map_shots(tmp_path, [sc])
    assert p.returncode == 0, p.stderr
    s = json.loads(p.stdout)['screenshots'][0]
    assert (s['width'], s['height'], s['originX'], s['originY']) == (400, 300, -100, 30)


@pytest.mark.parametrize('tool', ['right_click','double_click'])
def test_indexed_click_branches_schedule_feedback_before_action(tool):
    path = ROOT/'vendor-cua/libs/cua-driver/rust/crates/platform-macos/src/tools'/f'{tool}.rs'
    source = path.read_text()
    branch = source.split('// ── AX element path',1)[1].split('// ── Pixel path',1)[0]
    assert 'run_action_after_feedback(' in branch and 'animate_cursor_at_element(' in branch, branch


def test_portrait_downscale_rounding_is_valid(tmp_path):
    sc = dict(id='portrait',zIndex=0,originX=5,originY=-20,width=201,height=1000,
              coordinate_space='logical',pixel_width=145,pixel_height=720,
              scale_x=145/201,scale_y=.72,png_b64=png_b64(145,720))
    p = map_shots(tmp_path,[sc]); assert p.returncode==0,p.stderr
    s=json.loads(p.stdout)['screenshots'][0]
    assert (s['width'],s['height'])==(201,1000)
