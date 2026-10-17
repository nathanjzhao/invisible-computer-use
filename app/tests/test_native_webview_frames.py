"""Actual macOS WebKit, with prohibited activation and no desktop input."""
import json
from pathlib import Path
import subprocess
import sys

import pytest

APP = Path(__file__).resolve().parents[1]
pytestmark = pytest.mark.skipif(sys.platform != "darwin", reason="macOS WebKit probe")

SCRIPT = r"""
import {loadFrame, releaseFrame} from 'tauri://localhost/frames.js';
(async () => {
  const expect = (condition, message) => { if (!condition) throw new Error(message); };
  const revoked = async url => { try { await fetch(url); return false; } catch { return true; } };
  try {
    const img = new Image(); document.body.append(img);
    expect(await loadFrame(img, 'frame://localhost/success'), 'first frame refused');
    await img.decode();
    expect(img.naturalWidth === 1 && img.src.startsWith('blob:'), 'frame did not decode');
    const first = img.src;
    expect(!await loadFrame(img, 'frame://localhost/refused'), 'refusal reported success');
    expect(img.src === first, 'refusal replaced the good frame');
    expect(await loadFrame(img, 'frame://localhost/second'), 'second frame refused');
    expect(await revoked(first), 'replaced blob leaked');
    const second = img.src;
    const start = performance.now();
    expect(!await loadFrame(img, 'frame://localhost/hang'), 'hung frame reported success');
    const elapsed = performance.now() - start;
    expect(elapsed >= 3500 && elapsed < 6000, 'capture deadline not respected: ' + elapsed);
    expect(img.src === second, 'timeout replaced the good frame');
    releaseFrame(img);
    expect(await revoked(second), 'released blob leaked');
    window.webkit.messageHandlers.result.postMessage({ok: true, elapsed_ms: elapsed});
  } catch (error) {
    window.webkit.messageHandlers.result.postMessage({ok: false, error: String(error)});
  }
})();
"""

CANCEL_SCRIPT = r"""
import {loadFrame, releaseFrame} from 'tauri://localhost/frames.js';
(async () => {
  const expect = (condition, message) => { if (!condition) throw new Error(message); };
  const status = async () => (await fetch('tauri://localhost/status', {cache:'no-store'})).json();
  const waitFor = async predicate => {
    const end = performance.now() + 1500;
    while (performance.now() < end) {
      if (await predicate(await status())) return;
      await new Promise(resolve => setTimeout(resolve, 20));
    }
    throw new Error('native request state did not arrive');
  };
  try {
    const img = new Image(); document.body.append(img);
    expect(await loadFrame(img, 'frame://localhost/success'), 'initial frame refused');
    const old = img.src;
    const pending = loadFrame(img, 'frame://localhost/hang');
    await waitFor(s => s.requests.includes('frame://localhost/hang'));
    const start = performance.now();
    releaseFrame(img);
    const result = await Promise.race([
      pending,
      new Promise((_, reject) => setTimeout(() => reject(new Error('release did not cancel promptly')), 1000)),
    ]);
    expect(result === false, 'released request reported success');
    expect(img.getAttribute('src') === null, 'retired image still has a source');
    let revoked = false;
    try { await fetch(old); } catch { revoked = true; }
    expect(revoked, 'retired image blob is still readable');
    await waitFor(s => s.stopped_requests.includes('frame://localhost/hang'));
    window.webkit.messageHandlers.result.postMessage({ok:true, release_ms:performance.now()-start});
  } catch (error) {
    window.webkit.messageHandlers.result.postMessage({ok:false, error:String(error)});
  }
})();
"""


@pytest.fixture(scope="module")
def native_probe(tmp_path_factory):
    work = tmp_path_factory.mktemp("native-webkit")
    exe = work / "probe"
    subprocess.run([
        "swiftc", str(APP / "tests/native_frame_probe.swift"),
        "-framework", "AppKit", "-framework", "WebKit", "-o", str(exe)
    ], check=True, capture_output=True, timeout=120)
    module = work / "frames.js"
    source = (APP / "src/frames.ts").read_text()
    result = subprocess.run([
        "node", "--input-type=module", "-e",
        "import ts from 'typescript'; import fs from 'node:fs'; "
        "process.stdout.write(ts.transpileModule(fs.readFileSync(0,'utf8'),"
        "{compilerOptions:{target:ts.ScriptTarget.ES2022,module:ts.ModuleKind.ES2022}}).outputText);"
    ], cwd=APP, input=source, text=True, capture_output=True, check=True, timeout=30)
    module.write_text(result.stdout)
    script = work / "probe.js"
    script.write_text(SCRIPT)
    return exe, module, script


def run_probe(native_probe, module=None, script=None):
    exe, default_module, default_script = native_probe
    result = subprocess.run([str(exe), str(module or default_module), str(script or default_script)],
                            capture_output=True, text=True, timeout=45)
    report = json.loads(result.stdout.strip().splitlines()[-1])
    # The user may switch apps during the probe. That must not be mistaken for
    # the prohibited test application stealing focus.
    assert report["probe_never_activated"], report
    return result, report


def test_production_loader_in_native_webkit(native_probe):
    result, report = run_probe(native_probe)
    assert result.returncode == 0, report
    assert report["ok"], report
    assert [url for url in report["requests"] if url.startswith("frame:")] == [
        "frame://localhost/success", "frame://localhost/refused",
        "frame://localhost/second", "frame://localhost/hang",
    ], report
    # WKWebView may not acknowledge AbortController for a custom scheme. The
    # production loader must return on time regardless; native work is bounded
    # separately by the backend transport deadlines and capture admission cap.


def test_native_guard_rejects_a_false_success_mutation(native_probe, tmp_path):
    # Mutate only a scratch copy. Production source stays untouched.
    broken = tmp_path / "broken.js"
    code = native_probe[1].read_text()
    needle = "if (!response.ok || response.status === 204)"
    assert needle in code
    broken.write_text(code.replace(needle, "if (false)", 1).replace(
        "if (!bytes.size)", "if (false)", 1))
    # Reporting success before decoding must fail the refusal assertion.
    broken.write_text(broken.read_text().replace("const bytes = await response.blob();",
        "if (response.status === 204) return true;\nconst bytes = await response.blob();", 1))
    result, report = run_probe(native_probe, broken)
    assert result.returncode != 0, report
    assert "refusal reported success" in report.get("error", ""), report


def test_release_cancels_native_scheme_work_without_activating(native_probe, tmp_path):
    script = tmp_path / 'cancel.js'
    script.write_text(CANCEL_SCRIPT)
    result, report = run_probe(native_probe, script=script)
    assert result.returncode == 0 and report['ok'], report
    assert report['release_ms'] < 1500, report
    assert 'frame://localhost/hang' in report['stopped_requests'], report
    print(json.dumps(report, sort_keys=True))


def test_native_release_guard_rejects_omitted_abort(native_probe, tmp_path):
    script = tmp_path / 'cancel.js'
    script.write_text(CANCEL_SCRIPT)
    code = native_probe[1].read_text()
    assert code.count('pending?.abort();') == 1
    broken = tmp_path / 'uncancelled.js'
    broken.write_text(code.replace('pending?.abort();', '', 1))
    result, report = run_probe(native_probe, module=broken, script=script)
    assert result.returncode != 0 and not report['ok'], report
    assert 'release did not cancel promptly' in report.get('error', ''), report
    print(json.dumps(report, sort_keys=True))
