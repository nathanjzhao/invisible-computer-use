"""Shared plumbing for the installed-app macOS probes.

Only for a disposable Mac session: the probes post real HID input as the user
and read the whole desktop. See probe_macos_task.py / probe_macos_manual.py.
"""
import hashlib, json, os, pathlib, signal, socket, subprocess, time
from contextlib import contextmanager

BUNDLE_ID = 'com.nathanzhao.app'
CAPTURE_PROMPT = '“Copilot” is requesting to bypass the system private window picker'
HARNESS_CAPTURE_PROMPT = '“vmguest” is requesting to bypass the system private window picker'


class ProbeInterrupted(BaseException):
    """An operator/deadline interruption must not be swallowed by probe retries."""
    def __init__(self, signum):
        self.signum = signum
        super().__init__(f"probe interrupted by {signal.Signals(signum).name}")


def interrupt_probe(signum, _frame):
    raise ProbeInterrupted(signum)


def install_interrupt_handlers():
    previous = {sig: signal.getsignal(sig) for sig in (signal.SIGINT, signal.SIGTERM)}
    for sig in previous: signal.signal(sig, interrupt_probe)
    return previous


def restore_signal_handlers(previous):
    for sig, handler in previous.items(): signal.signal(sig, handler)


@contextmanager
def cleanup_signals():
    """A repeated termination request must not interrupt bounded finalizers."""
    previous = {sig: signal.getsignal(sig) for sig in (signal.SIGINT, signal.SIGTERM, signal.SIGALRM)}
    for sig in previous: signal.signal(sig, signal.SIG_IGN)
    try: yield
    finally: restore_signal_handlers(previous)


@contextmanager
def defer_interruptions():
    """Finish registering a just-launched owned process before unwinding."""
    pending = []
    previous = {sig: signal.getsignal(sig) for sig in (signal.SIGINT, signal.SIGTERM, signal.SIGALRM)}
    for sig in previous: signal.signal(sig, lambda signum, _frame: pending.append(signum))
    try: yield
    finally: restore_signal_handlers(previous)
    if pending: raise ProbeInterrupted(pending[0])


def process_identity(pid):
    """Bind a PID to its observed start time and executable, not its name alone."""
    result = subprocess.run(['/bin/ps', '-ww', '-p', str(pid), '-o', 'lstart=', '-o', 'comm='],
                            capture_output=True, text=True, timeout=3)
    return result.stdout.strip() if result.returncode == 0 and result.stdout.strip() else None


def stop_child(process, seconds=5):
    """Terminate and reap exactly the retained Popen child."""
    if process.poll() is None:
        try: process.terminate()
        except ProcessLookupError: pass
    try: process.wait(seconds)
    except subprocess.TimeoutExpired:
        process.kill(); process.wait(5)


def write_report(path, report):
    path = pathlib.Path(path)
    pending = path.with_suffix(path.suffix + '.pending')
    pending.write_text(json.dumps(report, indent=2, ensure_ascii=False) + '\n')
    pending.replace(path)


def wait_for(predicate, seconds, message, interval=.1):
    end = time.monotonic() + seconds
    while time.monotonic() < end:
        value = predicate()
        if value: return value
        time.sleep(interval)
    raise RuntimeError(message)


def protocol(path, request, timeout=6):
    with socket.socket(socket.AF_UNIX) as conn:
        conn.settimeout(timeout); conn.connect(str(path)); conn.sendall((json.dumps(request) + '\n').encode()); data = b''
        end = time.monotonic() + timeout
        while b'\n' not in data:
            if time.monotonic() >= end: raise TimeoutError('protocol deadline')
            chunk = conn.recv(65536)
            if not chunk: raise ConnectionError('peer closed')
            data += chunk
        value = json.loads(data.split(b'\n')[0]); assert value.get('ok'), value
        return value['result']


def alive(pid):
    try: os.kill(pid, 0); return True
    except ProcessLookupError: return False
    except PermissionError: return True


def center(node):
    f = node['frame']; return f['x'] + f['w'] / 2, f['y'] + f['h'] / 2


class Session:
    def __init__(self, work, bundle, fixture, ui_bin, report):
        self.work = pathlib.Path(work); self.work.mkdir(parents=True, exist_ok=True)
        self.bundle = pathlib.Path(bundle).resolve(); self.fixture = pathlib.Path(fixture).resolve(); self.ui_bin = pathlib.Path(ui_bin).resolve()
        self.app_exe = self.bundle / 'Contents/MacOS/app'; self.driver_exe = self.bundle / 'Contents/MacOS/cua-driver'
        self.report = report; self.processes = []; self.logs = []; self.app_pid = None; self.driver_pid = None; self.sock = None
        self.owned_identities = {}
        report.update(app_sha256=hashlib.sha256(self.app_exe.read_bytes()).hexdigest(),
                      driver_sha256=hashlib.sha256(self.driver_exe.read_bytes()).hexdigest())

    def ui(self, *args, check=True):
        out = subprocess.run([str(self.ui_bin), *map(str, args)], capture_output=True, text=True, timeout=30)
        value = json.loads(out.stdout.strip().splitlines()[-1]) if out.stdout.strip() else {'ok': False, 'error': out.stderr}
        if check and value.get('ok') is False: raise RuntimeError(f'macos_ui {args[0]} failed: {value}')
        return value

    def launch(self, args, name):
        f = open(self.work / (name + '.log'), 'wb'); self.logs.append(f)
        with defer_interruptions():
            p = subprocess.Popen(args, stdin=subprocess.DEVNULL, stdout=f, stderr=f, start_new_session=True)
            self.processes.append(p)
        return p

    def fixture_proc(self, mode):
        ready = self.work / f'fixture-{mode}-ready.json'
        if ready.exists(): ready.unlink()
        p = self.launch([str(self.fixture), str(self.work), mode], 'fixture-' + mode)
        wait_for(ready.exists, 15, f'{mode} fixture not ready')
        return p, json.loads(ready.read_text())

    def remember_owned_identity(self, kind, pid, expected_path):
        before = process_identity(pid)
        actual = pathlib.Path(self.ui('pidpath', pid)['path']).resolve()
        after = process_identity(pid)
        assert before and before == after and actual == pathlib.Path(expected_path).resolve(), f'{kind} process identity was not verified'
        self.owned_identities[kind] = (pid, before)
        self.report[kind + '_identity'] = {'pid': pid, 'process': before, 'path': str(actual)}

    def show_guard(self, guard):
        """Raise only this probe's guard fixture before the existing user click."""
        assert guard in self.processes and guard.poll() is None, 'guard is not a live owned fixture'
        assert pathlib.Path(self.ui('pidpath', guard.pid)['path']).resolve() == self.fixture, 'guard executable identity changed'
        receipts = self.work / 'command-results.jsonl'
        offset = len(receipts.read_text().splitlines()) if receipts.exists() else 0
        requested = time.time()
        command = self.work / 'command-guard.json'
        assert not command.exists(), 'another guard setup command is pending'
        write_report(command, {'cmd': 'show'})
        def shown():
            assert guard.poll() is None, 'guard exited before handoff'
            rows = receipts.read_text().splitlines()[offset:] if receipts.exists() else []
            for line in rows:
                row = json.loads(line); window = row.get('window', {})
                if row.get('mode') == 'guard' and row.get('cmd') == 'show' and window.get('time', 0) >= requested:
                    assert not row.get('error') and window.get('title') == 'ICU Foreground Guard', 'guard show receipt has the wrong window'
                    assert window.get('visible') is True and window.get('on_active_space') is True and window.get('miniaturized') is False, 'guard is not visible on the current desktop'
                    return row
            return None
        return wait_for(shown, 5, 'owned guard did not confirm its show command')

    def start_app(self, env=(), expect_no_overlay=True):
        # LaunchServices children are not Popen children. Complete their exact
        # identity registration before honoring a pending operator interruption.
        with defer_interruptions():
            return self._start_app(env, expect_no_overlay)

    def _start_app(self, env=(), expect_no_overlay=True):
        """Launch the bundle through LaunchServices, like a Finder/Dock launch.

        ICU_TEST_TAP=1 interposes driver_socket_tap.py to record the app's
        driver requests; the owned-driver identity checks are then skipped.
        """
        assert not self.ui('pid-of', BUNDLE_ID)['pids'], 'another Copilot instance is already running'
        tap = os.environ.get('ICU_TEST_TAP') == '1'
        if tap:
            env = [*env, f'ICU_CUA_BIN={pathlib.Path(__file__).resolve().parent / "driver_socket_tap.py"}', f'ICU_TAP_REAL_DRIVER={self.driver_exe}']
            self.report['driver_tap'] = True
        args = ['/usr/bin/open', '-g', '-n', '-a', str(self.bundle)]
        for item in env: args += ['--env', item]
        subprocess.run(args, check=True, timeout=20)
        self.app_pid = wait_for(lambda: (self.ui('pid-of', BUNDLE_ID)['pids'] or [None])[0], 20, 'Copilot did not start')
        self.report['app_pid'] = self.app_pid
        self.remember_owned_identity('app', self.app_pid, self.app_exe)
        self.sock = pathlib.Path.home() / 'Library/Caches/icu' / f'driver-{self.app_pid}.sock'
        def metadata():
            if not alive(self.app_pid): raise RuntimeError('Copilot exited')
            try: return protocol(self.sock, {'method': 'metadata'}, 2)
            except (OSError, ValueError, AssertionError): return None
        meta = wait_for(metadata, 40, 'owned driver not ready', .25); self.report['metadata'] = meta
        self.driver_pid = meta['pid']
        if tap:
            # Tap mode still has to prove this daemon is a child of the app.
            parent = int(subprocess.check_output(['/bin/ps', '-o', 'ppid=', '-p', str(self.driver_pid)], text=True).strip())
            assert parent == self.app_pid, 'tap daemon is not a child of the app'
            self.remember_owned_identity('driver', self.driver_pid, self.ui('pidpath', self.driver_pid)['path'])
            self.report['driver_argv'] = subprocess.check_output(['/bin/ps', '-o', 'args=', '-p', str(self.driver_pid)], text=True).strip()
            return meta
        self.report['driver_path'] = self.ui('pidpath', self.driver_pid)['path']
        assert pathlib.Path(self.report['driver_path']).resolve() == self.driver_exe.resolve(), 'owned daemon is not the bundled driver'
        self.report['driver_argv'] = subprocess.check_output(['/bin/ps', '-o', 'args=', '-p', str(self.driver_pid)], text=True).strip()
        self.report['driver_ppid'] = int(subprocess.check_output(['/bin/ps', '-o', 'ppid=', '-p', str(self.driver_pid)], text=True).strip())
        assert self.report['driver_ppid'] == self.app_pid, 'owned daemon is not a child of the app'
        self.remember_owned_identity('driver', self.driver_pid, self.driver_exe)
        if expect_no_overlay: assert '--no-overlay' in self.report['driver_argv'].split(), 'owned daemon started with overlay rendering'
        return meta

    def call(self, name, args, timeout=8):
        return protocol(self.sock, {'method': 'call', 'name': name, 'args': args, 'session_id': 'icu-test-observer', 'client_kind': 'cli'}, timeout)

    def windows(self):
        raw = self.call('list_windows', {}); return raw.get('structuredContent', raw)['windows']

    def tree(self, pid=None):
        return self.ui('tree', pid or self.app_pid)['nodes']

    def find(self, pred, pid=None):
        matches = [n for n in self.tree(pid) if pred(n)]
        return matches[0] if len(matches) == 1 else None

    @staticmethod
    def named(text, role=None, exact=True):
        def pred(n):
            name = n.get('name', '')
            ok = name.casefold() == text.casefold() if exact else text.casefold() in name.casefold()
            return ok and (role is None or n.get('role') == role) and n.get('frame', {}).get('w', 0) > 0
        return pred

    def press(self, text, role='AXButton', exact=True):
        node = wait_for(lambda: self.find(self.named(text, role, exact)), 8, f'{text!r} unavailable')
        return self.ui('press', self.app_pid, json.dumps({'role': role, 'name': node['name'], 'visible': True}))

    def raise_copilot_for_goal(self):
        """Return the user to the verified owned app before clicking its field."""
        owned = self.owned_identities.get('app')
        assert owned and owned[0] == self.app_pid and process_identity(self.app_pid) == owned[1], 'Copilot process identity changed before command setup'
        assert pathlib.Path(self.ui('pidpath', self.app_pid)['path']).resolve() == self.app_exe.resolve(), 'Copilot executable changed before command setup'
        window = wait_for(lambda: self.find(self.named('Copilot', 'AXWindow')), 5, 'Copilot window unavailable')
        frame = window['frame']
        # The owned guard covers the lower command pane after a task. The
        # exposed title bar is the existing user activation route; never type
        # unless the independent foreground check proves it reached Copilot.
        self.ui('click', frame['x'] + frame['w'] / 2, frame['y'] + 12)
        state = wait_for(lambda: (lambda current: current if current.get('front_pid') == self.app_pid else None)(self.ui('state')),
                         5, 'Copilot did not take foreground for user setup')
        return {'pid': self.app_pid, 'path': str(self.app_exe.resolve()), 'window': window.get('name'),
                'front_pid': state['front_pid'], 'time': time.time()}

    def enter_goal(self, goal):
        """Type as a user and prove the entire goal reached the focused field."""
        handoff = self.raise_copilot_for_goal()
        entry = lambda: self.find(lambda n: n.get('role') == 'AXTextField' and 'e.g. in @Calculator' in n.get('placeholder', ''))
        field = wait_for(entry, 10, 'command input unavailable')
        self.ui('click', *center(field))
        wait_for(lambda: self.ui('state')['front_pid'] == self.app_pid, 5, 'Copilot did not take foreground for user setup')
        try: wait_for(lambda: (entry() or {}).get('focused'), 1, 'command input not focused')
        except RuntimeError: self.ui('click', *center(entry()))
        def focused():
            state = self.ui('state')
            return ((entry() or {}).get('focused') and state.get('front_pid') == self.app_pid
                    and state.get('focused_element', {}).get('pid') == self.app_pid)
        wait_for(focused, 5, 'command input did not take focus in the owned Copilot app')
        self.ui('key', 0, 'cmd'); self.ui('type', goal)
        wait_for(lambda: (entry() or {}).get('value') == goal, 5, 'full goal did not reach command input')
        return handoff

    def chat_text(self, fragment):
        return any(fragment in n.get('name', '') and n.get('role') in ('AXStaticText', 'AXGroup') for n in self.tree())

    def pick_window(self, title):
        self.press('+ window')
        wait_for(lambda: self.find(lambda n: n.get('role') == 'AXButton' and title in n.get('name', '') and n.get('frame', {}).get('w', 0) > 0), 12, title + ' missing from window picker')
        self.ui('press', self.app_pid, json.dumps({'role': 'AXButton', 'contains': title, 'visible': True}))
        wait_for(lambda: self.find(lambda n: n.get('role') == 'AXButton' and n.get('name', '').casefold() == 'control'), 8, 'window tile did not appear')
        self.approve_capture_prompt()

    def approve_capture_prompt(self, seconds=3, prompt=CAPTURE_PROMPT):
        """macOS 15 asks the user to confirm the app's direct screen capture.

        Approving it is a user decision taken during setup, before any
        ownership sampling. Record whether it appeared; approve nothing else.
        `prompt` names the requester; the harness's own `screencapture` runs
        under the ssh session's responsible process and gets its own prompt.
        """
        end = time.monotonic() + seconds
        while time.monotonic() < end:
            owners = {w['owner'] for w in self.ui('onscreen')['windows'] if w['owner_name'] == 'UserNotificationCenter'}
            for pid in owners:
                nodes = self.tree(pid)
                allows = [n for n in nodes if n.get('role') == 'AXButton' and n.get('name') == 'Allow']
                for window in [n for n in nodes if n.get('role') == 'AXWindow']:
                    descendants = [n for n in nodes if n.get('path', '').startswith(window['path'] + '.')]
                    texts = ' '.join(n.get('name', '') for n in descendants if n.get('role') == 'AXStaticText')
                    targets = [n for n in descendants if n.get('role') == 'AXButton' and n.get('name') == 'Allow']
                    if prompt in texts and len(targets) == 1:
                        self.ui('press', pid, json.dumps({'role': 'AXButton', 'name': 'Allow', 'index': allows.index(targets[0])}))
                        self.report.setdefault('capture_consent_prompts', []).append(texts[:200])
                        return True
            time.sleep(.3)
        return False

    def screenshot(self, name):
        subprocess.run(['/usr/sbin/screencapture', '-x', str(self.work / name)], check=False, timeout=20)

    def close(self):
        # Cleanup stages are independent: a failed UI quit must not skip driver,
        # fixture, sampler or log cleanup. Only exact registered identities may
        # receive signals; a reused or unverified PID is a visible failure.
        errors = self.report.setdefault('cleanup_errors', [])
        def failed(stage, error):
            errors.append({'stage': stage, 'error': str(error)})
            self.report['ok'] = False
        def owned_alive(kind):
            saved = self.owned_identities.get(kind)
            if saved is None:
                pid = getattr(self, kind + '_pid')
                if pid and alive(pid): raise RuntimeError(f'{kind} PID has no verified ownership record')
                return False
            pid, identity = saved
            if not alive(pid): return False
            current = process_identity(pid)
            if current is None and not alive(pid): return False
            if current != identity: raise RuntimeError(f'{kind} PID identity changed; refusing to signal it')
            return True
        def signal_owned(kind, sig):
            if owned_alive(kind): os.kill(self.owned_identities[kind][0], sig)
        with cleanup_signals():
            # Startup may fail after the app launches but before driver metadata
            # is registered. Recover only through that app's private socket and
            # verify executable plus direct parent before retaining its PID.
            if self.app_pid and self.sock and not self.driver_pid:
                try:
                    if owned_alive('app') and pathlib.Path(self.sock).exists():
                        metadata = protocol(self.sock, {'method': 'metadata'}, 2)
                        pid = metadata['pid']
                        parent = int(subprocess.check_output(['/bin/ps', '-o', 'ppid=', '-p', str(pid)], text=True, timeout=3).strip())
                        assert parent == self.app_pid, 'unregistered daemon is not a child of the owned app'
                        self.remember_owned_identity('driver', pid, self.driver_exe)
                        self.driver_pid = pid
                except Exception as error: failed('driver_registration', error)
            try:
                if owned_alive('app'):
                    try: self.ui('quit', self.app_pid, check=False)
                    except Exception as error: failed('app_quit', error)
                    try: wait_for(lambda: not owned_alive('app'), 8, 'app did not quit')
                    except RuntimeError:
                        signal_owned('app', signal.SIGTERM)
                        try: wait_for(lambda: not owned_alive('app'), 3, 'app did not terminate')
                        except RuntimeError:
                            signal_owned('app', signal.SIGKILL); self.report['app_forced'] = True
                            wait_for(lambda: not owned_alive('app'), 3, 'app survived forced cleanup')
                self.report['owned_app_exited'] = not owned_alive('app')
            except Exception as error:
                self.report['owned_app_exited'] = False; failed('app', error)
            try:
                if owned_alive('driver'):
                    try: wait_for(lambda: not owned_alive('driver'), 5, 'driver did not exit')
                    except RuntimeError:
                        signal_owned('driver', signal.SIGTERM)
                        try: wait_for(lambda: not owned_alive('driver'), 3, 'driver did not terminate')
                        except RuntimeError:
                            signal_owned('driver', signal.SIGKILL)
                            wait_for(lambda: not owned_alive('driver'), 3, 'driver survived forced cleanup')
                        failed('driver', 'owned daemon survived app quit')
                self.report['owned_daemon_exited'] = not owned_alive('driver')
            except Exception as error:
                self.report['owned_daemon_exited'] = False; failed('driver', error)
            for process in self.processes:
                try: stop_child(process)
                except Exception as error: failed(f'child_{process.pid}', error)
            self.report['owned_processes_reaped'] = all(process.poll() is not None for process in self.processes)
            for log in self.logs:
                try: log.close()
                except Exception as error: failed('log', error)
            if errors: self.report.setdefault('error', 'probe cleanup was incomplete or required recovery')
