"""Signed update check, download, install and restart for Copilot.app.

Disposable Mac only. Uses the bundles from macos_update_builds.sh (throwaway
key, localhost endpoint) and serves latest.json over https://localhost:8443.

Phases, each with a narrow pass condition:
1. check: 0.1.0 shows "Update 0.2.0 is ready" for a valid manifest;
2. install: after Install & restart, a new process runs the on-disk 0.2.0
   bundle whose app and driver bytes equal the 0.2.0 build, its owned driver is
   the bundled binary with --no-overlay, and the app-data sentinel survives;
   the TCC grant pinned to 0.1.0's ad-hoc code requirement is reported, not
   assumed (ad-hoc signatures change on every build);
3. wrong key: a 0.3.0 manifest whose signature comes from an untrusted key
   shows "Update failed" and leaves the 0.2.0 bundle bytes and process running;
4. no endpoint: with the server down the app starts quietly with no toast.

  probe_macos_update.py <work> <builds-dir> <minicalc_mac> <macos_ui>
"""
import hashlib, http.server, json, os, pathlib, plistlib, signal, ssl, subprocess, sys, threading, time, traceback
sys.path.insert(0, str(pathlib.Path(__file__).resolve().parent))
from macos_probe import Session, wait_for, protocol, alive, BUNDLE_ID

if sys.platform != 'darwin': raise SystemExit('macOS-only probe')
work = pathlib.Path(sys.argv[1]); builds = pathlib.Path(sys.argv[2]).resolve()
installed = pathlib.Path('/Applications/Copilot.app')
report = {'source_commit': os.environ.get('ICU_TEST_APP_REVISION', 'unknown'), 'phases': {},
          'scope': 'installed Copilot.app, throwaway minisign key, https://localhost:8443 endpoint with a throwaway trusted CA; nothing published',
          'probe_sha256': hashlib.sha256(pathlib.Path(__file__).read_bytes()).hexdigest()}
s = Session(work, installed if installed.exists() else builds / '0.1.0/Copilot.app', sys.argv[3], sys.argv[4], report)
TCC = '/Library/Application Support/com.apple.TCC/TCC.db'
DATA = pathlib.Path.home() / 'Library/Application Support' / BUNDLE_ID
serve_dir = work / 'www'; serve_dir.mkdir(parents=True, exist_ok=True)
server = None


def sha(p): return hashlib.sha256(pathlib.Path(p).read_bytes()).hexdigest()
def bundle_hashes(app): return {n: sha(pathlib.Path(app) / 'Contents/MacOS' / n) for n in ('app', 'cua-driver')}
def version(app): return plistlib.loads((pathlib.Path(app) / 'Contents/Info.plist').read_bytes())['CFBundleShortVersionString']
def sudo(*args): return subprocess.run(['/usr/bin/sudo', *args], capture_output=True, text=True, check=True).stdout


def install(src):
    sudo('/bin/rm', '-rf', str(installed)); sudo('/usr/bin/ditto', str(src), str(installed))
    sudo('/usr/sbin/chown', '-R', f'{os.getuid()}:admin', str(installed))


def grant(requirement_hex=None):
    csreq = f"X'{requirement_hex}'" if requirement_hex else 'NULL'
    for service in ('kTCCServiceAccessibility', 'kTCCServiceScreenCapture'):
        sudo('/usr/bin/sqlite3', TCC, f"insert or replace into access (service, client, client_type, auth_value, auth_reason, auth_version, csreq, flags, indirect_object_identifier) "
                                      f"values ('{service}', '{BUNDLE_ID}', 0, 2, 4, 1, {csreq}, 0, 'UNUSED')")


def pinned_requirement(app):
    """The binary code requirement System Settings would record for this build."""
    text = subprocess.run(['/usr/bin/codesign', '-d', '-r-', str(app)], capture_output=True, text=True).stdout + \
           subprocess.run(['/usr/bin/codesign', '-d', '-r-', str(app)], capture_output=True, text=True).stderr
    dr = next(l.split('=>', 1)[1].strip() for l in text.splitlines() if 'designated =>' in l)
    out = work / 'requirement.bin'
    subprocess.run(['/usr/bin/csreq', '-r-', '-b', str(out)], input=dr, text=True, check=True)
    return dr, out.read_bytes().hex()


def manifest(version_, archive, signature):
    (serve_dir / 'latest.json').write_text(json.dumps({'version': version_, 'notes': 'probe', 'pub_date': '2026-09-25T00:00:00Z',
        'platforms': {'darwin-aarch64': {'signature': pathlib.Path(signature).read_text(), 'url': f'https://localhost:8443/{pathlib.Path(archive).name}'}}}))
    (serve_dir / pathlib.Path(archive).name).write_bytes(pathlib.Path(archive).read_bytes())


class Quiet(http.server.SimpleHTTPRequestHandler):
    def __init__(self, *a, **k): super().__init__(*a, directory=str(serve_dir), **k)
    def log_message(self, fmt, *args): (work / 'server.log').open('a').write((fmt % args) + '\n')


def start_server():
    global server
    server = http.server.ThreadingHTTPServer(('127.0.0.1', 8443), Quiet)
    ctx = ssl.SSLContext(ssl.PROTOCOL_TLS_SERVER); ctx.load_cert_chain(builds / 'server.pem', builds / 'server.key')
    server.socket = ctx.wrap_socket(server.socket, server_side=True)
    threading.Thread(target=server.serve_forever, daemon=True).start()


def stop_server():
    global server
    if server: server.shutdown(); server.server_close(); server = None


def toast(text):
    # The toast splits its text across nodes ("Update", <b>"0.2.0"</b>, "is ready"),
    # so compare with whitespace removed and keep the last tree for diagnosis.
    squash = lambda t: ''.join(str(t or '').split())
    nodes = s.tree(); (work / 'last-tree.json').write_text(json.dumps(nodes, ensure_ascii=False, indent=0))
    return any(squash(text) in squash(n.get('name')) + squash(n.get('value')) for n in nodes) or \
        squash(text) in ''.join(squash(n.get('value') if n.get('role') == 'AXStaticText' else '') for n in nodes)


def driver_view():
    raw = s.call('check_permissions', {}); sc = raw.get('structuredContent', raw)
    return {'accessibility': sc.get('accessibility'), 'screen_recording': sc.get('screen_recording')}


def restart():
    if s.app_pid and alive(s.app_pid):
        s.ui('quit', s.app_pid); wait_for(lambda: not alive(s.app_pid), 10, 'app did not quit')
    s.app_pid = None
    s.start_app()


signal.signal(signal.SIGALRM, lambda *_: (_ for _ in ()).throw(TimeoutError('update probe deadline'))); signal.alarm(420)
try:
    v1, v2 = builds / '0.1.0/Copilot.app', builds / '0.2.0/Copilot.app'
    report['build_hashes'] = {'0.1.0': bundle_hashes(v1), '0.2.0': bundle_hashes(v2),
                              'archive_0.2.0': sha(builds / '0.2.0/Copilot.app.tar.gz'), 'wrong_key_archive': sha(builds / 'wrong-key.app.tar.gz')}
    install(v1)
    assert version(installed) == '0.1.0'
    report['app_sha256'], report['driver_sha256'] = bundle_hashes(installed).values()  # Session hashed the previous install
    dr, req_hex = pinned_requirement(installed); report['pinned_requirement'] = dr
    grant(req_hex)
    DATA.mkdir(parents=True, exist_ok=True); sentinel = DATA / 'icu-update-sentinel.txt'; sentinel.write_text('kept across updates\n')

    p = report['phases']['check'] = {}
    manifest('0.2.0', builds / '0.2.0/Copilot.app.tar.gz', builds / '0.2.0/Copilot.app.tar.gz.sig')
    start_server()
    s.start_app()
    old_pid, old_driver = s.app_pid, s.driver_pid
    p['driver_view_0.1.0'] = driver_view()
    wait_for(lambda: toast('Update 0.2.0 is ready'), 20, 'update toast did not appear')
    s.screenshot('toast.png'); p['ok'] = True

    p = report['phases']['install'] = {}
    s.press('Install & restart')
    new_pid = wait_for(lambda: next((x for x in s.ui('pid-of', BUNDLE_ID)['pids'] if x != old_pid and alive(x)), None) if not alive(old_pid) else None, 90, 'app did not restart')
    s.app_pid = new_pid; s.sock = pathlib.Path.home() / 'Library/Caches/icu' / f'driver-{new_pid}.sock'
    meta = wait_for(lambda: (lambda: protocol(s.sock, {'method': 'metadata'}, 2))() if s.sock.exists() else None, 40, 'restarted app has no owned driver')
    s.driver_pid = meta['pid']
    p.update(old_pid=old_pid, new_pid=new_pid, on_disk_version=version(installed), on_disk_hashes=bundle_hashes(installed),
             new_process_path=s.ui('pidpath', new_pid)['path'], driver_path=s.ui('pidpath', s.driver_pid)['path'],
             driver_argv=subprocess.check_output(['/bin/ps', '-o', 'args=', '-p', str(s.driver_pid)], text=True).strip(),
             old_driver_exited=not alive(old_driver), sentinel_kept=sentinel.exists() and sentinel.read_text() == 'kept across updates\n',
             codesign=subprocess.run(['/usr/bin/codesign', '--verify', '--deep', '--strict', str(installed)], capture_output=True, text=True).returncode == 0)
    time.sleep(3)
    p['driver_view_0.2.0_with_0.1.0_grant'] = driver_view()
    p['permission_notice_after_update'] = toast('access to see and drive windows')
    s.screenshot('after-update.png')
    p['ok'] = (p['on_disk_version'] == '0.2.0' and p['on_disk_hashes'] == report['build_hashes']['0.2.0']
               and p['new_process_path'] == str(installed / 'Contents/MacOS/app') and p['driver_path'] == str(installed / 'Contents/MacOS/cua-driver')
               and '--no-overlay' in p['driver_argv'].split() and p['old_driver_exited'] and p['sentinel_kept'] and p['codesign'])
    # Ad-hoc code requirements pin one build; say whether the grant carried over.
    p['grant_retained'] = p['driver_view_0.2.0_with_0.1.0_grant'] == {'accessibility': True, 'screen_recording': True}

    p = report['phases']['wrong_key'] = {}
    grant()  # the user re-grants 0.2.0
    manifest('0.3.0', builds / 'wrong-key.app.tar.gz', builds / 'wrong-key.app.tar.gz.sig')
    restart()
    before = bundle_hashes(installed)
    wait_for(lambda: toast('Update 0.3.0 is ready'), 20, 'wrong-key toast did not appear')
    s.press('Install & restart')
    wait_for(lambda: toast('Update failed'), 30, 'wrong-key update did not report failure')
    time.sleep(2)
    p.update(version_after=version(installed), bytes_unchanged=bundle_hashes(installed) == before, app_alive=alive(s.app_pid), driver_alive=alive(s.driver_pid),
             failure_logged=any('update' in n.get('name', '') and 'failed' in n.get('name', '') for n in s.tree()))
    s.screenshot('wrong-key.png')
    p['ok'] = p['version_after'] == '0.2.0' and p['bytes_unchanged'] and p['app_alive'] and p['driver_alive']

    p = report['phases']['no_endpoint'] = {}
    stop_server(); restart()
    time.sleep(10)
    p.update(toast=toast('is ready'), failed=toast('Update failed'), driver_view=driver_view())
    p['ok'] = not p['toast'] and not p['failed']
    report['ok'] = all(ph.get('ok') for ph in report['phases'].values())
except Exception as error:
    report.update(ok=False, error=str(error), error_type=type(error).__name__, traceback=traceback.format_exc())
    s.screenshot('failure.png')
finally:
    signal.alarm(0)
    stop_server()
    try: grant()
    except Exception: pass
    s.close()
    (work / 'result.json').write_text(json.dumps(report, indent=2, ensure_ascii=False) + '\n')
    print(json.dumps({k: v for k, v in report.items() if k != 'traceback'}, indent=1, ensure_ascii=False)[:6000])
    if report.get('error'): print(report.get('traceback', ''))
    sys.exit(0 if report.get('ok') else 1)
