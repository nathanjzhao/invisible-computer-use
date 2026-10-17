"""The required golden guard must execute the workspace build or fail loudly."""
import os, subprocess, sys
from pathlib import Path
import test_conformance

ROOT = Path(__file__).resolve().parents[1]


def test_default_axdiff_binary_is_workspace_build(tmp_path, monkeypatch):
    monkeypatch.delenv('AXDIFF_CUA_BIN', raising=False)
    real = subprocess.run
    seen = []
    def checked(argv, **kw):
        assert Path(argv[0]) == ROOT / 'impl/target/debug/axdiff-cua'
        seen.append(argv[0])
        return real(argv, **kw)
    monkeypatch.setattr(subprocess, 'run', checked)
    test_conformance.test_ourlayer_ax_diff_matches_golden_format(tmp_path)
    assert len(seen) == 2


def test_missing_required_axdiff_binary_is_failure(tmp_path):
    env = dict(os.environ, AXDIFF_CUA_BIN=str(tmp_path / 'absent'))
    p = subprocess.run([sys.executable, '-m', 'pytest', 'conformance/test_conformance.py', '-q',
                        '-k', 'test_ourlayer_ax_diff_matches_golden_format'],
                       cwd=ROOT, env=env, capture_output=True, text=True, timeout=10)
    assert p.returncode == 1, p.stdout + p.stderr
    assert '1 failed' in p.stdout and '1 skipped' not in p.stdout
