"""Installed Copilot.app task acceptance on a disposable macOS session.

Run only on a throwaway Mac (never a developer's desktop), inside the console
user's GUI bootstrap context. The probe plays the user with real HID input for
setup, then switches the foreground to a separate guard app and samples
frontmost app, keyboard focus, pointer, HID idle and driver-owned on-screen
windows while the app drives the owned AppKit calculator in the background.

  probe_macos_task.py <work> <Copilot.app> <minicalc_mac> <macos_ui>
  ICU_TEST_MODE=complete|stop_model|tool_error   ICU_TEST_REFERENCES=1
  ICU_TEST_MODEL_URL=http://127.0.0.1:11500/<bridge token>      relay every request to a real model
      (the app's claude_code_bridge over a reverse tunnel) instead of the stub;
      only `complete` mode, and the completion summary is the model's own.
  ICU_TEST_NEGATIVE=pointer|focus|overlay        deliberately break one invariant;
      the run passes only when the matching guard reports the violation.
      overlay launches the bundled driver without --no-overlay via ICU_CUA_BIN.
"""
import hashlib, http.server, json, os, pathlib, re, signal, socket, subprocess, sys, threading, time, traceback
sys.path.insert(0, str(pathlib.Path(__file__).resolve().parent))
from macos_probe import Session, wait_for, center

if sys.platform != 'darwin': raise SystemExit('macOS-only probe')
work = pathlib.Path(sys.argv[1])
mode = os.environ.get('ICU_TEST_MODE', 'complete')
check_references = os.environ.get('ICU_TEST_REFERENCES') == '1'
negative = os.environ.get('ICU_TEST_NEGATIVE', '')
relay_url = os.environ.get('ICU_TEST_MODEL_URL', '').rstrip('/')
if relay_url and (mode != 'complete' or negative): raise SystemExit('the real-model relay runs only the complete task')
if mode not in ('complete', 'stop_model', 'tool_error') or negative not in ('', 'pointer', 'focus', 'overlay'): raise SystemExit('unsupported mode')
EXPECTED_NEGATIVE = {'pointer': 'automation changed foreground, keyboard focus or pointer',
                     'focus': 'automation changed foreground, keyboard focus or pointer',
                     'overlay': 'the owned driver showed an on-screen window (overlay)'}
report = {'source_commit': os.environ.get('ICU_TEST_APP_REVISION', 'unknown'), 'mode': mode, 'check_references': check_references, 'negative_control': negative or None,
          'scope': 'installed Copilot.app launched through LaunchServices, deterministic localhost model stub, owned AppKit receiver, separate foreground guard app',
          'probe_sha256': hashlib.sha256(pathlib.Path(__file__).read_bytes()).hexdigest()}
s = Session(work, *sys.argv[2:5], report)
model_requests = []; first_request = threading.Event(); release_model = threading.Event(); model_outcome = {'fresh42': False, 'terminal': False}
sampler = None; server = None


class Model(http.server.BaseHTTPRequestHandler):
    def log_message(self, *args): pass
    def do_POST(self):
        body = json.loads(self.rfile.read(int(self.headers['Content-Length'])))
        model_requests.append(body); (work / f'model-request-{len(model_requests)}.json').write_text(json.dumps(body, indent=2))
        if relay_url: return self.relay(body)
        if len(model_requests) == 1:
            first_request.set()
            if not release_model.wait(20): decision = {'action': {'kind': 'fail', 'reason': 'focus guard was not established'}}
            # AppKit reports an AXValue write on a button as an unverifiable send,
            # so the macOS error case uses an AX action the button lacks.
            elif mode == 'tool_error': decision = {'thought': 'Exercise a native tool failure before a second action', 'actions': [{'kind': 'click', 'target': '6', 'action': 'cancel'}, {'kind': 'click', 'target': '7'}]}
            else: decision = {'thought': 'Compute six times seven using the fixture controls', 'actions': [{'kind': 'click', 'target': t} for t in ['6', '×', '7', '=']]}
        else:
            text = next(m['content'] for m in body['messages'] if m['role'] == 'user')
            observation = text.partition('\n\nWINDOW ELEMENT TREE:\n')[2].rpartition('\n\nNext action as JSON:')[0]
            model_outcome['observation_excerpt'] = observation[:1500]
            # The AppKit readout is an AXTextField labelled Result.
            model_outcome['fresh42'] = bool(re.search(r'^\[\d+\] AXTextField "Result" value="42"$', observation, re.M))
            model_outcome['terminal'] = True
            decision = {'action': {'kind': 'done', 'summary': 'Observed 42'}} if model_outcome['fresh42'] else {'action': {'kind': 'fail', 'reason': '42 absent from fresh observation'}}
        encoded = json.dumps({'message': {'role': 'assistant', 'content': json.dumps(decision, ensure_ascii=False)}}).encode()
        self.send_response(200); self.send_header('Content-Type', 'application/json'); self.send_header('Content-Length', str(len(encoded))); self.end_headers(); self.wfile.write(encoded)


    def relay(self, body):
        # The model decides; this only forwards and records.
        if len(model_requests) == 1:
            first_request.set(); release_model.wait(20)
        text = next(m['content'] for m in body['messages'] if m['role'] == 'user')
        observation = text.partition('\n\nWINDOW ELEMENT TREE:\n')[2].rpartition('\n\nNext action as JSON:')[0]
        started = time.monotonic()
        import urllib.request, urllib.error
        request = urllib.request.Request(relay_url + self.path, json.dumps(body).encode(), {'content-type': 'application/json'})
        try:
            with urllib.request.urlopen(request, timeout=150) as response: status, encoded = response.status, response.read()
        except urllib.error.HTTPError as error: status, encoded = error.code, error.read()
        except Exception as error: status, encoded = 502, json.dumps({'error': f'relay: {error!r}'}).encode()
        call = {'n': len(model_requests), 'ms': round((time.monotonic() - started) * 1000), 'status': status}
        try:
            call['reply'] = json.loads(encoded)['message']['content']
            kinds = re.findall(r'"kind"\s*:\s*"(\w+)"', call['reply'])
            if kinds and kinds[-1] in ('done', 'fail'):
                model_outcome['terminal'] = True
                model_outcome['fresh42'] = bool(re.search(r'^\[\d+\] AXTextField "Result" value="42"$', observation, re.M))
        except Exception as error: call['reply_error'] = repr(error)
        model_outcome.setdefault('calls', []).append(call)
        self.send_response(status); self.send_header('Content-Type', 'application/json'); self.send_header('Content-Length', str(len(encoded))); self.end_headers(); self.wfile.write(encoded)


def entry():
    return s.find(lambda n: n.get('role') == 'AXTextField' and 'e.g. in @Calculator' in n.get('placeholder', ''))


def entry_value():
    node = entry(); return node.get('value', '') if node else None


def fixture_state():
    return json.loads((work / 'fixture-state.json').read_text())


def deadline(_s, _f): raise TimeoutError('overall macOS probe deadline')
signal.signal(signal.SIGALRM, deadline); signal.signal(signal.SIGTERM, deadline); signal.alarm(600 if relay_url else 180)
try:
    report['trust'] = s.ui('trusted')
    with socket.socket() as probe_port:
        assert probe_port.connect_ex(('127.0.0.1', 11434)) != 0, 'port 11434 already has a listener; refusing to share the model endpoint'
    server = http.server.ThreadingHTTPServer(('127.0.0.1', 11434), Model)
    threading.Thread(target=server.serve_forever, daemon=True).start()
    for f in work.glob('fixture-state.json'): f.unlink()
    fx, _ = s.fixture_proc('calc'); guard, _ = s.fixture_proc('guard')
    report.update(fixture_pid=fx.pid, guard_pid=guard.pid)
    env = ['ICU_BRAIN=ollama']
    if negative == 'overlay':
        # Same bundled binary, started without the owned overlay policy.
        wrapper = work / 'overlay-driver.sh'
        wrapper.write_text('#!/bin/sh\nargs=""\nfor a in "$@"; do [ "$a" = "--no-overlay" ] || args="$args $a"; done\n'
                           f'exec "{s.driver_exe}" $args\n')
        wrapper.chmod(0o755); env.append(f'ICU_CUA_BIN={wrapper}')
    s.start_app(env, expect_no_overlay=negative != 'overlay')
    windows = s.windows(); (work / 'native-windows.json').write_text(json.dumps(windows, indent=2))
    target_record = next(w for w in windows if w.get('pid') == fx.pid and w.get('title') == 'ICU Calculator')
    sibling_record = next(w for w in windows if w.get('pid') == fx.pid and w.get('title') == 'ICU Focus Guard')
    fixture_window = target_record['window_id']
    report['stage'] = 'pick-window'
    s.pick_window('ICU Calculator')
    # Before any task: the owned daemon must not show a window of its own.
    report['setup_state'] = s.ui('state', s.driver_pid)
    report['driver_onscreen_windows_example'] = report['setup_state'].get('owned_onscreen_windows') or None
    assert not report['setup_state'].get('owned_onscreen_windows'), 'the owned driver showed an on-screen window (overlay)'
    report['stage'] = 'enter-goal'
    field = wait_for(entry, 8, 'command input unavailable')
    s.ui('click', *center(field))
    wait_for(lambda: s.ui('state')['front_pid'] == s.app_pid, 5, 'user click did not bring Copilot forward')
    wait_for(lambda: (entry() or {}).get('focused'), 5, 'command entry did not acquire focus')
    goal = 'Verify that a rejected value change stops the task.' if mode == 'tool_error' else 'Compute six times seven using the calculator buttons.'
    def replace_command(text):
        s.ui('click', *center(entry()))
        s.ui('key', 0, 'cmd')  # Cmd+A
        s.ui('type', text)
    if check_references:
        def select_reference(title):
            replace_command('@')
            rows = lambda: [n for n in s.tree() if n.get('role') == 'AXButton' and title in n.get('name', '') and n.get('frame', {}).get('w', 0) > 0 and n['frame']['y'] > 500]
            row = wait_for(lambda: (lambda r: r[0] if len(r) == 1 else None)(rows()), 8, 'mention suggestion missing or ambiguous: ' + title)
            s.ui('click', *center(row))
            return wait_for(lambda: entry_value().strip() if re.fullmatch(r'@[^\s@]+', (entry_value() or '').strip()) else None, 4, 'mention was not inserted')
        assert target_record['app_name'] == sibling_record['app_name'], 'fixture does not exercise same-name aliases'
        first = select_reference('ICU Calculator'); second = select_reference('ICU Focus Guard')
        assert first.lower() != second.lower(), 'same-name windows share a reference alias'
        for draft, message in [('in @UnknownAuditWindow type hello', 'Select each @window'), (first + ' ' + second + ' ' + goal, 'Choose one window')]:
            replace_command(draft)
            wait_for(lambda: entry_value() == draft, 5, 'draft did not reach the command field')
            s.press('Send')
            wait_for(lambda: s.chat_text(message), 5, 'reference validation did not reach GUI: ' + message)
            assert entry_value() == draft, 'invalid-reference submission lost its draft'
            assert not first_request.is_set(), 'invalid reference reached the model'
        assert fixture_state() == {'display': '0', 'buttons': []}, 'invalid reference caused input'
        report['references'] = {'first': first, 'second': second, 'target_window': fixture_window, 'sibling_window': sibling_record['window_id'], 'unresolved_rejected': True, 'multiple_rejected': True}
        replace_command(first + ' ' + goal)
        wait_for(lambda: entry_value() == first + ' ' + goal, 5, 'goal draft did not reach the command field')
    else:
        s.ui('type', goal)
    before = s.call('get_window_state', {'pid': fx.pid, 'window_id': fixture_window, 'include_screenshot': False})
    (work / 'initial-driver-state.json').write_text(json.dumps(before, indent=2))
    before_state = before.get('structuredContent', before)
    if mode == 'tool_error':
        target = next(e for e in before_state['elements'] if e.get('label') == '6' and e.get('element_token'))
        native_error = s.call('click', {'pid': fx.pid, 'window_id': fixture_window, 'element_token': target['element_token'], 'action': 'cancel'})
        (work / 'native-tool-error.json').write_text(json.dumps(native_error, indent=2))
        assert native_error.get('isError') is True, 'fixture button unexpectedly accepted AXCancel'
        report['native_error_code'] = native_error.get('structuredContent', {}).get('code')
        assert fixture_state() == {'display': '0', 'buttons': []}, 'error oracle changed receiver state'
    report['stage'] = 'submit-goal'
    s.press('Send')
    assert first_request.wait(15), 'task did not reach the configured model endpoint'
    # The user leaves for another app while the model is thinking.
    guard_field = wait_for(lambda: next((n for n in s.tree(guard.pid) if n.get('role') == 'AXTextField'), None), 5, 'guard field missing')
    s.ui('click', *center(guard_field)); clicked_at = time.time()
    wait_for(lambda: (lambda st: st['front_pid'] == guard.pid and st.get('focused_element', {}).get('pid') == guard.pid)(s.ui('state')), 5, 'guard app did not take foreground and keyboard focus')
    baseline = s.ui('state', f'{s.driver_pid},{s.app_pid}'); report['baseline'] = baseline
    assert baseline.get('focused_element', {}).get('role') == 'AXTextField' and baseline.get('focused_window') == 'ICU Foreground Guard', 'guard field does not own keyboard focus'
    sampler = subprocess.Popen([str(s.ui_bin), 'sample', str(work / 'ownership.jsonl'), f'{s.driver_pid}'], stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    time.sleep(.3)
    if negative == 'pointer': s.ui('move', baseline['pointer'][0] + 40, baseline['pointer'][1] - 40)
    if negative == 'focus': s.ui('click', 960, 110)  # the user clicks Copilot's title bar
    if mode == 'stop_model':
        s.press('Stop')
        report['stopping_before_response'] = bool(wait_for(lambda: s.find(s.named('Stopping…', 'AXButton')), 5, 'GUI did not acknowledge cancellation'))
        assert not release_model.is_set()
    release_model.set()
    if mode == 'tool_error':
        wait_for(lambda: s.find(s.named('Send', 'AXButton')), 12, 'failed native task did not reach a terminal GUI state')
        state = fixture_state()
        assert state == {'display': '0', 'buttons': []}, 'native tool error was followed by another action'
        assert len(model_requests) == 1, 'native tool error reached another model decision'
        report['ui_error_summary'] = bool(wait_for(lambda: s.chat_text('Action failed. Stopped without replaying'), 6, 'native tool failure did not reach the GUI'))
    elif mode == 'stop_model':
        report['ui_stopped_summary'] = bool(wait_for(lambda: s.chat_text('stopped: stopped by request'), 10, 'cancellation did not reach the GUI'))
        state = fixture_state()
        assert state == {'display': '0', 'buttons': []}, 'cancelled model response still caused native input'
        assert len(model_requests) == 1, 'cancelled run contacted the model again'
    else:
        state = wait_for(lambda: (lambda st: st if st['display'] == '42' and model_outcome['terminal'] else None)(fixture_state()), 40, 'task did not reach 42 and a terminal model decision') if not relay_url else wait_for(lambda: (lambda st: st if model_outcome['terminal'] else None)(fixture_state()), 400, 'the model never reached a terminal decision')
        final = s.call('get_window_state', {'pid': fx.pid, 'window_id': fixture_window, 'include_screenshot': False, 'cumulative_diff': True})
        (work / 'final-driver-state.json').write_text(json.dumps(final, indent=2))
        report['native_diff_has_42'] = '42' in json.dumps(final.get('structuredContent', final).get('diff', ''))
        verification = s.call('verify_state', {'pid': fx.pid, 'window_id': fixture_window, 'expect': [{'element': {'selector': {'label_contains': 'Result'}, 'exists': True, 'value_equals': '42'}}], 'timeout_ms': 3000, 'stable_samples': 2, 'include_screenshot': False}, 10)
        (work / 'native-verification.json').write_text(json.dumps(verification, indent=2))
        verified = verification.get('structuredContent', verification)
        report['native_predicate'] = {'status': verified.get('status'), 'stable': verified.get('stable')}
        assert model_outcome['fresh42'], 'model observation after the batch did not contain the fresh readout'
        assert state['buttons'] == ['6', '×', '7', '='], 'input multiplicity/order mismatch'
        assert report['native_diff_has_42'] and report['native_predicate'] == {'status': 'satisfied', 'stable': True}, 'native diff/predicate did not confirm 42'
        report['ui_done_summary'] = bool(wait_for(lambda: s.chat_text('done. ' if relay_url else 'done. Observed 42'), 8, 'completion summary did not reach the GUI'))
    report['fixture_state'] = state; report['model_requests'] = len(model_requests); report['model_outcome'] = model_outcome
    report['ui_returned_to_send'] = bool(wait_for(lambda: s.find(s.named('Send', 'AXButton')), 8, 'terminal event did not reset the GUI'))
    time.sleep(.5)
    sampler.send_signal(signal.SIGTERM); sampler.wait(5)
    samples = [json.loads(l) for l in (work / 'ownership.jsonl').read_text().splitlines() if l.strip()]
    report['ownership_samples'] = len(samples)
    report['ownership_violations'] = [x for x in samples if x['front_pid'] != guard.pid or x['menubar_pid'] != guard.pid
                                      or x.get('ax_focused_app', guard.pid) != guard.pid or x.get('focused_window') != 'ICU Foreground Guard'
                                      or x.get('focused_element') != baseline.get('focused_element') or x['pointer'] != baseline['pointer']][:10]
    # Any HID-level event resets the system idle counter.
    report['hid_resets'] = [x for x in samples if x['hid_idle'] + 0.25 < x['time'] - clicked_at][:10]
    report['driver_onscreen_window_samples'] = sum(bool(x.get('owned_onscreen_windows')) for x in samples)
    report['driver_onscreen_windows_example'] = next((x['owned_onscreen_windows'] for x in samples if x.get('owned_onscreen_windows')), None)
    assert len(samples) >= 20, 'ownership observation was too short'
    assert report['driver_onscreen_window_samples'] == 0, 'the owned driver showed an on-screen window (overlay)'
    assert not report['ownership_violations'], 'automation changed foreground, keyboard focus or pointer'
    assert not report['hid_resets'], 'HID-level input occurred after the user left Copilot'
    events = [json.loads(l) for l in (work / 'receiver-events.jsonl').read_text().splitlines()] if (work / 'receiver-events.jsonl').exists() else []
    report['receiver_events'] = events
    assert [e['button'] for e in events] == (['6', '×', '7', '='] if mode == 'complete' else []), 'receiver callbacks disagree with the expected effects'
    assert all(not e['active'] for e in events), 'receiver became active during a callback'
    assert not (work / 'guard-events.jsonl').exists(), 'keystrokes reached the foreground guard field'
    s.screenshot('completed.png')
    report['ok'] = True
except Exception as error:
    report.update(ok=False, error=str(error), error_type=type(error).__name__, traceback=traceback.format_exc())
    try: report['ui_nodes'] = [{k: n.get(k) for k in ('role', 'name', 'value')} for n in s.tree()][:400]
    except Exception: pass
    s.screenshot('failure.png')
finally:
    signal.alarm(0)
    release_model.set()
    if sampler and sampler.poll() is None: sampler.send_signal(signal.SIGTERM)
    if server: server.shutdown(); server.server_close()
    s.close()
    if negative:
        # A negative control passes only when its guard catches the violation.
        report['negative_expected_error'] = EXPECTED_NEGATIVE[negative]
        report['negative_detected'] = report.get('error') == EXPECTED_NEGATIVE[negative]
        report['ok'] = report['negative_detected'] and report.get('owned_daemon_exited', False)
    (work / 'result.json').write_text(json.dumps(report, indent=2) + '\n')
    print(json.dumps({k: v for k, v in report.items() if k not in ('ui_nodes', 'traceback', 'receiver_events', 'baseline')}, indent=1)[:5000])
    if report.get('error') and not negative: print(report.get('traceback', ''))
    sys.exit(0 if report.get('ok') else 1)
