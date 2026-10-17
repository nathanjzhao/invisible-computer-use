"""Clean-install permission onboarding for Copilot.app on a disposable Mac.

Removes Copilot's TCC grants (sudo, system TCC.db), launches the installed
bundle through LaunchServices and checks what a new user is told. Passes when
the app names the missing grants, asks macOS for them only after the user
chooses Grant access, and stops warning once the grants exist.

  probe_macos_permissions.py <work> <Copilot.app> <minicalc_mac> <macos_ui>
"""
import hashlib, json, os, pathlib, signal, subprocess, sys, time, traceback
sys.path.insert(0, str(pathlib.Path(__file__).resolve().parent))
from macos_probe import Session, wait_for, BUNDLE_ID

if sys.platform != 'darwin': raise SystemExit('macOS-only probe')
work = pathlib.Path(sys.argv[1])
report = {'source_commit': os.environ.get('ICU_TEST_APP_REVISION', 'unknown'),
          'scope': 'installed Copilot.app (LaunchServices) with its TCC rows removed, then restored',
          'probe_sha256': hashlib.sha256(pathlib.Path(__file__).read_bytes()).hexdigest()}
s = Session(work, *sys.argv[2:5], report)
TCC = '/Library/Application Support/com.apple.TCC/TCC.db'
SERVICES = ('kTCCServiceAccessibility', 'kTCCServiceScreenCapture')
NOTICE = 'access to see and drive windows'
PROMPT_OWNERS = ('UserNotificationCenter', 'universalAccessAuthWarn', 'System Settings', 'tccd')


def tcc(sql):
    return subprocess.run(['/usr/bin/sudo', '/usr/bin/sqlite3', TCC, sql], capture_output=True, text=True, check=True).stdout.strip()


def rows():
    return [dict(zip(('service', 'auth_value'), r.split('|'))) for r in tcc(f"select service, auth_value from access where client='{BUNDLE_ID}'").splitlines() if r]


def grant():
    for service in SERVICES:
        tcc(f"insert or replace into access (service, client, client_type, auth_value, auth_reason, auth_version, flags, indirect_object_identifier) values ('{service}', '{BUNDLE_ID}', 0, 2, 4, 1, 0, 'UNUSED')")


def notice():
    return next((n['name'] for n in s.tree() if NOTICE in n.get('name', '') and n.get('role') in ('AXStaticText', 'AXGroup')), None)


def driver_view():
    raw = s.call('check_permissions', {}); sc = raw.get('structuredContent', raw)
    return {'accessibility': sc.get('accessibility'), 'screen_recording': sc.get('screen_recording')}


def prompts():
    found = []
    for w in s.ui('onscreen')['windows']:
        if w['owner_name'] in PROMPT_OWNERS and w['layer'] >= 0:
            texts = ' '.join(n.get('name', '') for n in s.tree(w['owner']) if n.get('role') == 'AXStaticText')
            found.append({'owner': w['owner_name'], 'pid': w['owner'], 'text': texts[:300]})
    return found


def deadline(_s, _f): raise TimeoutError('permission probe deadline')
signal.signal(signal.SIGALRM, deadline); signal.alarm(150)
try:
    tcc(f"delete from access where client='{BUNDLE_ID}'")
    report['rows_before_launch'] = rows()
    fx, _ = s.fixture_proc('calc')
    s.start_app()
    time.sleep(4)
    report['driver_view_ungranted'] = driver_view()
    assert report['driver_view_ungranted'] == {'accessibility': False, 'screen_recording': False}, 'grants were not removed'
    report['windows_ungranted'] = [{'app': w['app_name'], 'title': w.get('title')} for w in s.windows() if w.get('pid') == fx.pid]
    s.screenshot('ungranted.png')
    report['notice_ungranted'] = notice()
    report['prompts_before_request'] = prompts()
    assert not report['prompts_before_request'], 'a system prompt appeared before the user asked'
    assert report['notice_ungranted'], 'clean install shows no permission guidance'
    s.press('Grant access')
    report['prompts_after_request'] = wait_for(lambda: prompts() or None, 8, 'Grant access showed no system prompt')
    time.sleep(1)
    report['rows_after_request'] = rows()
    s.screenshot('requested.png')
    # Stand in for the user enabling Copilot in System Settings.
    grant(); report['rows_after_grant'] = rows()
    for p in report['prompts_after_request']:
        s.ui('quit', p['pid'], check=False)
    s.ui('quit', s.app_pid); wait_for(lambda: not s.ui('pid-of', BUNDLE_ID)['pids'], 10, 'Copilot did not quit')
    s.app_pid = None
    s.start_app()
    time.sleep(4)
    report['driver_view_granted'] = driver_view()
    report['notice_granted'] = notice()
    s.screenshot('granted.png')
    assert report['driver_view_granted'] == {'accessibility': True, 'screen_recording': True}, 'restored grants were not seen'
    assert report['notice_granted'] is None, 'granted app still warns'
    report['ok'] = True
except Exception as error:
    report.update(ok=False, error=str(error), error_type=type(error).__name__, traceback=traceback.format_exc())
    s.screenshot('failure.png')
finally:
    signal.alarm(0)
    try: grant()
    except Exception as error: report['regrant_error'] = str(error)
    s.close()
    (work / 'result.json').write_text(json.dumps(report, indent=2, ensure_ascii=False) + '\n')
    print(json.dumps({k: v for k, v in report.items() if k != 'traceback'}, indent=1, ensure_ascii=False)[:4000])
    if report.get('error'): print(report.get('traceback', ''))
    sys.exit(0 if report.get('ok') else 1)
