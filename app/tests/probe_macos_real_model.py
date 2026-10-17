"""Installed Copilot.app on a disposable Mac, driven by a REAL model through a relay.

The macOS counterpart of probe_linux_real_model.py, with the same tasks. The
app uses its Ollama adapter against 127.0.0.1:11434; this probe answers there
and forwards each request unchanged to ICU_TEST_MODEL_URL, where the app's
`claude_code_bridge` test harness answers through `claude -p` (reached over a
tunnel; the Mac has no Claude login). Nothing here scripts an action. The probe
sets up the desktop, picks the window and submits each goal through the GUI
like a user, hands foreground and keyboard focus to a separate guard app while
the model works, samples foreground/focus/pointer/HID idle until the GUI
returns to Send, and reads the fixture's own state afterwards.

Run only on a throwaway Mac, inside the console user's GUI bootstrap context.

  probe_macos_real_model.py <work> <Copilot.app> <minicalc_mac> <macos_ui> <task>
  ICU_TEST_MODEL_URL=http://127.0.0.1:11500/<bridge token>

TASK is calc, type, two_window or refusal. A later goal is the user's retry,
submitted only if the earlier run left the fixture short of the goal.
"""
import hashlib, http.server, json, os, pathlib, re, signal, socket, subprocess, sys, threading, time, traceback, urllib.error, urllib.request
import math
sys.path.insert(0, str(pathlib.Path(__file__).resolve().parent))
from macos_probe import (Session, wait_for, center, HARNESS_CAPTURE_PROMPT, ProbeInterrupted,
                         cleanup_signals, defer_interruptions, interrupt_probe, install_interrupt_handlers, stop_child, write_report)
from real_model_verdict import verdict
from task_quality import TASKS, start_trial_app, task_success

if sys.platform != 'darwin' or os.environ.get('ICU_DISPOSABLE_HOST') != '1':
    raise SystemExit('real-model input requires an explicitly disposable macOS host')
work = pathlib.Path(sys.argv[1]); task = sys.argv[5]
relay_url = os.environ['ICU_TEST_MODEL_URL'].rstrip('/')

spec = TASKS[task]
report = {'task': task, 'goals': spec['goals'], 'fixture_mode': spec['mode'],
          'measurement_kind': 'live_model_desktop',
          'scope': 'installed Copilot.app via LaunchServices, real model via relay, owned AppKit fixture, separate foreground guard app',
          'source_commit': os.environ.get('ICU_TEST_APP_REVISION', 'unknown'),
          'probe_sha256': hashlib.sha256(pathlib.Path(__file__).read_bytes()).hexdigest()}
s = Session(work, *sys.argv[2:5], report)
calls = []; hold = threading.Event(); first_request = threading.Event()
server = None; server_thread = None; sampler = None


class Relay(http.server.BaseHTTPRequestHandler):
    def log_message(self, *args): pass
    def do_POST(self):
        body = self.rfile.read(int(self.headers['Content-Length']))
        first_request.set()
        # Hold only while the probe hands foreground to the guard app.
        hold.wait(30)
        started = time.monotonic(); entry = {'n': len(calls) + 1}; calls.append(entry)
        request = urllib.request.Request(relay_url + self.path, body, {'content-type': 'application/json'})
        try:
            with urllib.request.urlopen(request, timeout=150) as response: status, reply = response.status, response.read()
        except urllib.error.HTTPError as error: status, reply = error.code, error.read()
        except Exception as error: status, reply = 502, json.dumps({'error': f'relay: {error!r}'}).encode()
        entry.update(ms=round((time.monotonic() - started) * 1000), status=status)
        try:
            user = next(m['content'] for m in json.loads(body)['messages'] if m['role'] == 'user')
            entry['history'] = user.partition('ACTIONS SO FAR:\n')[2].partition('\n\nWINDOW ELEMENT TREE:')[0]
            (work / f'model-request-{entry["n"]}.txt').write_text(user)
            decoded = json.loads(reply)
            entry['reply'] = decoded.get('message', {}).get('content') if status == 200 else decoded.get('error')
            entry['model'] = decoded.get('model')
        except Exception as error: entry['reply_error'] = repr(error)
        entry['complete'] = True
        self.send_response(status); self.send_header('Content-Type', 'application/json')
        self.send_header('Content-Length', str(len(reply))); self.end_headers(); self.wfile.write(reply)


def fixture_state():
    return json.loads((work / spec['state']).read_text())


def fixture_events(started, ended):
    # The owned fixture creates this log on its first event. Absence means no event,
    # unlike a missing ownership sampler. Reject malformed records rather than dropping them.
    path = work / 'receiver-events.jsonl'
    rows = [json.loads(line) for line in path.read_text().splitlines() if line.strip()] if path.exists() else []
    if any(not isinstance(row, dict) or type(row.get('time')) not in (float, int) or not math.isfinite(row['time']) for row in rows):
        raise ValueError('malformed independent fixture event log')
    return [row for row in rows if started <= row['time'] <= ended]


def chat_lines():
    return [n['name'] for n in s.tree() if n.get('role') == 'AXStaticText' and n.get('name') and not re.fullmatch(r'\d\d:\d\d:\d\d', n['name'])]


def wait_for_relay_idle():
    deadline = time.monotonic() + 160
    quiet_since = None; count = len(calls)
    while time.monotonic() < deadline:
        if len(calls) != count or not all(call.get('complete') for call in calls):
            quiet_since = None; count = len(calls)
        elif quiet_since is None:
            quiet_since = time.monotonic()
        elif time.monotonic() - quiet_since >= .5:
            return
        time.sleep(.05)
    raise RuntimeError('post-task model request did not complete')


install_interrupt_handlers()
signal.signal(signal.SIGALRM, interrupt_probe); signal.alarm(900)
try:
    report['trust'] = s.ui('trusted')
    # The first harness screencapture on a fresh image raises an alert over
    # whatever is under it; take and approve it now, before anything is on screen.
    s.screenshot('setup.png')
    report['harness_capture_prompt'] = s.approve_capture_prompt(prompt=HARNESS_CAPTURE_PROMPT)
    with socket.socket() as probe_port:
        assert probe_port.connect_ex(('127.0.0.1', 11434)) != 0, 'port 11434 already has a listener; refusing to share the model endpoint'
    server = http.server.ThreadingHTTPServer(('127.0.0.1', 11434), Relay)
    server.daemon_threads = True  # interrupted relay requests cannot block desktop cleanup
    server_thread = threading.Thread(target=server.serve_forever, daemon=True)
    server_thread.start()
    for f in work.glob('*-state.json'): f.unlink()
    fx, _ = s.fixture_proc(spec['mode']); guard, _ = s.fixture_proc('guard')
    report.update(fixture_pid=fx.pid, guard_pid=guard.pid, initial_state=fixture_state())
    start_trial_app(s, work, report)
    windows = s.windows(); (work / 'native-windows.json').write_text(json.dumps(windows, indent=2))
    report['fixture_windows'] = [w.get('title') for w in windows if w.get('pid') == fx.pid]
    report['stage'] = 'pick-window'
    s.pick_window(spec['window'])
    report['runs'] = []
    for i, goal in enumerate(spec['goals']):
        if i and task_success(task, fixture_state()) is True: break
        run = {'goal': goal, 'first_call': len(calls) + 1}; report['runs'].append(run)
        report['stage'] = f'goal-{i + 1}'
        # Every goal, including a retry, returns through the owned app's title
        # bar before clicking the command field underneath the guard window.
        run['command_handoff'] = s.enter_goal(goal)
        hold.clear(); first_request.clear()
        s.press('Send'); sent = time.monotonic()
        assert first_request.wait(15), 'task did not reach the model endpoint'
        # The user leaves for another app while the model works.
        # Ensure the owned field is not covered by Copilot before its real user click.
        run['guard_handoff'] = s.show_guard(guard)
        guard_field = wait_for(lambda: next((n for n in s.tree(guard.pid) if n.get('role') == 'AXTextField'), None), 5, 'guard field missing')
        s.ui('click', *center(guard_field)); clicked_at = time.time()
        wait_for(lambda: (lambda st: st['front_pid'] == guard.pid and st.get('focused_element', {}).get('pid') == guard.pid)(s.ui('state')), 5,
                 'guard app did not take foreground and keyboard focus')
        baseline = s.ui('state', f'{s.driver_pid},{s.app_pid}')
        samples_path = work / f'ownership-{i + 1}.jsonl'
        with defer_interruptions():
            sampler = subprocess.Popen([str(s.ui_bin), 'sample', str(samples_path), f'{s.driver_pid}'], stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
        time.sleep(.3)
        run['measurement_started_unix'] = time.time()
        hold.set()
        # Stop is shown while the run lives; Send comes back at its terminal event.
        wait_for(lambda: s.find(s.named('Stop', 'AXButton')) is None and s.find(s.named('Send', 'AXButton')), 400, 'the run never reached a terminal state', .5)
        # The app records task memory after emitting its terminal event. Keep
        # observing until its in-flight real-model reply has been retained;
        # closing the app/server here used to serialize an unfinished call.
        wait_for_relay_idle()
        run['wall_s'] = round(time.monotonic() - sent, 1)
        time.sleep(.5)
        run['measurement_ended_unix'] = time.time()
        sampler.send_signal(signal.SIGTERM); sampler.wait(5); sampler = None
        samples = [json.loads(l) for l in samples_path.read_text().splitlines() if l.strip()]
        run['ownership_samples'] = len(samples)
        run['ownership_violations'] = [x for x in samples if x['front_pid'] != guard.pid or x['menubar_pid'] != guard.pid
                                       or x.get('ax_focused_app', guard.pid) != guard.pid or x.get('focused_window') != 'ICU Foreground Guard'
                                       or x.get('focused_element') != baseline.get('focused_element') or x['pointer'] != baseline['pointer']][:10]
        run['hid_resets'] = [x for x in samples if x['hid_idle'] + 0.25 < x['time'] - clicked_at][:10]
        run['driver_onscreen_window_samples'] = sum(bool(x.get('owned_onscreen_windows')) for x in samples)
        run['calls'] = calls[run['first_call'] - 1:]
        # The whole visible chat: the command pane runs oldest first, the actions pane newest first.
        run['chat'] = chat_lines()[:80]
        run['fixture_state'] = fixture_state()
        run['fixture_events'] = fixture_events(run['measurement_started_unix'], run['measurement_ended_unix'])
        run['fixture_events_complete'] = True
        run['success'] = task_success(task, run['fixture_state']) is True
        s.screenshot(f'after-goal-{i + 1}.png')
    report['guard_keystrokes'] = (work / 'guard-events.jsonl').exists()
    report['calls'] = len(calls)
    report['final_state'] = fixture_state()
    report['success'] = task_success(task, report['final_state']) is True
    report['ownership_clean'] = all(not r['ownership_violations'] and not r['hid_resets'] and not r['driver_onscreen_window_samples']
                                    and r['ownership_samples'] >= 20 for r in report['runs']) and not report['guard_keystrokes']
    # The probe completed; success and ownership are the findings, not probe failures.
    report['ok'] = True
except (Exception, ProbeInterrupted, KeyboardInterrupt) as error:
    report.update(ok=False, error=str(error), error_type=type(error).__name__, traceback=traceback.format_exc())
    if isinstance(error, (ProbeInterrupted, KeyboardInterrupt)):
        report['interruption'] = {'type': type(error).__name__, 'message': str(error)}
        if isinstance(error, ProbeInterrupted): report['interruption']['signal'] = signal.Signals(error.signum).name
    else:
        # Failure diagnostics are optional and must not prevent finalization.
        try: report['ui_nodes'] = [{k: n.get(k) for k in ('role', 'name', 'value')} for n in s.tree()][:400]
        except Exception: pass
        try: s.screenshot('failure.png')
        except Exception as diagnostic: report['failure_screenshot_error'] = str(diagnostic)
finally:
    with cleanup_signals():
        signal.alarm(0)
        hold.set()
        report['all_calls'] = calls
        # Persist the incomplete attempt before cleanup. A forced supervisor
        # exit must leave unknown cleanup, not a prior successful result.
        report['owned_daemon_exited'] = False
        report['owned_processes_reaped'] = False
        def checkpoint():
            try: write_report(work / 'result.json', report)
            except Exception as error:
                report.setdefault('cleanup_errors', []).append({'stage': 'report', 'error': str(error)})
                report['ok'] = False
                print(f'Could not persist probe report: {error}', file=sys.stderr)
        checkpoint()
        for name, finalize in (
            ('sampler', lambda: stop_child(sampler) if sampler else None),
            ('session', s.close),
            ('relay_shutdown', lambda: server.shutdown() if server and server_thread and server_thread.is_alive() else None),
            ('relay_close', lambda: server.server_close() if server else None),
        ):
            try: finalize()
            except Exception as error:
                report.setdefault('cleanup_errors', []).append({'stage': name, 'error': str(error)})
                report['ok'] = False
                report.setdefault('error', 'probe cleanup was incomplete')
            checkpoint()
        report['acceptance'] = verdict(report)
        checkpoint()
        print(json.dumps({k: v for k, v in report.items() if k not in ('ui_nodes', 'traceback', 'all_calls')}, indent=1, ensure_ascii=False)[:6000])
        if report.get('error'): print(report.get('traceback', ''))
    sys.exit(0 if report['acceptance']['passed'] else 1)
