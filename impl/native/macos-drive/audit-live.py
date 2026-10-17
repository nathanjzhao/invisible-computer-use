#!/usr/bin/env python3
"""One-shot, opt-in acceptance. --prepare only builds the owned fixture; never opens an app.

The fixture and driver are new child processes. Never reuse/kill a user's app or daemon.
Raw RPC, source state, hashes, app events and bounded-region PNGs stay in the run directory.
No cloud, model, recurring schedule, or default live execution.
"""
import argparse, base64, hashlib, json, os, selectors, signal, subprocess, sys, threading, time
from pathlib import Path
from datetime import datetime, timezone
ROOT = Path(__file__).resolve().parents[3]
HERE = Path(__file__).resolve().parent
DRIVER = ROOT / 'vendor-cua/libs/cua-driver/rust/target/release/cua-driver'
ICU = ROOT / 'impl/target/debug/icu'
BUILD = ROOT / 'study/audit/fixes/live-prepared'
APP = BUILD / 'ICUAuditFixture.app'
EXE = APP / 'Contents/MacOS/ICUAuditFixture'
PROBE = BUILD / 'AuditObserver'
PLAN = '''Bounded macOS acceptance (maximum 180 seconds plus up to 60 seconds cleanup):
- Open only ICUAuditFixture, a throwaway local Cocoa app with a Play/Pause button,
  editable scrollable text, and a two-item context menu. No Calculator/TextEdit.
- Start the freshly built driver on a unique private socket; preserve existing daemons.
  Restore any focus change caused by our own AppKit startup before testing actions.
- Exercise AX click, pixel click, indexed middle click, right/double click, scroll,
  drag, and selection readback, serially. Record attached/detached/off click samples.
- Capture only the fixture rectangle while actions run to inspect the ghost cursor;
  save each returned window/menu PNG, exact logical geometry and decoded dimensions.
- Compare pointer, front app and WindowServer front PSN after actions. Save focused
  WindowServer logs. These checks supplement a human physical-keyboard check.
- Dismiss the test menu, end test sessions, stop only test child processes, and
  restore the saved front app and pointer on success, failure, timeout or Ctrl-C.
- Output remains live-unverified until the run, bitmap inspection, visible-cursor
  inspection and physical-keyboard confirmation have actually happened.'''


def run(argv, **kw):
    return subprocess.run(list(map(str, argv)), capture_output=True, text=True, timeout=kw.pop('timeout', 10), **kw)


def prepare():
    (EXE.parent).mkdir(parents=True, exist_ok=True)
    import plistlib
    before_build = current_build_manifest(include_binaries=False)
    (APP/'Contents/Info.plist').write_bytes(plistlib.dumps({'CFBundleExecutable': EXE.name,
        'CFBundleIdentifier': 'local.icu.audit-fixture', 'CFBundleName': 'ICU Audit Fixture', 'CFBundlePackageType': 'APPL', 'LSUIElement': True, 'NSPrincipalClass':'AuditApplication'}))
    for label, command in [
        ('workspace', ['cargo','build','--locked','--manifest-path',ROOT/'impl/Cargo.toml','--target-dir',ROOT/'impl/target']),
        ('native', ['cargo','build','--locked','--manifest-path',ROOT/'vendor-cua/libs/cua-driver/rust/Cargo.toml','--target-dir',ROOT/'vendor-cua/libs/cua-driver/rust/target','--release','-p','cua-driver'])]:
        build_cwd = ROOT if label == 'workspace' else ROOT/'vendor-cua/libs/cua-driver/rust'
        built = run(command, timeout=180, cwd=build_cwd)
        (BUILD/f'{label}-build.txt').write_text(built.stdout+built.stderr)
        if built.returncode: raise RuntimeError(f'{label} build failed: {built.stderr}')
    patch = (ROOT/'impl/native/cua-driver.patch').read_bytes()
    source_diff = subprocess.check_output(['git','-C',str(ROOT/'vendor-cua'),'diff','--','libs/cua-driver/rust/crates'])
    if patch != source_diff: raise RuntimeError('native patch differs from prepared vendor source')
    p = run(['swiftc', HERE/'audit-fixture.swift' , '-o', EXE, '-framework', 'Cocoa', '-framework', 'CoreGraphics'], timeout=60)
    (BUILD/'build.txt').write_text(p.stdout+p.stderr)
    if p.returncode: raise RuntimeError(p.stderr)
    probe = run(['swiftc',HERE/'audit-fixture.swift','-o',PROBE,'-framework','Cocoa','-framework','CoreGraphics'],timeout=60)
    (BUILD/'observer-build.txt').write_text(probe.stdout+probe.stderr)
    if probe.returncode: raise RuntimeError(probe.stderr)
    (BUILD/'PLAN.txt').write_text(PLAN+'\n')
    after_build = current_build_manifest()
    for key in ('source_sha256','vendor_source_sha256','vendor_revision','vendor_diff_sha256'):
        if before_build[key] != after_build[key]:
            raise RuntimeError('source changed during preparation; rebuild before desktop acceptance')
    (BUILD/'build-manifest.json').write_text(json.dumps(after_build,indent=2)+'\n')
    print(PLAN)
    print('Prepared fixture:', EXE)
    print('No app was launched and no input was injected.')


def current_build_manifest(include_binaries=True):
    binaries = [DRIVER, ICU, EXE, PROBE] if include_binaries else []
    p = run([sys.executable, ROOT/'impl/verification_manifest.py', *binaries])
    if p.returncode: raise RuntimeError(f'preparation manifest failed: {p.stderr}')
    return json.loads(p.stdout)


def validate_prepared_build():
    path = BUILD/'build-manifest.json'
    if not path.is_file(): raise RuntimeError('live preparation missing; run --prepare before desktop acceptance')
    expected = json.loads(path.read_text())
    current = current_build_manifest()
    for key in ('source_sha256','vendor_source_sha256','vendor_revision','vendor_diff_sha256','binaries'):
        if key not in expected or key not in current or expected[key] != current[key]:
            raise RuntimeError(f'stale live preparation ({key}); rerun --prepare; no desktop access attempted')
    return current


class RPC:
    def __init__(self, argv, env, directory, label):
        self.directory, self.label, self.seq = directory, label, 0
        self.log = open(directory/f'{label}-stderr.txt', 'w')
        self.proc = subprocess.Popen(list(map(str, argv)), env=env, stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=self.log)
        self.selector = selectors.DefaultSelector(); self.selector.register(self.proc.stdout, selectors.EVENT_READ)
        self.buffer = b''
        try:
            self.request('initialize', {'protocolVersion':'2024-11-05', 'capabilities':{}, 'clientInfo':{'name':'bounded-audit','version':'1'}})
            self.proc.stdin.write(b'{"jsonrpc":"2.0","method":"notifications/initialized"}\n'); self.proc.stdin.flush()
        except BaseException:
            self.close()
            raise
    def request(self, method, params):
        self.seq += 1
        req = dict(jsonrpc='2.0', id=self.seq, method=method, params=params)
        start = time.monotonic(); deadline = start + 12
        self.proc.stdin.write(json.dumps(req).encode()+b'\n'); self.proc.stdin.flush()
        while time.monotonic() < deadline:
            if b'\n' in self.buffer:
                line, self.buffer = self.buffer.split(b'\n', 1)
                try: reply = json.loads(line)
                except ValueError: continue
                if reply.get('id') != self.seq: continue
                (self.directory/f'{self.label}-{self.seq:03}.json').write_text(json.dumps({
                    'request':req, 'response':reply, 'elapsed_seconds':time.monotonic()-start}, indent=2))
                if 'error' in reply: raise RuntimeError(reply)
                return reply['result']
            if self.selector.select(max(0, deadline-time.monotonic())):
                chunk = os.read(self.proc.stdout.fileno(), 65536)
                if not chunk: raise RuntimeError(f'{self.label} transport EOF')
                self.buffer += chunk
        raise TimeoutError(f'{self.label} {method}')
    def call(self, tool, args):
        r = self.request('tools/call', {'name':tool, 'arguments':args})
        if r.get('isError'): raise RuntimeError(r)
        payload = r.get('structuredContent')
        if payload is None:
            payload = json.loads(next(c['text'] for c in r.get('content',[]) if c.get('type')=='text'))
        return payload
    def close(self):
        if self.proc.poll() is None:
            self.proc.terminate()
            try: self.proc.wait(timeout=3)
            except subprocess.TimeoutExpired: self.proc.kill(); self.proc.wait(timeout=2)
        self.log.close(); self.selector.close()


def snapshot():
    p = run([PROBE, 'state']); assert p.returncode == 0, p.stderr
    return json.loads(p.stdout)


class UserIntervened(RuntimeError):
    """A person changed the desktop during setup; cleanup must keep their choice."""


def check_setup_intervention(before, owned_pids, observed):
    if 'hid_input_counts' in before and observed.get('hid_input_counts') != before['hid_input_counts']:
        raise UserIntervened('physical input changed or became unreadable during setup; refusing further input')
    if observed['pointer'] != before['pointer']:
        raise UserIntervened('pointer moved during setup; refusing further input')
    if observed['front_pid'] != before['front_pid'] and observed['front_pid'] not in owned_pids:
        raise UserIntervened('unowned app became foreground during setup; refusing further input')


def require_original_setup_owner(before, owned_pids, observed):
    check_setup_intervention(before, owned_pids, observed)
    if any(observed[k] != before[k] for k in ('front_pid','pointer','windowserver_front_psn')):
        raise RuntimeError('setup did not retain original input ownership; refusing actions')


def restore_owned_startup(before, owned_pids, read_state, restore_focus):
    """AppKit finishLaunching may activate a raw daemon/fixture. Restore only
    our own setup transition; never rebase the action baseline or fight the user.
    """
    observed = read_state()
    if 'hid_input_counts' in before and observed.get('hid_input_counts') != before['hid_input_counts']:
        raise UserIntervened('physical input changed or became unreadable during setup; refusing further input')
    if observed['pointer'] != before['pointer']:
        raise UserIntervened('pointer moved during setup; refusing further input')
    if observed['front_pid'] != before['front_pid']:
        if observed['front_pid'] not in owned_pids:
            raise UserIntervened('unowned app became foreground during setup; refusing further input')
        restore_focus(before['front_pid'])
    after = read_state()
    check_setup_intervention(before, owned_pids, after)
    if any(after[k] != before[k] for k in ('front_pid','pointer','windowserver_front_psn')):
        raise RuntimeError('startup did not restore original input ownership; refusing actions')
    return {'observed': observed, 'restored': after}


def wait_for_actionable_fixture(before, owned_pids, observe, read_state, pause):
    button = None
    for attempt in range(8):
        require_original_setup_owner(before, owned_pids, read_state())
        try:
            doc = observe(f'initial-{attempt}')
        finally:
            require_original_setup_owner(before, owned_pids, read_state())
        button = next((e for e in doc.get('elements', []) if e.get('identifier') == 'audit-play-control'), None)
        if button is not None: return doc, button
        if attempt < 7: pause(.4)
    raise RuntimeError(f'fixture AX window did not become actionable within bounded readiness wait: {doc.get("degraded_reason")}')


class OwnershipFailure(RuntimeError):
    def __init__(self, report):
        self.report = report
        super().__init__(json.dumps(report, sort_keys=True))


def ownership_report(before, owned_pids, observed):
    """A changed pointer/unowned front is ambiguous, not proof the user did it.
    A known owned foreground or changed WS chain remains a visible regression.
    Either case stops input and forbids overwriting the changed desktop.
    """
    if observed is None:
        return {'classification':'ownership_unreadable', 'observed':None, 'baseline':before}
    if 'hid_input_counts' in before and observed.get('hid_input_counts') != before['hid_input_counts']:
        return {'classification':'ownership_uncertain', 'observed':observed, 'baseline':before}
    if all(observed[k] == before[k] for k in ('front_pid','pointer','windowserver_front_psn')):
        return None
    classification = 'ownership_uncertain'
    if observed['pointer'] == before['pointer'] and (
        observed['front_pid'] in owned_pids or observed['front_pid'] == before['front_pid']
    ):
        classification = 'owned_process_regression'
    return {'classification':classification, 'observed':observed, 'baseline':before}


def require_action_owner(before, owned_pids, observed):
    report = ownership_report(before, owned_pids, observed)
    if report: raise OwnershipFailure(report)


def require_click_sequence(events, count):
    observed=[e.get('click_count') for e in events if e.get('kind')=='left_down']
    assert observed==list(range(1,count+1)), observed
    assert sum(e.get('kind')=='text_mouse_down_returned' for e in events)==count, events

def require_menu_sequence(events):
    observed=[e['kind'] for e in events if e.get('kind') in ('right_down','menu_closed')]
    assert observed==['right_down','menu_closed'], observed

def require_middle_pair(events):
    pair = [e['kind'] for e in events if e.get('kind') in ('middle_button_down','middle_button_up')]
    assert pair == ['middle_button_down','middle_button_up'], events


def require_scroll_effect(before, after, events, wid, ticks):
    wheels=[e for e in events if e.get('kind')=='input_event' and e.get('type')==22]
    assert len(wheels)==ticks, wheels
    assert all(e.get('window_id')==wid for e in wheels), wheels
    assert all(e.get('delta_y',0)<0 for e in wheels), wheels
    assert sum(e['delta_y'] for e in wheels)==-ticks, wheels
    assert before['window_id']==after['window_id']==wid, (before,after)
    assert after['scroll_y'] > before['scroll_y'], (before,after)


def save_surfaces(out, tag, state):
    from PIL import Image
    shots = state.get('screenshots', [])
    for s in shots:
        data = base64.b64decode(s.get('png_b64') or s['url'].split(',',1)[1], validate=True)
        path = out/f'{tag}-{s["id"]}.png'; path.write_bytes(data)
        with Image.open(path) as im:
            im.load(); w, h = im.size
        assert s.get('width',0) > 0 and s.get('height',0) > 0, s
        assert max((w-.5)/s['width'],(h-.5)/s['height']) <= min((w+.5)/s['width'],(h+.5)/s['height'])+1e-12, (s,w,h)
        if 'pixel_width' in s: assert (w,h)==(s['pixel_width'],s['pixel_height'])
    return shots


def live(diagnose_only=False):
    if os.environ.get('ICU_BOUNDED_TEST') != '1':
        raise SystemExit('REFUSED: user go-ahead required; then ICU_BOUNDED_TEST=1 and --run. Use --prepare first.')
    for binary in [DRIVER, ICU, EXE, PROBE]:
        if not binary.is_file(): raise SystemExit(f'Required fresh/prepared binary missing: {binary}')
    prepared = validate_prepared_build()
    import PIL.Image  # validate harness dependency before opening an app
    out = ROOT/'study/audit/fixes'/('live-'+datetime.now(timezone.utc).strftime('%Y%m%dT%H%M%SZ'))
    out.mkdir()
    (out/'manifest.json').write_text(json.dumps(prepared,indent=2))
    before = snapshot(); (out/'before.json').write_text(json.dumps(before,indent=2))
    if before['windowserver_front_psn'] is None: raise RuntimeError('WindowServer focus probe unavailable; no input injected')
    env = {k:v for k,v in os.environ.items() if not k.startswith(('ICU_','CUA_'))}
    socket = f'/tmp/icu-audit-{os.getpid()}.sock'
    env.update(CUA_DRIVER_EMBEDDED='1', ICU_DRIVER=str(DRIVER), ICU_SOCKET=socket,
        ICU_HOME=str(out/'home'), ICU_SESSION=f'audit-{os.getpid()}', ICU_REQUIRE_APP_APPROVAL='0')
    daemon=fixture=native=icu=trace=None
    user_intervened = False
    preserve_changed = False
    ownership_failures = []
    acceptance_failures = []
    completed_checks = []
    actions_started = False
    stop = threading.Event(); recorder=None; cleanup=[]
    def expired(*_): raise TimeoutError('180-second harness wall deadline')
    prior_alarm=signal.signal(signal.SIGALRM, expired); signal.alarm(180)
    daemon_log=open(out/'daemon.txt','w')
    try:
        daemon=subprocess.Popen([str(DRIVER),'serve','--socket',socket,'--no-permissions-gate'],env=env,stdout=daemon_log,stderr=subprocess.STDOUT)
        for _ in range(30):
            if Path(socket).exists(): break
            if daemon.poll() is not None: raise RuntimeError('test daemon exited')
            time.sleep(.2)
        native=RPC([DRIVER,'--socket',socket,'mcp'],env,out,'native')
        health=native.call('health_report',{})
        host=next((c.get('data',{}).get('host',{}) for c in health.get('checks',[]) if c.get('name')=='host_interactive'),{})
        if host.get('screen_locked') is not False or host.get('display_asleep') is True:
            raise RuntimeError(f'BLOCKED: host is not ready: {host}')
        def settle_owned_setup(tag, owned_pids):
            report = restore_owned_startup(before, owned_pids, snapshot,
                lambda pid: run([PROBE,'restore-focus',pid]))
            (out/f'setup-{tag}.json').write_text(json.dumps(report,indent=2))
        settle_owned_setup('daemon', {daemon.pid})
        fixture=subprocess.Popen([str(EXE),'fixture',str(out)],stdout=open(out/'fixture-stdout.txt','w'),stderr=subprocess.STDOUT)
        for _ in range(40):
            if (out/'fixture-ready.json').exists(): break
            time.sleep(.1)
        ready=json.loads((out/'fixture-ready.json').read_text()); assert ready['pid']==fixture.pid
        time.sleep(.2)  # finishLaunching follows the fixture's ready notification
        settle_owned_setup('fixture', {daemon.pid, fixture.pid})
        pid,wid=ready['pid'],ready['window_id']; b=ready['bounds']
        independent = run([PROBE,'ax-observe',pid,wid])
        (out/'independent-ax-observer.json').write_text(independent.stdout)
        (out/'independent-ax-observer-stderr.txt').write_text(independent.stderr)
        if diagnose_only:
            stages = []
            def diagnostic_stage(label):
                owner = snapshot()
                require_action_owner(before, {daemon.pid,fixture.pid}, owner)
                result = native.call('get_window_state',{'pid':pid,'window_id':wid,'include_screenshot':True,'include_text':True})
                save_surfaces(out,label,result)
                observer = run([PROBE,'ax-observe',pid,wid])
                (out/f'{label}-independent.json').write_text(observer.stdout)
                placement = native.call('list_windows',{'pid':pid})
                stages.append({'stage':label,'placement':placement,'ax_diagnostics':result.get('ax_diagnostics'),
                               'element_count':result.get('element_count'),'degraded_reason':result.get('degraded_reason')})
            diagnostic_stage('before-client-setup')
            icu=RPC([ICU,'mcp'],env,out,'icu')
            diagnostic_stage('after-icu-client')
            sid=f'audit-diagnose-{os.getpid()}'
            native.call('start_session',{'session':sid})
            native.call('set_agent_cursor_enabled',{'session':sid,'enabled':True,'detached':True})
            diagnostic_stage('after-cursor-session')
            diagnostic_stage('final-independent-comparison')
            (out/'RESULT.json').write_text(json.dumps({'automated_checks':'diagnosis_only_no_input',
                'stages':stages, 'physical_keyboard_confirmation':'not applicable: no input dispatched'},indent=2))
            return

        if os.environ.get('ICU_AUDIT_WHEEL_TAP')=='1':
            trace=subprocess.Popen([str(PROBE),'trace-wheel',str(pid),str(out/'pid-wheel-tap.jsonl')],stdout=open(out/'tap-stdout.txt','w'),stderr=subprocess.STDOUT)
            for _ in range(20):
                tf=out/'pid-wheel-tap.jsonl'
                if tf.exists() and '"ready":true' in tf.read_text(): break
                if trace.poll() is not None: raise RuntimeError('wheel tap failed')
                time.sleep(.05)
            assert '"ready":true' in tf.read_text(), 'tap did not become ready'
        rect=','.join(str(int(b[k])) for k in ('X','Y','Width','Height'))
        def record_frames():
            for i in range(90):
                if stop.is_set(): break
                try: run(['/usr/sbin/screencapture','-x','-R',rect,out/f'cursor-frame-{i:03}.png'],timeout=3)
                except Exception as e: cleanup.append(f'frame capture: {e}'); break
                if stop.wait(.15): break
        recorder=threading.Thread(target=record_frames,daemon=True); recorder.start()
        # Separate persistent layer and raw driver clients, each owning its session.
        icu=RPC([ICU,'mcp'],env,out,'icu')
        session=f'audit-native-{os.getpid()}'
        native.call('start_session',{'session':session})
        native.call('set_agent_cursor_enabled',{'session':session,'enabled':True,'detached':True})
        native.call('set_agent_cursor_motion',{'session':session,'glide_duration_ms':200,'idle_hide_ms':1500})
        win={'app':'local.icu.audit-fixture','id':wid}
        def check_owner(tag):
            try: now = snapshot()
            except Exception: now = None
            (out/f'ownership-{tag}.json').write_text(json.dumps(now,indent=2))
            require_action_owner(before, {daemon.pid,fixture.pid}, now)
        def action(tool, args, tag):
            nonlocal actions_started
            check_owner(f'before-{tag}')
            actions_started = True
            result = native.call(tool,args)
            check_owner(f'after-{tag}')
            return result
        check_owner('fixture-start')
        latest_doc = None
        def observe(tag):
            nonlocal latest_doc
            doc=native.call('get_window_state',{'pid':pid,'window_id':wid,'include_screenshot':True,'include_text':True})
            save_surfaces(out,tag,doc)
            latest_doc=doc
            return doc
        doc, button = wait_for_actionable_fixture(before, {daemon.pid, fixture.pid}, observe, snapshot, time.sleep)
        args={'pid':pid,'window_id':wid,'session':session,'element_token':button['element_token']}
        # The action's own observable fixture event must exist before its response is accepted.
        count=0
        if os.environ.get('ICU_AUDIT_DRAG_ONLY')=='1':
            scale=doc['screenshot_scale']
            prior=json.loads((out/'fixture-state.json').read_text())
            action('drag',{'pid':pid,'window_id':wid,'session':session,'from_x':200*scale,'from_y':200*scale,'to_x':260*scale,'to_y':200*scale,'duration_ms':160},'drag-diagnostic')
            after=json.loads((out/'fixture-state.json').read_text())
            (out/'drag-readback.json').write_text(json.dumps({'before':prior,'after':after},indent=2))
            observe('after-drag')
            assert after['selected_length']>0 and after['selected_text']!=prior['selected_text'], (prior,after)
            (out/'RESULT.json').write_text(json.dumps({'automated_checks':'drag_diagnostic_only_passed','acceptance':'partial; full batch required'},indent=2))
            return
        if os.environ.get('ICU_AUDIT_ACTIVE_CONTROL')=='1':
            check_owner('before-active-control')
            try:
                activated=run([PROBE,'restore-focus',pid])
                assert activated.returncode==0, activated.stderr
                owner=snapshot()
                assert owner['front_pid']==pid and owner['pointer']==before['pointer'], owner
                prior=json.loads((out/'fixture-state.json').read_text())
                sent=run([PROBE,'wheel-probe',pid,wid,200,200,'nil-primer-tagged-line'])
                assert sent.returncode==0, sent.stderr
                observe('active-control')
                after=json.loads((out/'fixture-state.json').read_text())
                (out/'active-control.json').write_text(json.dumps({'before':prior,'after':after,'sent':json.loads(sent.stdout)},indent=2))
            finally:
                restore_owned_startup(before,{pid},snapshot,lambda p:run([PROBE,'restore-focus',p]))
            (out/'RESULT.json').write_text(json.dumps({'automated_checks':'foreground_control_only','acceptance':'not background acceptance'},indent=2))
            return
        if os.environ.get('ICU_AUDIT_WHEEL_MATRIX')=='1':
            samples=[]
            for variant in ['nil-primer-pixel','nil-primer-tagged-pixel','nil-primer-tagged-group-pixel']:
                check_owner(f'before-{variant}')
                prior=json.loads((out/'fixture-state.json').read_text())
                sent=run([PROBE,'wheel-probe',pid,wid,200,200,variant])
                assert sent.returncode==0, sent.stderr
                check_owner(f'after-{variant}')
                observe(variant)
                after=json.loads((out/'fixture-state.json').read_text())
                samples.append({'variant':variant,'sent':json.loads(sent.stdout),'before':prior,'after':after})
            (out/'wheel-matrix.json').write_text(json.dumps(samples,indent=2))
            (out/'RESULT.json').write_text(json.dumps({'automated_checks':'independent_wheel_diagnosis_only','acceptance':'not product-path acceptance'},indent=2))
            return
        if os.environ.get('ICU_AUDIT_SCROLL_ONLY')=='1':
            scale=doc['screenshot_scale']
            before_scroll=json.loads((out/'fixture-state.json').read_text())
            start_events=len((out/'fixture-events.jsonl').read_text().splitlines())
            action('scroll',{'pid':pid,'window_id':wid,'session':session,'x':200*scale,'y':200*scale,'direction':'down','amount':2, 'delivery_mode':'foreground' if os.environ.get('ICU_AUDIT_SCROLL_FOREGROUND_CONTROL')=='1' else 'background'},'scroll-diagnostic')
            after_scroll=json.loads((out/'fixture-state.json').read_text())
            received=[json.loads(l) for l in (out/'fixture-events.jsonl').read_text().splitlines()[start_events:]]
            (out/'scroll-readback.json').write_text(json.dumps({'before':before_scroll,'after':after_scroll,'events':received},indent=2))
            observe('after-scroll')
            require_scroll_effect(before_scroll,after_scroll,received,wid,2)
            (out/'RESULT.json').write_text(json.dumps({'automated_checks':'scroll_diagnostic_only_passed','acceptance':'partial; full batch still required'},indent=2))
            return
        if os.environ.get('ICU_AUDIT_MIDDLE_ONLY')=='1':
            action('click',dict(args,button='middle'),'middle-diagnostic')
            (out/'RESULT.json').write_text(json.dumps({'automated_checks':'middle_diagnostic_only', 'acceptance':'partial; inspect raw events'},indent=2))
            return
        for mode in ('attached','detached','off'):
            native.call('set_agent_cursor_enabled',{'session':session,'enabled':mode!='off','detached':mode=='detached'})
            for i in range(3):
                action('click',args,f'{mode}-{i}'); count+=1
                events=[json.loads(l) for l in (out/'fixture-events.jsonl').read_text().splitlines()]
                assert sum(e['kind']=='button_action' for e in events)==count, 'response preceded actual button completion'
                check_owner(f'{mode}-{i}')
            completed_checks.append(f'ax-{mode}')
        native.call('set_agent_cursor_enabled',{'session':session,'enabled':True,'detached':True})
        # Coordinates are raw driver screenshot pixels; infer scale from the actual frame.
        scale=doc['screenshot_scale']; x,y=200*scale,200*scale
        common={'pid':pid,'window_id':wid,'session':session}
        for tag,tool,payload in [
            ('pixel','click',{'x':x,'y':y}),
            ('middle','click',dict(element_token=button['element_token'],button='middle')),
            ('double','double_click',{'x':x,'y':y}),
            ('scroll','scroll',{'x':x,'y':y,'direction':'down','amount':2}),
            ('drag','drag',{'from_x':x,'from_y':y,'to_x':x+60*scale,'to_y':y,'duration_ms':160}),
            ('right','right_click',{'x':x,'y':y})]:
            prior_events = len((out/'fixture-events.jsonl').read_text().splitlines())
            prior_state=json.loads((out/'fixture-state.json').read_text())
            if tag=='scroll': observe('before-scroll')
            action(tool,dict(common,**payload),tag)
            observed = [json.loads(l) for l in (out/'fixture-events.jsonl').read_text().splitlines()[prior_events:]]
            if tag == 'middle': require_middle_pair(observed)
            else:
                expected_type = {'pixel':1,'double':1,'scroll':22,'drag':6,'right':3}[tag]
                try:
                    assert any(e['kind'] in ('input_event','tracking_event') and e.get('type')==expected_type and e.get('window_id')==wid for e in observed), (tag,observed)
                except AssertionError as failure:
                    if tag != 'scroll': raise
                    # Independent remaining routes still run, but this required
                    # guard remains FAILED and the run must exit nonzero.
                    acceptance_failures.append({'route':tag,'error':repr(failure),'events':observed})
                if tag in ('pixel','double'): require_click_sequence(observed,1 if tag=='pixel' else 2)
            check_owner(tag)
            after_state=json.loads((out/'fixture-state.json').read_text())
            if tag=='scroll':
                (out/'scroll-readback.json').write_text(json.dumps({'before':prior_state,'after':after_state,'events':observed},indent=2))
                try: require_scroll_effect(prior_state,after_state,observed,wid,2)
                except AssertionError as failure: acceptance_failures.append({'route':tag,'error':repr(failure)})
                else: completed_checks.append('scroll')
                observe('after-scroll')
            elif tag=='drag':
                assert after_state['selected_length']>0 and (after_state['selected_location'],after_state['selected_length']) != (prior_state['selected_location'],prior_state['selected_length']), (prior_state,after_state)
                (out/'drag-readback.json').write_text(json.dumps({'before':prior_state,'after':after_state,'events':observed},indent=2))
                observe('after-drag'); completed_checks.append('drag')
            elif tag!='right': completed_checks.append(tag)
            if tag=='right':
                for round_index in range(3):
                    if round_index:
                        prior_events=len((out/'fixture-events.jsonl').read_text().splitlines())
                        action('right_click',dict(common,x=x,y=y),f'menu-{round_index}')
                    menu=observe(f'menu-{round_index}-immediate')
                    assert len(menu.get('screenshots',[]))>=2, 'no transient immediately after opening'
                    menu_events=[json.loads(l) for l in (out/'fixture-events.jsonl').read_text().splitlines()[prior_events:]]
                    surfaces=[s for e in menu_events if e.get('kind')=='menu_surface_bounds' for s in e['surfaces']]
                    assert surfaces, 'no independent menu bounds from fixture'
                    for shot in menu['screenshots'][1:]:
                        assert any(abs(shot['originX']-s['kCGWindowBounds']['X'])<=1 and abs(shot['originY']-s['kCGWindowBounds']['Y'])<=1 and abs(shot['width']-s['kCGWindowBounds']['Width'])<=1 and abs(shot['height']-s['kCGWindowBounds']['Height'])<=1 for s in surfaces), (shot['id'],surfaces)
                    if round_index==2:
                        mapped_menu=icu.call('get_window_state',{'window':win,'include_screenshot':True,'include_text':True})
                        save_surfaces(out,'window2-menu',mapped_menu)
                        assert len(mapped_menu.get('screenshots',[]))>=2
                        completed_checks.append('window2-menu')
                    action('press_key',dict(common,key='escape'),f'dismiss-menu-{round_index}')
                    closed_events=[json.loads(l) for l in (out/'fixture-events.jsonl').read_text().splitlines()[prior_events:]]
                    require_menu_sequence(closed_events)
                    completed_checks.append(f'menu-{round_index}')
                completed_checks.append('right')
        fresh=observe('before-selection')
        field=next(e for e in fresh['elements'] if e.get('identifier')=='audit-text')
        action('select_text',dict(common,element_token=field['element_token'],text='quick'),'selection')
        selected=observe('selection')
        assert selected.get('selected_text')=='quick', selected
        assert json.loads((out/'fixture-state.json').read_text())['selected_text']=='quick'
        completed_checks.append('selection')
        mapped=icu.call('get_window_state',{'window':win,'include_screenshot':True,'include_text':True})
        save_surfaces(out,'window2',mapped); check_owner('complete')
        assert mapped['accessibility']['selected_text']=='quick'
        completed_checks.append('window2')
        required={'ax-attached','ax-detached','ax-off','pixel','middle','double','scroll','drag','right','menu-0','menu-1','menu-2','window2-menu','selection','window2'}
        assert set(completed_checks)==required, (completed_checks,required)
        (out/'RESULT.json').write_text(json.dumps({'automated_checks':'failed' if acceptance_failures else 'passed','acceptance_failures':acceptance_failures,'completed_checks':completed_checks,'visual_cursor_and_menu_review':'pending',
            'physical_keyboard_confirmation':'pending','note':'No claim of timing equivalence; raw samples retained.'},indent=2))
    except BaseException as error:
        user_intervened = isinstance(error, UserIntervened)
        if isinstance(error, OwnershipFailure): ownership_failures.append(error.report)
        # Any error may coincide with intervention, including RPC and capture failures.
        try: observed = snapshot()
        except BaseException: observed = None
        report = ownership_report(before, {p.pid for p in (daemon,fixture) if p}, observed)
        if report: ownership_failures.append(report)
        preserve_changed = user_intervened or bool(ownership_failures)
        (out/'RESULT.json').write_text(json.dumps({'automated_checks':'failed','error':repr(error),'acceptance_failures':acceptance_failures,'completed_checks':completed_checks,
            'visual_cursor_and_menu_review':'pending','physical_keyboard_confirmation':'pending'},indent=2))
        raise
    finally:
        signal.alarm(0); signal.signal(signal.SIGALRM,prior_alarm)
        stop.set()
        if recorder: recorder.join(timeout=4)
        # Closing each owned MCP transport ends its lifecycle lease; fixture termination
        # releases its menus. Do not attempt new input after a timeout/failure.
        for client in (icu,native):
            if client:
                try: client.close()
                except Exception as e: cleanup.append(str(e))
        for child in (trace,daemon,fixture):
            if child and child.poll() is None:
                child.terminate()
                try: child.wait(timeout=3)
                except subprocess.TimeoutExpired: child.kill(); child.wait(timeout=2)
        daemon_log.close()
        Path(socket).unlink(missing_ok=True)
        # Check again after potentially slow cleanup, immediately before any restoration.
        # If the baseline already matches there is nothing to restore; do not issue
        # an unnecessary activation/warp that could race a user's next choice.
        try: restore_state = snapshot()
        except BaseException: restore_state = None
        report = ownership_report(before, {p.pid for p in (daemon,fixture) if p}, restore_state)
        if report: ownership_failures.append(report)
        preserve_changed = preserve_changed or bool(ownership_failures)
        (out/'restore.txt').write_text('Preserved changed/uncertain desktop; no activation or pointer warp.\n'
            if preserve_changed else 'Original desktop already restored after owned child cleanup; no activation or warp needed.\n')
        try: after = snapshot()
        except BaseException: after = None
        report = ownership_report(before, {p.pid for p in (daemon,fixture) if p}, after)
        if report: ownership_failures.append(report); preserve_changed = True
        (out/'after.json').write_text(json.dumps(after,indent=2))
        restored = after is not None and all(after[k] == before[k] for k in ('front_pid','pointer','windowserver_front_psn'))
        if not restored: cleanup.append('focus/pointer/WindowServer restoration did not match baseline')
        result_path = out/'RESULT.json'
        result = json.loads(result_path.read_text()) if result_path.exists() else {'automated_checks':'failed'}
        result['cleanup_restored'] = restored
        result['cleanup_policy'] = 'preserve_changed_desktop' if preserve_changed else 'baseline_already_restored'
        if ownership_failures:
            result['ownership_failure'] = ownership_failures[0]
            result['ownership_observations'] = ownership_failures
            result['automated_checks'] = 'failed'
        if not restored: result['automated_checks'] = 'failed'
        result_path.write_text(json.dumps(result,indent=2))
        (out/'cleanup.json').write_text(json.dumps(cleanup,indent=2))
        # Narrow WindowServer record; no unrelated application contents.
        if fixture:
            logs=run(['/usr/bin/log','show','--last','4m','--style','compact','--info','--predicate',
                f'process == "WindowServer" AND (eventMessage CONTAINS "{fixture.pid}") AND (eventMessage CONTAINS[c] "keyThief" OR eventMessage CONTAINS[c] "keyFocus" OR eventMessage CONTAINS[c] "keyboardFocus")'])
            (out/'windowserver.txt').write_text(logs.stdout+logs.stderr)
    assert restored, f'cleanup restoration failed; inspect {out}'
    if acceptance_failures: raise RuntimeError(f'required acceptance failed: {acceptance_failures}; evidence: {out}')
    print('Evidence:',out)
    print('Automated run finished; PNG/ghost-cursor review and physical-keyboard confirmation remain required.')


if __name__=='__main__':
    parser=argparse.ArgumentParser(description=__doc__)
    group=parser.add_mutually_exclusive_group(required=True); group.add_argument('--prepare',action='store_true'); group.add_argument('--run',action='store_true'); group.add_argument('--diagnose',action='store_true')
    args=parser.parse_args()
    if args.prepare: prepare()
    else: live(diagnose_only=args.diagnose)
