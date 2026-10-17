"""Do the user's keystrokes stay in their app while the owned driver acts?

Sampling cannot see transients shorter than its period, so this probe types
continuously into a separate foreground guard app (real HID keys, one every
~20 ms) while Copilot's owned driver performs one background action on the
calculator. Every typed character must land in the guard field and none in the
calculator's editable display. AppKit activation of either app and sampled
WindowServer ownership are recorded for each route.

  probe_macos_focus.py <work> <Copilot.app> <minicalc_mac> <macos_ui>
"""
import hashlib, json, os, pathlib, signal, subprocess, sys, time, traceback
sys.path.insert(0, str(pathlib.Path(__file__).resolve().parent))
from macos_probe import Session, wait_for, center

if sys.platform != 'darwin': raise SystemExit('macOS-only probe')
work = pathlib.Path(sys.argv[1])
report = {'source_commit': os.environ.get('ICU_TEST_APP_REVISION', 'unknown'), 'cases': {},
          'scope': "installed Copilot.app's owned driver acting on a background AppKit calculator while the user types into a foreground guard app",
          'probe_sha256': hashlib.sha256(pathlib.Path(__file__).read_bytes()).hexdigest()}
s = Session(work, *sys.argv[2:5], report)
TYPED = 60


def jsonl(name):
    p = work / name
    return [json.loads(l) for l in p.read_text().splitlines() if l.strip()] if p.exists() else []


def command(mode, cmd):
    before = len(jsonl('command-results.jsonl'))
    (work / f'command-{mode}.json').write_text(json.dumps({'cmd': cmd}))
    return wait_for(lambda: (lambda r: r[before] if len(r) > before else None)(jsonl('command-results.jsonl')), 8, f'{mode} did not run {cmd}')


def deadline(_s, _f): raise TimeoutError('focus probe deadline')
signal.signal(signal.SIGALRM, deadline); signal.alarm(400)
try:
    fx, _ = s.fixture_proc('calc'); guard, _ = s.fixture_proc('guard')
    s.start_app()
    target = next(w for w in s.windows() if w.get('pid') == fx.pid and w.get('title') == 'ICU Calculator')
    wid = target['window_id']
    s.pick_window('ICU Calculator')

    def element(label):
        st = s.call('get_window_state', {'pid': fx.pid, 'window_id': wid, 'include_screenshot': False})
        sc = st.get('structuredContent', st)
        return next(e for e in sc['elements'] if e.get('label') == label), sc.get('window_bounds') or target['bounds']

    def point_of(label):
        e, wb = element(label); f = e['frame']
        return {'x': f['x'] + f['w'] / 2 - wb['x'], 'y': f['y'] + f['h'] / 2 - wb['y']}

    cases = [
        ('ax_element_click', None, lambda: {'name': 'click', 'args': {'element_token': element('6')[0]['element_token']}}),
        ('coordinate_on_button', None, lambda: {'name': 'click', 'args': point_of('6')}),
        ('coordinate_on_background', None, lambda: {'name': 'click', 'args': {'x': 300, 'y': 60}}),
        ('type_text', None, lambda: {'name': 'type_text', 'args': {'text': '5'}}),
        ('press_key', None, lambda: {'name': 'press_key', 'args': {'key': 'backspace'}}),
        ('drag_on_background', None, lambda: {'name': 'drag', 'args': {'from_x': 300, 'from_y': 40, 'to_x': 300, 'to_y': 70}}),
        ('ax_element_click_off_space', 'offspace', lambda: {'name': 'click', 'args': {'element_token': element('6')[0]['element_token']}}),
        ('coordinate_on_button_off_space', 'offspace', lambda: {'name': 'click', 'args': point_of('6')}),
    ]
    for name, space, build in cases:
        c = report['cases'][name] = {}
        command('calc', 'clear'); command('calc', 'reset_text')
        if space: command('calc', space); time.sleep(1)
        request = build()
        guard_field = next(n for n in s.tree(guard.pid) if n.get('role') == 'AXTextField')
        s.ui('click', *center(guard_field))
        wait_for(lambda: s.ui('state')['front_pid'] == guard.pid, 5, 'guard did not take the foreground')
        command('guard', 'reset_text')
        start_text = command('guard', 'text')['text']
        base = s.ui('state', str(s.driver_pid)); mark = time.time()
        path = work / f'ownership-{name}.jsonl'
        sampler = subprocess.Popen([str(s.ui_bin), 'sample', str(path), str(s.driver_pid)], stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
        typer = subprocess.Popen([str(s.ui_bin), 'type', 'x' * TYPED], stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
        time.sleep(.4)
        act_start = time.time()
        result = s.call(request['name'], {'pid': fx.pid, 'window_id': wid, **request['args']})
        c['action_window'] = [act_start, time.time()]
        typer.wait(30); time.sleep(.6)
        sampler.send_signal(signal.SIGTERM); sampler.wait(5)
        samples = [json.loads(l) for l in path.read_text().splitlines() if l.strip()]
        end_text = command('guard', 'text')['text']
        c.update(request=request, isError=result.get('isError'),
                 driver_text=' '.join(x.get('text', '') for x in result.get('content', []) if x.get('type') == 'text')[:240],
                 route=(result.get('structuredContent') or {}).get('route'),
                 guard_text_start=start_text, guard_x_received=end_text.count('x'), typed=TYPED,
                 display_edits=[e for e in jsonl('display-edits.jsonl') if e['time'] >= mark],
                 receiver_events=[e for e in jsonl('receiver-events.jsonl') if e['time'] >= mark],
                 activation=[e for e in jsonl('activation-events.jsonl') if e['time'] >= mark],
                 guard_observed=[e for e in jsonl('workspace-events.jsonl') if e['time'] >= mark],
                 samples=len(samples),
                 ownership_violations=[x for x in samples if x['front_pid'] != guard.pid or x['menubar_pid'] != guard.pid][:3])
        c['checks'] = {'all_keystrokes_stayed': c['guard_x_received'] == TYPED and not c['display_edits'],
                       'no_sampled_ownership_change': not c['ownership_violations'] and c['samples'] >= 20,
                       'no_appkit_activation': not c['activation'],
                       # Pushed to the guard, so no activation is too short to see.
                       'guard_saw_no_activation_change': not c['guard_observed']}
        if space: command('calc', 'onspace'); time.sleep(.5)
    report['failed_checks'] = {k: [n for n, v in c['checks'].items() if not v] for k, c in report['cases'].items() if not all(c['checks'].values())}
    report['ok'] = True  # the verdict per route is in failed_checks; the run itself completed
except Exception as error:
    report.update(ok=False, error=str(error), error_type=type(error).__name__, traceback=traceback.format_exc())
    s.screenshot('failure.png')
finally:
    signal.alarm(0)
    s.close()
    (work / 'result.json').write_text(json.dumps(report, indent=2, ensure_ascii=False) + '\n')
    for k, c in report['cases'].items():
        print(k, 'route=%s err=%s x=%s/%s edits=%d samples=%d violations=%d activation=%s' % (
            c.get('route'), c.get('isError'), c.get('guard_x_received'), c.get('typed'), len(c.get('display_edits', [])),
            c.get('samples', 0), len(c.get('ownership_violations', [])), [(e['mode'], e['kind'], round(e['time'] % 100, 3)) for e in c.get('activation', [])]))
        print('    guard saw:', [(e['kind'], e['name'], round(e['time'] % 100, 3)) for e in c.get('guard_observed', [])])
        print('   ', c.get('driver_text', '')[:160])
    print(json.dumps({'ok': report.get('ok'), 'error': report.get('error'), 'failed_checks': report.get('failed_checks')}))
    if report.get('error'): print(report.get('traceback', ''))
    sys.exit(0 if report.get('ok') else 1)
