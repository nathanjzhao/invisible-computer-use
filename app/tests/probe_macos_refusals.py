"""Model-facing refusals in real macOS driver output: set_value on a button and
the Apple menu.

Through installed Copilot.app's owned driver, against the owned AppKit
calculator:

- set_value on a button must refuse with a typed code and leave the fixture
  untouched, while set_value on the editable Result field is still verified;
- the window observation must not carry the Apple menu (Restart, Shut Down,
  Log Out, ...), while the app's own menu stays observable;
- invoke_menu into the Apple menu must refuse with a typed code before any
  focus change. "About This Mac" is the probe's target: on a driver without the
  refusal it only opens an information window.

  probe_macos_refusals.py <work> <Copilot.app> <minicalc_mac> <macos_ui>
"""
import hashlib, json, os, pathlib, signal, sys, time, traceback
sys.path.insert(0, str(pathlib.Path(__file__).resolve().parent))
from macos_probe import Session, wait_for

if sys.platform != 'darwin': raise SystemExit('macOS-only probe')
work = pathlib.Path(sys.argv[1])
report = {'source_commit': os.environ.get('ICU_TEST_APP_REVISION', 'unknown'), 'checks': {},
          'scope': "installed Copilot.app's owned driver, owned AppKit calculator",
          'probe_sha256': hashlib.sha256(pathlib.Path(__file__).read_bytes()).hexdigest()}
s = Session(work, *sys.argv[2:5], report)
SYSTEM_ITEMS = ('About This Mac', 'System Settings', 'Force Quit', 'Sleep', 'Restart', 'Shut Down', 'Lock Screen', 'Log Out')


def jsonl(name):
    p = work / name
    return [json.loads(l) for l in p.read_text().splitlines() if l.strip()] if p.exists() else []


def text(result):
    return ' '.join(x.get('text', '') for x in result.get('content', []) if x.get('type') == 'text')[:300]


def check(name, ok, detail=None):
    report['checks'][name] = {'ok': bool(ok), 'detail': detail}


signal.signal(signal.SIGALRM, lambda *_: (_ for _ in ()).throw(TimeoutError('refusals probe deadline'))); signal.alarm(180)
try:
    fx, _ = s.fixture_proc('calc')
    s.start_app()
    wid = next(w['window_id'] for w in s.windows() if w.get('pid') == fx.pid and w.get('title') == 'ICU Calculator')
    state = s.call('get_window_state', {'pid': fx.pid, 'window_id': wid, 'include_screenshot': False, 'disable_diff': True})
    (work / 'observation.json').write_text(json.dumps(state, indent=2, ensure_ascii=False))
    sc = state.get('structuredContent', state)
    observed = json.dumps(sc, ensure_ascii=False) + ' ' + text(state)
    labels = [e.get('label') or '' for e in sc['elements']] + [e.get('label') or '' for e in sc.get('passive_elements', [])]
    report['menu_bar_items'] = [e.get('label') for e in sc['elements'] if e.get('role') == 'AXMenuBarItem']
    check('apple_menu_not_observed', not any(item in observed for item in SYSTEM_ITEMS) and 'Apple' not in report['menu_bar_items'],
          [item for item in SYSTEM_ITEMS if item in observed])
    check('app_menu_still_observed', 'minicalc_mac' in report['menu_bar_items'], report['menu_bar_items'])

    def element(label, role):
        st = s.call('get_window_state', {'pid': fx.pid, 'window_id': wid, 'include_screenshot': False})
        return next(e for e in st.get('structuredContent', st)['elements'] if e.get('label') == label and e.get('role') == role)

    before = json.loads((work / 'fixture-state.json').read_text()); mark = time.time()
    button = s.call('set_value', {'pid': fx.pid, 'window_id': wid, 'element_token': element('7', 'AXButton')['element_token'], 'value': '5'})
    time.sleep(.5)
    report['set_value_button'] = {'isError': button.get('isError'), 'structured': button.get('structuredContent'), 'text': text(button),
                                  'fixture_before': before, 'fixture_after': json.loads((work / 'fixture-state.json').read_text()),
                                  'receiver_events': [e for e in jsonl('receiver-events.jsonl') if e['time'] >= mark]}
    b = report['set_value_button']
    check('set_value_button_refused_typed', b['isError'] is True and (b['structured'] or {}).get('code') == 'set_value_unsupported', b)
    check('set_value_button_changed_nothing', b['fixture_after'] == b['fixture_before'] and not b['receiver_events'])
    field = s.call('set_value', {'pid': fx.pid, 'window_id': wid, 'element_token': element('Result', 'AXTextField')['element_token'], 'value': '42'})
    report['set_value_field'] = {'isError': field.get('isError'), 'structured': field.get('structuredContent'), 'text': text(field)}
    check('set_value_field_still_verified', not field.get('isError') and (field.get('structuredContent') or {}).get('effect') == 'confirmed', report['set_value_field'])

    base = s.ui('state', str(s.driver_pid)); amark = time.time()
    menu = s.call('invoke_menu', {'pid': fx.pid, 'window_id': wid, 'path': ['Apple', 'About This Mac']})
    time.sleep(1)
    report['invoke_menu_apple'] = {'isError': menu.get('isError'), 'structured': menu.get('structuredContent'), 'text': text(menu),
                                   'front_before': base['front_pid'], 'front_after': s.ui('state')['front_pid'],
                                   'activation': [e for e in jsonl('activation-events.jsonl') if e['time'] >= amark]}
    m = report['invoke_menu_apple']
    check('invoke_menu_apple_refused_typed', m['isError'] is True and ((m['structured'] or {}).get('refusal') or {}).get('code') == 'system_menu_refused', m)
    check('invoke_menu_apple_no_focus_change', not m['activation'] and m['front_after'] == m['front_before'], m)
    report['ok'] = all(c['ok'] for c in report['checks'].values())
except Exception as error:
    report.update(ok=False, error=str(error), error_type=type(error).__name__, traceback=traceback.format_exc())
    s.screenshot('failure.png')
finally:
    signal.alarm(0)
    s.close()
    (work / 'result.json').write_text(json.dumps(report, indent=2, ensure_ascii=False) + '\n')
    for k, c in report['checks'].items(): print(('PASS ' if c['ok'] else 'FAIL ') + k, json.dumps(c['detail'], ensure_ascii=False)[:300])
    print(json.dumps({'ok': report.get('ok'), 'error': report.get('error')}))
    if report.get('error'): print(report.get('traceback', ''))
    sys.exit(0 if report.get('ok') else 1)
