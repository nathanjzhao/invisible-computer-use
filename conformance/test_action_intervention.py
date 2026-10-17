"""Exercise the real live() cleanup with fake processes/RPC and no desktop access."""
from pathlib import Path
import importlib.util
import json
import tempfile
from unittest.mock import patch

ROOT = Path(__file__).resolve().parents[1]
spec = importlib.util.spec_from_file_location('review_live', ROOT/'impl/native/macos-drive/audit-live.py')
live = importlib.util.module_from_spec(spec)
spec.loader.exec_module(live)
import pytest

@pytest.mark.parametrize("scenario", ["after_action", "during_cleanup", "owned_regression"])
def test_real_live_preserves_changed_desktop(tmp_path, scenario):
    base = {'front_pid': 10, 'pointer': {'x': 1, 'y': 2}, 'windowserver_front_psn': [1]}
    changed = {'front_pid': 90, 'pointer': {'x': 800, 'y': 600}, 'windowserver_front_psn': [9]}
    state = dict(base)
    calls = []
    restores = []


    class Result:
        returncode = 0
        stdout = '{}'
        stderr = ''


    class Thread:
        def __init__(self, **_): pass
        def start(self): pass
        def join(self, **_): pass


    class Process:
        def __init__(self, argv, **_):
            self.pid = 20 if 'serve' in list(map(str, argv)) else 30
            self.returncode = None
            if self.pid == 20:
                Path(argv[argv.index('--socket')+1]).touch()
            else:
                out = Path(argv[-1])
                (out/'fixture-ready.json').write_text(json.dumps({'pid': self.pid, 'window_id': 50,
                    'bounds': {'X': 100, 'Y': 100, 'Width': 640, 'Height': 480}}))
                self.out = out
        def poll(self): return self.returncode
        def terminate(self):
            self.returncode = 0
            if scenario == 'during_cleanup': state.clear(); state.update(changed)
        def kill(self): self.returncode = -9
        def wait(self, **_): self.returncode = 0


    class RPC:
        def __init__(self, argv, env, out, label): self.out = out
        def close(self): pass
        def call(self, tool, args):
            calls.append(tool)
            if tool == 'health_report':
                return {'checks': [{'name': 'host_interactive', 'data': {'host': {
                    'screen_locked': False, 'display_asleep': False}}}]}
            if tool == 'get_window_state':
                return {'elements': [{'identifier': 'audit-play-control', 'element_token': 's1:0'}]}
            if tool == 'click':
                (self.out/'fixture-events.jsonl').write_text(json.dumps({'kind': 'button_action'})+'\n')
                if scenario == 'during_cleanup':
                    # No ownership mismatch exists at failure time. The person
                    # changes desktop only while child teardown is underway.
                    raise RuntimeError('driver refusal before any ownership change')
                # Simulate a person switching apps/moving the pointer after the first completed click.
                state.clear(); state.update(changed if scenario == 'after_action' else dict(base, front_pid=30, windowserver_front_psn=[3]))
            return {}


    def fake_run(argv, **_):
        if 'restore' in list(map(str, argv)):
            restores.append(list(map(str, argv)))
            state.clear(); state.update(base)
        return Result()


    with tempfile.TemporaryDirectory(prefix='icu-review-no-desktop-') as temp:
        root = Path(temp)
        (root/'study/audit/fixes').mkdir(parents=True)
        binary = root/'fake-executable'; binary.touch()
        with patch.multiple(live, ROOT=root, DRIVER=binary, ICU=binary, EXE=binary, PROBE=binary,
                            RPC=RPC, snapshot=lambda: dict(state), run=fake_run,
                            validate_prepared_build=lambda: {}, save_surfaces=lambda *a: []), \
             patch.object(live.subprocess, 'Popen', Process), \
             patch.object(live.threading, 'Thread', Thread), \
             patch.object(live.time, 'sleep', lambda _: None), \
             patch.object(live.signal, 'signal', lambda *a: None), \
             patch.object(live.signal, 'alarm', lambda *a: None), \
             patch.dict(live.os.environ, {'ICU_BOUNDED_TEST': '1'}):
            try:
                live.live()
            except (AssertionError, RuntimeError) as error:
                failure = str(error)
            else:
                raise AssertionError('Probe failed to exercise ownership failure')
        result_path = next((root/'study/audit/fixes').glob('live-*/RESULT.json'))
        evidence = {'real_desktop_access': False, 'failure': failure, 'rpc_calls': calls,
                    'restore_commands': restores, 'result': json.loads(result_path.read_text()),
                    'final_simulated_desktop': state}
        assert calls.count('click') == 1
        assert not restores, 'cleanup overwrote changed user state'
        assert state == (changed if scenario != 'owned_regression' else dict(base, front_pid=30, windowserver_front_psn=[3]))
        assert evidence['result']['cleanup_policy'] == 'preserve_changed_desktop'
        if scenario == 'owned_regression':
            assert evidence['result']['ownership_failure']['classification'] == 'owned_process_regression'
        print(json.dumps(evidence, indent=2))
