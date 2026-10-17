"""Synthetic process lifetimes and fixture receipts; no native UI or model calls."""
import json
import os
from pathlib import Path
import signal
import subprocess
import sys
import textwrap
import time
from types import SimpleNamespace

import pytest

import macos_probe as helper
import run_macos_real_model_suite as runner


def wait_path(path, timeout=10):
    end = time.monotonic() + timeout
    while time.monotonic() < end:
        if path.exists(): return
        time.sleep(.02)
    raise AssertionError(f'missing synthetic receipt: {path}')


def test_suite_interrupt_then_probe_term_still_reaps_owned_processes(tmp_path):
    tests = str(Path(__file__).resolve().parent)
    probe = tmp_path / 'synthetic_probe.py'
    probe.write_text(textwrap.dedent('''
        import json, pathlib, signal, subprocess, sys, time
        import macos_probe as h
        work=pathlib.Path(sys.argv[1]); report={'ok':False}
        children=[subprocess.Popen([sys.executable,'-c','import time; time.sleep(60)']) for _ in range(3)]
        app,driver,fixture=children
        s=h.Session.__new__(h.Session)
        s.report=report; s.processes=[fixture]; s.logs=[]; s.sock=None; s.owned_identities={}
        s.app_pid=app.pid; s.driver_pid=driver.pid; s.app_exe=pathlib.Path(sys.executable); s.driver_exe=s.app_exe
        def ui(command,*args,**kwargs):
            if command=='pidpath': return {'path':sys.executable}
            raise RuntimeError('synthetic UI quit failure')
        s.ui=ui
        s.remember_owned_identity('app',app.pid,sys.executable)
        s.remember_owned_identity('driver',driver.pid,sys.executable)
        alive=h.alive
        def reaping_alive(pid):
            for child in children:
                if child.pid==pid and child.poll() is not None: return False
            return alive(pid)
        h.alive=reaping_alive
        wait=h.wait_for
        h.wait_for=lambda pred,seconds,message,interval=.1: wait(pred,min(seconds,.2),message,.01)
        h.install_interrupt_handlers()
        h.write_report(work/'ready.json',{'probe':__import__('os').getpid(),'children':[p.pid for p in children]})
        try: time.sleep(60)
        except (h.ProbeInterrupted,KeyboardInterrupt) as error:
            report['interruption']=type(error).__name__
        finally:
            with h.cleanup_signals():
                (work/'cleanup-started').write_text('ready')
                time.sleep(.4)
                s.close()
                report['exit_codes']=[p.poll() for p in children]
                h.write_report(work/'result.json',report)
        sys.exit(1)
    '''))
    supervisor = tmp_path / 'supervisor.py'
    supervisor.write_text(textwrap.dedent('''
        import pathlib,sys
        from macos_probe import install_interrupt_handlers,write_report
        from run_macos_real_model_suite import run_probe
        work=pathlib.Path(sys.argv[1]); row={}
        install_interrupt_handlers()
        with (work/'probe.log').open('w') as log:
            run_probe([sys.executable,str(work/'synthetic_probe.py'),str(work)],log,row,timeout=30,cleanup_seconds=5)
        write_report(work/'row.json',row)
    '''))
    env = dict(os.environ, PYTHONPATH=tests)
    unrelated = subprocess.Popen([sys.executable, '-c', 'import time; time.sleep(60)'])
    parent = subprocess.Popen([sys.executable, str(supervisor), str(tmp_path)], env=env, start_new_session=True)
    owned = []
    try:
        wait_path(tmp_path / 'ready.json')
        ready = json.loads((tmp_path / 'ready.json').read_text()); owned = ready['children']
        parent.send_signal(signal.SIGINT)
        wait_path(tmp_path / 'cleanup-started')
        # The exact failure sequence: a further termination arrives during finally.
        os.kill(ready['probe'], signal.SIGTERM)
        parent.send_signal(signal.SIGTERM)
        assert parent.wait(10) == 0
        row = json.loads((tmp_path / 'row.json').read_text())
        result = json.loads((tmp_path / 'result.json').read_text())
        assert row['interruption']['signal'] == 'SIGINT'
        assert row['exit_code'] == 1 and row['probe_reaped'] is True
        assert 'probe_forced' not in row
        assert result['interruption'] == 'ProbeInterrupted'
        assert result['owned_app_exited'] and result['owned_daemon_exited'] and result['owned_processes_reaped']
        assert all(code is not None for code in result['exit_codes'])
        assert any(error['stage'] == 'app_quit' for error in result['cleanup_errors'])
        assert result['ok'] is False, 'cleanup recovery must not convert interruption into success'
        assert unrelated.poll() is None
        assert all(not helper.alive(pid) for pid in owned)
    finally:
        if parent.poll() is None: parent.kill(); parent.wait()
        for pid in owned:
            if helper.alive(pid): os.kill(pid, signal.SIGKILL)
        unrelated.terminate(); unrelated.wait()


def test_probe_timeout_retains_unknown_cleanup_and_reaps_only_its_child(tmp_path, monkeypatch):
    script = tmp_path / 'stuck.py'
    ready = tmp_path / 'stuck-ready'
    script.write_text("import pathlib,signal,time\nsignal.signal(signal.SIGTERM,signal.SIG_IGN)\n"
                      + f"pathlib.Path({str(ready)!r}).write_text('ready')\ntime.sleep(60)\n")
    unrelated = subprocess.Popen([sys.executable, '-c', 'import time; time.sleep(60)'])
    # Start the short timeout only after the synthetic child installed its
    # SIGTERM policy. Loaded CI must not turn this into a startup timing test.
    popen = subprocess.Popen
    def ready_child(*args, **kwargs):
        child = popen(*args, **kwargs)
        try: wait_path(ready)
        except BaseException:
            child.kill(); child.wait()
            raise
        return child
    monkeypatch.setattr(runner.subprocess, 'Popen', ready_child)
    row = {}
    try:
        with (tmp_path / 'log').open('w') as log:
            code = runner.run_probe([sys.executable, str(script)], log, row, timeout=.2, cleanup_seconds=.2)
        assert code == -signal.SIGKILL
        assert row['interruption']['type'] == 'TimeoutExpired' and row['probe_forced'] is True
        assert row['probe_reaped'] is True and 'owned_daemon_exited' not in row
        assert unrelated.poll() is None
    finally:
        unrelated.terminate(); unrelated.wait()


def test_changed_or_unverified_pid_is_never_signaled(tmp_path):
    unrelated = subprocess.Popen([sys.executable, '-c', 'import time; time.sleep(60)'])
    fixture = subprocess.Popen([sys.executable, '-c', 'import time; time.sleep(60)'])
    session = helper.Session.__new__(helper.Session)
    session.report = {}; session.processes = [fixture]; session.logs = []; session.sock = None
    session.app_pid = unrelated.pid; session.driver_pid = None
    session.owned_identities = {'app': (unrelated.pid, 'different process start identity')}
    session.ui = lambda *args, **kwargs: pytest.fail('unverified process must not receive UI commands')
    try:
        session.close()
        assert unrelated.poll() is None
        assert fixture.poll() is not None
        assert session.report['owned_app_exited'] is False
        assert session.report['owned_processes_reaped'] is True
        assert 'identity changed' in session.report['cleanup_errors'][0]['error']
    finally:
        unrelated.terminate(); unrelated.wait()
        if fixture.poll() is None: fixture.terminate(); fixture.wait()


def guard_session(tmp_path):
    guard = SimpleNamespace(pid=42, poll=lambda: None)
    session = helper.Session.__new__(helper.Session)
    session.work = tmp_path; session.processes = [guard]; session.fixture = Path(sys.executable).resolve()
    session.ui = lambda *args: {'path': str(session.fixture)}
    return session, guard


def test_guard_show_ignores_old_receipts_and_requires_current_visible_window(tmp_path, monkeypatch):
    session, guard = guard_session(tmp_path)
    receipt = {'cmd': 'show', 'mode': 'guard', 'window': {'title': 'ICU Foreground Guard',
        'visible': True, 'miniaturized': False, 'on_active_space': True, 'time': 1}}
    path = tmp_path / 'command-results.jsonl'; path.write_text(json.dumps(receipt) + '\n')
    def respond(predicate, seconds, message, interval=.1):
        assert predicate() is None, 'an old show receipt must not authorize a new handoff'
        assert json.loads((tmp_path / 'command-guard.json').read_text()) == {'cmd': 'show'}
        receipt['window']['time'] = time.time()
        with path.open('a') as output: output.write(json.dumps(receipt) + '\n')
        return predicate()
    monkeypatch.setattr(helper, 'wait_for', respond)
    assert session.show_guard(guard)['window']['visible'] is True


def test_guard_show_refuses_unowned_or_hidden_guard(tmp_path, monkeypatch):
    session, guard = guard_session(tmp_path)
    with pytest.raises(AssertionError, match='owned'): session.show_guard(SimpleNamespace(pid=99, poll=lambda: None))
    def hidden(predicate, *_):
        (tmp_path / 'command-results.jsonl').write_text(json.dumps({'cmd': 'show', 'mode': 'guard',
            'window': {'title': 'ICU Foreground Guard', 'visible': False, 'miniaturized': False,
                       'on_active_space': True, 'time': time.time()}}) + '\n')
        return predicate()
    monkeypatch.setattr(helper, 'wait_for', hidden)
    with pytest.raises(AssertionError, match='not visible'): session.show_guard(guard)


def command_session(tmp_path, monkeypatch):
    session, guard = guard_session(tmp_path)
    session.app_pid = 101; session.app_exe = Path(sys.executable).resolve()
    session.owned_identities = {'app': (101, 'owned-start')}
    state = {'front': guard.pid, 'focused': False, 'value': '', 'guard_shown': True}
    actions = []
    monkeypatch.setattr(helper, 'process_identity', lambda pid: 'owned-start' if pid == 101 else None)
    def tree(pid=None):
        assert pid in (None, 101)
        return [
            {'role':'AXWindow','name':'Copilot','frame':{'x':0,'y':0,'w':800,'h':600}},
            {'role':'AXTextField','name':'','placeholder':'e.g. in @Calculator','focused':state['focused'],
             'value':state['value'],'frame':{'x':20,'y':400,'w':300,'h':30}},
        ]
    session.tree = tree
    def ui(command, *args, **kwargs):
        actions.append((command,args))
        if command == 'pidpath': return {'path':str(session.app_exe)}
        if command == 'state': return {'front_pid':state['front'],'focused_element':{'pid':state['front']}}
        if command == 'click':
            if args == (400.,12):
                state.update(front=101,focused=False)
            elif args == (170.,415.):
                assert state['front'] == 101, 'the guard intercepted a covered command-field click'
                state['focused'] = True
            elif args == (30,430):
                assert state['guard_shown']
                state.update(front=guard.pid,focused=False)
            else: raise AssertionError(f'unexpected synthetic click {args}')
            return {'ok':True}
        if command in ('key','type'):
            assert state['front'] == 101 and state['focused'], 'typed into another application'
            state['value'] = '' if command == 'key' else args[0]
            return {'ok':True}
        raise AssertionError(command)
    session.ui = ui
    write = helper.write_report
    def fixture_receipt(path, report):
        write(path,report)
        if Path(path).name == 'command-guard.json':
            Path(path).unlink()
            state['guard_shown'] = True
            with (tmp_path/'command-results.jsonl').open('a') as receipt:
                receipt.write(json.dumps({'cmd':'show','mode':'guard','window':{
                    'title':'ICU Foreground Guard','visible':True,'miniaturized':False,
                    'on_active_space':True,'time':time.time()}})+'\n')
    monkeypatch.setattr(helper,'write_report',fixture_receipt)
    return session,guard,state,actions


def test_two_goals_return_guard_to_copilot_then_back_to_guard(tmp_path, monkeypatch):
    session,guard,state,actions = command_session(tmp_path,monkeypatch)
    for goal in ('first goal','user retry goal'):
        assert state['front'] == guard.pid
        handoff = session.enter_goal(goal)
        assert handoff['pid'] == handoff['front_pid'] == session.app_pid
        assert state['value'] == goal and state['focused'] is True
        assert session.show_guard(guard)['window']['visible'] is True
        session.ui('click',30,430)
        assert state['front'] == guard.pid
    clicks = [args for command,args in actions if command == 'click']
    assert clicks == [(400.,12),(170.,415.),(30,430)] * 2
    assert [args for command,args in actions if command == 'type'] == [('first goal',),('user retry goal',)]


@pytest.mark.parametrize('failure',['identity','executable','occluded_titlebar'])
def test_command_setup_refuses_unproven_app_before_typing(tmp_path,monkeypatch,failure):
    session,guard,state,actions = command_session(tmp_path,monkeypatch)
    if failure == 'identity': monkeypatch.setattr(helper,'process_identity',lambda pid:'reused-pid')
    if failure == 'executable':
        ui=session.ui
        session.ui=lambda command,*args,**kwargs: {'path':sys.executable} if command=='pidpath' else ui(command,*args,**kwargs)
        session.app_exe = tmp_path/'different-app'
    if failure == 'occluded_titlebar':
        ui=session.ui
        session.ui=lambda command,*args,**kwargs: {'ok':True} if command=='click' else ui(command,*args,**kwargs)
        wait=helper.wait_for
        monkeypatch.setattr(helper,'wait_for',lambda pred,seconds,message,interval=.1:wait(pred,.02,message,.005))
    with pytest.raises((AssertionError,RuntimeError)): session.enter_goal('must not be typed')
    assert all(command not in ('type','key') for command,_ in actions)
    assert state['front'] == guard.pid
