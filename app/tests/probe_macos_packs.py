"""Instruction-pack delivery in the shipped paths, on a disposable Mac.

Layer (impl/install.sh output: `icu call` and `icu mcp`) against real Clock,
Music and TextEdit windows plus a Numbers stand-in (the owned AppKit fixture
inside a bundle whose identifier is com.apple.iWork.Numbers; Numbers itself is
not installed on the image). Then Copilot.app's own chat loop against the
stand-in, reading the exact model requests from a localhost stub.

Claims checked, each narrowly:
- verbatim: the text between the tags is byte-identical to the repository pack,
  whose sha256 matches the recorded shipped-bundle hash;
- first touch: a session's first observation of a pack app carries the pack,
  its second does not; a new session delivers again;
- scope: an app without a pack gets no prefix and does not consume one;
- override: a personal pack replaces (default) or precedes (ICU_PERSONAL_OVERRIDES=0)
  the built-in pack;
- MCP: one `icu mcp` process delivers once across two calls;
- Copilot: a task's first model request carries the stand-in's pack exactly
  once, its later requests do not, and the next task carries it again.

  probe_macos_packs.py <work> <Copilot.app> <minicalc_mac> <macos_ui>   (ICU_TEST_REPO, ICU_TEST_ICU_BIN)
"""
import hashlib, http.server, json, os, pathlib, plistlib, re, shutil, signal, socket, subprocess, sys, threading, time, traceback
sys.path.insert(0, str(pathlib.Path(__file__).resolve().parent))
from macos_probe import Session, wait_for, center

if sys.platform != 'darwin': raise SystemExit('macOS-only probe')
work = pathlib.Path(sys.argv[1])
repo = pathlib.Path(os.environ['ICU_TEST_REPO']); icu_bin = pathlib.Path(os.environ['ICU_TEST_ICU_BIN'])
report = {'source_commit': os.environ.get('ICU_TEST_APP_REVISION', 'unknown'), 'checks': {},
          'scope': 'icu layer from impl/install.sh (call + mcp) on Clock/Music/TextEdit and a Numbers stand-in; Copilot.app chat loop on the stand-in',
          'probe_sha256': hashlib.sha256(pathlib.Path(__file__).read_bytes()).hexdigest()}
s = Session(work, *sys.argv[2:5], report)
OPEN, CLOSE = '<app_specific_instructions>', '</app_specific_instructions>'
PACKS = {'com.apple.clock': 'Clock.md', 'com.apple.Music': 'AppleMusic.md', 'com.apple.iWork.Numbers': 'Numbers.md'}
shipped = dict(reversed(l.split('  ', 1)) for l in (repo / 'reference/openai-sky/app-bundle-packs/SHA256SUMS.txt').read_text().splitlines() if l.strip())
model_requests = []; server = None; launched = []


def check(name, ok, detail=None):
    report['checks'][name] = {'ok': bool(ok), 'detail': detail}


def pack(file):
    body = (repo / 'impl/app-instructions/packs' / file).read_bytes()
    assert hashlib.sha256(body).hexdigest() == shipped[file], f'{file} differs from the shipped bundle hash'
    return body.decode()


ICU_SOCK = str(work / 'icu-driver.sock')


def icu_env(session, daemon=True, **extra):
    # README route on macOS: a `cua-driver serve` daemon plus ICU_SOCKET.
    env = {k: v for k, v in os.environ.items() if not k.startswith('ICU_')}
    env.update(ICU_HOME=str(work / 'icu-home'), ICU_DRIVER=str(icu_bin / 'cua-driver'), ICU_SESSION=session, **extra)
    if daemon: env['ICU_SOCKET'] = ICU_SOCK
    return env


def icu_call(session, tool, args, daemon=True, **extra):
    out = subprocess.run([str(icu_bin / 'icu'), 'call', tool, json.dumps(args)], capture_output=True, text=True, timeout=60, env=icu_env(session, daemon, **extra))
    try: return json.loads(out.stdout)
    except ValueError: raise RuntimeError(f'icu call {tool} failed: {out.stdout[:300]} {out.stderr[:300]}')


def state_text(result):
    """The accessibility text a model sees from window2 get_window_state."""
    if isinstance(result, dict):
        if isinstance((result.get('accessibility') or {}).get('tree'), str): return result['accessibility']['tree']
        if isinstance(result.get('text'), str): return result['text']
        for c in result.get('content', []):
            if c.get('type') == 'text':
                try: return state_text(json.loads(c['text']))  # MCP wraps the WindowState as JSON text
                except (ValueError, RuntimeError): return c['text']
        sc = result.get('structuredContent')
        if isinstance(sc, dict) and isinstance(sc.get('text'), str): return sc['text']
    raise RuntimeError('no text in ' + json.dumps(result)[:300])


def delivered(text):
    """(pack body or None, count of opening tags)."""
    count = text.count(OPEN)
    if not text.startswith(OPEN + '\n'): return None, count
    return text[len(OPEN) + 1:text.index('\n' + CLOSE + '\n')], count


def observe(session, wid, **extra):
    return state_text(icu_call(session, 'get_window_state', {'window': {'id': wid}, 'include_text': True, 'include_screenshot': False}, **extra))


class Model(http.server.BaseHTTPRequestHandler):
    def log_message(self, *args): pass
    def do_POST(self):
        body = json.loads(self.rfile.read(int(self.headers['Content-Length'])))
        model_requests.append(body)
        text = next(m['content'] for m in body['messages'] if m['role'] == 'user')
        tree = text.partition('\n\nWINDOW ELEMENT TREE:\n')[2].rpartition('\n\nNext action as JSON:')[0]
        if '"Result" value="42"' in tree: decision = {'action': {'kind': 'done', 'summary': 'Observed 42'}}
        else: decision = {'thought': 'compute', 'actions': [{'kind': 'click', 'target': t} for t in ['C', '6', '×', '7', '=']]}
        encoded = json.dumps({'message': {'role': 'assistant', 'content': json.dumps(decision, ensure_ascii=False)}}).encode()
        self.send_response(200); self.send_header('Content-Type', 'application/json'); self.send_header('Content-Length', str(len(encoded))); self.end_headers(); self.wfile.write(encoded)


def request_tree(body):
    text = next(m['content'] for m in body['messages'] if m['role'] == 'user')
    return text.partition('\n\nWINDOW ELEMENT TREE:\n')[2].rpartition('\n\nNext action as JSON:')[0]


signal.signal(signal.SIGALRM, lambda *_: (_ for _ in ()).throw(TimeoutError('packs probe deadline'))); signal.alarm(360)
try:
    # A Numbers stand-in: the owned fixture in a bundle claiming Numbers' identifier.
    standin = work / 'Numbers Stand-in.app'
    (standin / 'Contents/MacOS').mkdir(parents=True)
    shutil.copy2(s.fixture, standin / 'Contents/MacOS/minicalc_mac')
    (standin / 'Contents/Info.plist').write_bytes(plistlib.dumps({'CFBundleIdentifier': 'com.apple.iWork.Numbers', 'CFBundleName': 'NumbersStandIn',
                                                                  'CFBundleExecutable': 'minicalc_mac', 'CFBundlePackageType': 'APPL'}))
    subprocess.run(['/usr/bin/codesign', '--force', '--sign', '-', str(standin)], check=True, capture_output=True)
    subprocess.run(['/usr/bin/open', '-g', '-n', '-a', str(standin), '--args', str(work), 'calc'], check=True, timeout=20)
    for app in ['Clock', 'Music', 'TextEdit']:
        subprocess.run(['/usr/bin/open', '-g', '-a', app], check=True, timeout=20)
    pids = {}
    for bid in ['com.apple.clock', 'com.apple.Music', 'com.apple.TextEdit', 'com.apple.iWork.Numbers']:
        pids[bid] = wait_for(lambda: (s.ui('pid-of', bid)['pids'] or [None])[0], 20, bid + ' did not start')
    launched = [pids[b] for b in ('com.apple.clock', 'com.apple.Music', 'com.apple.TextEdit', 'com.apple.iWork.Numbers')]
    time.sleep(3)
    # The default install path without a daemon, as a fresh user would run it.
    report['icu_call_without_daemon'] = icu_call('no-daemon', 'list_windows', {}, daemon=False)
    # The standalone daemon disclaims responsibility and needs its own grants;
    # these rows stand in for the user enabling it in System Settings.
    for service in ('kTCCServiceAccessibility', 'kTCCServiceScreenCapture'):
        subprocess.run(['/usr/bin/sudo', '/usr/bin/sqlite3', '/Library/Application Support/com.apple.TCC/TCC.db',
                        f"insert or replace into access (service, client, client_type, auth_value, auth_reason, auth_version, flags, indirect_object_identifier) "
                        f"values ('{service}', '{icu_bin / 'cua-driver'}', 1, 2, 4, 1, 0, 'UNUSED')"], check=True)
    s.launch([str(icu_bin / 'cua-driver'), 'serve', '--socket', ICU_SOCK], 'icu-daemon')
    wait_for(lambda: os.path.exists(ICU_SOCK), 15, 'icu daemon socket missing')
    listing = icu_call('discovery', 'list_windows', {})
    (work / 'icu-list-windows.json').write_text(json.dumps(listing, indent=2, ensure_ascii=False))
    windows = listing if isinstance(listing, list) else (listing.get('structuredContent') or listing).get('windows') or []
    def window_of(bid, title=None):
        found = [w for w in windows if w.get('app') == bid and (w.get('title') == title if title else w.get('title'))]
        return found[0]['id'] if found else None
    # Music can also own untitled or auxiliary titled windows; use its main one.
    wins = {'clock': window_of('com.apple.clock'), 'music': window_of('com.apple.Music', 'Music'), 'textedit': window_of('com.apple.TextEdit'),
            'numbers': window_of('com.apple.iWork.Numbers', 'ICU Calculator')}
    report['windows'] = wins
    assert all(wins.values()), f'missing windows: {wins}'
    clock = pack('Clock.md'); music = pack('AppleMusic.md'); numbers = pack('Numbers.md')

    first = observe('s1', wins['clock']); second = observe('s1', wins['clock'])
    body, count = delivered(first)
    check('clock_first_touch_verbatim', body == clock and count == 1, {'count': count, 'prefix': first[:60]})
    check('clock_second_touch_silent', OPEN not in second, second[:80])
    plain = observe('s1', wins['textedit']); after = observe('s1', wins['music'])
    check('app_without_pack_gets_none', OPEN not in plain, plain[:80])
    check('music_first_touch_verbatim', delivered(after)[0] == music, after[:80])
    num1 = observe('s1', wins['numbers']); num2 = observe('s1', wins['numbers'])
    check('numbers_standin_once_in_window2_path', delivered(num1)[0] == numbers and OPEN not in num2, [num1[:60], num2[:60]])
    other = observe('s2', wins['clock'])
    check('new_session_delivers_again', delivered(other)[0] == clock, other[:60])
    personal = work / 'personal'; personal.mkdir()
    (personal / 'com.apple.clock.md').write_text('PERSONAL CLOCK RULES\n')
    replaced = observe('s3', wins['clock'], ICU_PERSONAL_PACKS=str(personal))
    # A personal pack is delivered with trailing whitespace trimmed (impl/axdiff/src/layer.rs).
    check('personal_pack_overrides_by_default', delivered(replaced)[0] == 'PERSONAL CLOCK RULES', replaced[:120])
    both = observe('s4', wins['clock'], ICU_PERSONAL_PACKS=str(personal), ICU_PERSONAL_OVERRIDES='0')
    got = delivered(both)[0] or ''
    check('personal_pack_precedes_when_not_overriding', got == 'PERSONAL CLOCK RULES\n\n' + clock, got[:80])

    # MCP stdio: one server process is one session.
    mcp = subprocess.Popen([str(icu_bin / 'icu'), 'mcp'], stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.DEVNULL, text=True, env=icu_env('mcp-unused'))
    def rpc(i, method, params):
        mcp.stdin.write(json.dumps({'jsonrpc': '2.0', 'id': i, 'method': method, 'params': params}) + '\n'); mcp.stdin.flush()
        while True:
            line = json.loads(mcp.stdout.readline())
            if line.get('id') == i: return line
    rpc(1, 'initialize', {'protocolVersion': '2025-06-18', 'capabilities': {}, 'clientInfo': {'name': 'probe', 'version': '1'}})
    mcp.stdin.write(json.dumps({'jsonrpc': '2.0', 'method': 'notifications/initialized'}) + '\n'); mcp.stdin.flush()
    calls = [rpc(2 + i, 'tools/call', {'name': 'get_window_state', 'arguments': {'window': {'id': wins['clock']}, 'include_text': True, 'include_screenshot': False}}) for i in range(2)]
    mcp.terminate(); mcp.wait(5)
    texts = [state_text(c['result']) for c in calls]
    check('mcp_process_delivers_once', delivered(texts[0])[0] == clock and OPEN not in texts[1], [t[:60] for t in texts])

    # Copilot.app's own chat loop on the stand-in window.
    with socket.socket() as port:
        assert port.connect_ex(('127.0.0.1', 11434)) != 0, 'port 11434 already has a listener'
    server = http.server.ThreadingHTTPServer(('127.0.0.1', 11434), Model)
    threading.Thread(target=server.serve_forever, daemon=True).start()
    s.start_app(['ICU_BRAIN=ollama'])
    s.pick_window('ICU Calculator')
    field = lambda: s.find(lambda n: n.get('role') == 'AXTextField' and 'e.g. in @Calculator' in n.get('placeholder', ''))
    runs = []
    for task in range(2):
        (work / 'command-calc.json').write_text(json.dumps({'cmd': 'clear'}))  # each task starts from 0
        wait_for(lambda: not (work / 'command-calc.json').exists(), 5, 'stand-in did not clear'); time.sleep(.3)
        start = len(model_requests)
        s.ui('click', *center(field())); s.ui('key', 0, 'cmd'); s.ui('type', 'Compute six times seven using the calculator buttons.')
        wait_for(lambda: (field() or {}).get('value', '').startswith('Compute six'), 5, 'goal not typed')
        s.press('Send')
        wait_for(lambda: s.chat_text('done. Observed 42') and len(model_requests) > start and s.find(s.named('Send', 'AXButton')), 30, 'Copilot task did not finish')
        trees = [request_tree(b) for b in model_requests[start:]]
        runs.append([{'count': t.count(OPEN), 'verbatim': delivered(t)[0] == numbers} for t in trees])
        time.sleep(.5)
    report['copilot_requests'] = runs
    for i, r in enumerate(runs):
        (work / f'copilot-task{i + 1}-request1.txt').write_text(request_tree(model_requests[sum(len(x) for x in runs[:i])]))
    check('copilot_first_request_carries_pack_once', all(r[0]['count'] == 1 and r[0]['verbatim'] for r in runs), runs)
    check('copilot_later_requests_silent', all(x['count'] == 0 for r in runs for x in r[1:]) and all(len(r) >= 2 for r in runs), runs)
    report['ok'] = all(c['ok'] for c in report['checks'].values())
except Exception as error:
    report.update(ok=False, error=str(error), error_type=type(error).__name__, traceback=traceback.format_exc())
    s.screenshot('failure.png')
finally:
    signal.alarm(0)
    if server: server.shutdown(); server.server_close()
    try:
        # The standalone daemon detaches from its launcher; stop it by its own pid.
        from macos_probe import protocol
        daemon = protocol(ICU_SOCK, {'method': 'metadata'}, 3)['pid']; os.kill(daemon, signal.SIGTERM); report['icu_daemon_stopped'] = daemon
    except Exception as error: report['icu_daemon_stop_error'] = repr(error)
    s.close()
    for pid in launched:
        try: s.ui('quit', pid, check=False)
        except Exception: pass
    (work / 'result.json').write_text(json.dumps(report, indent=2, ensure_ascii=False) + '\n')
    for k, c in report['checks'].items(): print(('PASS ' if c['ok'] else 'FAIL ') + k, json.dumps(c['detail'], ensure_ascii=False)[:300])
    print(json.dumps({'ok': report.get('ok'), 'error': report.get('error')}))
    if report.get('error'): print(report.get('traceback', ''))
    sys.exit(0 if report.get('ok') else 1)
