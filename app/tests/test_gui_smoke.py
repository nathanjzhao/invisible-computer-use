"""Headless GUI smoke for the Copilot webview.

Serves the *built* `dist/` in a real Chromium (Playwright), mocks the Tauri
bridge (`window.__TAURI_INTERNALS__`), and drives the actual UI end to end:

1. launch flow — type `@Calc`, the mention popover offers Calculator as a LAUNCH
   row (it isn't open), selecting it calls `cua_launch_app` and references the
   launched window in the command bar;
2. drive flow — `@`-mention an already-open window and submit, and `agent_run`
   is invoked against that exact window with the typed goal.

Run: `python3 -m pytest app/tests/test_gui_smoke.py -q` (chromium from the local
Playwright cache; no network). The app is rebuilt into `dist/` by the fixture.
"""
import http.server
import os
import re
import base64
import struct
import zlib
import socket
import socketserver
import subprocess
import threading
from pathlib import Path

import pytest
from playwright.sync_api import sync_playwright

APP_DIR = Path(__file__).resolve().parents[1]
DIST = APP_DIR / "dist"
STOP_GRACE_MS = int(re.search(r"STOP_GRACE_MS\s*=\s*(\d+)", (APP_DIR / "src/send-control.ts").read_text()).group(1))

# Injected before the app module runs: a complete-enough Tauri v2 bridge.
INIT = r"""
window.__ICU_CALLS__ = [];
(() => {
  let cbid = 0; const cbs = {}; const listeners = {};
  const readTask = () => JSON.parse(localStorage.getItem('icu-test-task') || 'null');
  const saveTask = task => localStorage.setItem('icu-test-task', JSON.stringify(task));
  window.__ICU_EMIT__ = payload => {
    if (['done','error','fail','stopped'].includes(payload.kind) && readTask()?.runId === payload.runId) saveTask(null);
    listeners.agent?.({event:'agent',payload});
  };
  const WINDOWS = [
    { app_name: "TextEdit", title: "notes.txt", pid: 700, window_id: 1452,
      is_on_screen: true, bounds: {x:0,y:0,width:400,height:300} },
  ];
  if (localStorage.getItem('icu-test-off-space')) WINDOWS[0].on_current_space = false;
  WINDOWS[0].app_name = localStorage.getItem('icu-test-window-name') || WINDOWS[0].app_name;
  Object.assign(WINDOWS[0].bounds, JSON.parse(localStorage.getItem('icu-test-window-bounds') || '{}'));
  for (const extra of JSON.parse(localStorage.getItem('icu-test-extra-windows') || '[]')) {
    WINDOWS.push({...WINDOWS[0], ...extra});
  }
  window.__ICU_WINDOWS__ = WINDOWS;
  const APPS = [
    { name: "TextEdit", bundle_id: "com.apple.TextEdit", running: true },
    { name: "Calculator", bundle_id: "com.apple.calculator", running: false },
    { name: "Numbers", bundle_id: "com.apple.iWork.Numbers", running: false },
  ];
  const LAUNCHED = { app_name: "Calculator", title: "", pid: 4242, window_id: 9001,
      on_current_space: true, is_on_screen: true, bounds: {x:10,y:10,width:320,height:400} };
  const manual = window.__ICU_MANUAL__ = {selected:'before',before:'',after:''};
  let releaseClick;
  window.__ICU_RELEASE_CLICK__ = async (fail = false) => {
    if (!releaseClick) throw new Error('no pending manual click');
    releaseClick(fail);
    await new Promise(resolve => setTimeout(resolve, 0));
  };
  const pendingLaunch = [];
  window.__ICU_RELEASE_LAUNCH__ = async () => {
    localStorage.removeItem('icu-test-hold-launch');
    pendingLaunch.splice(0).forEach(resolve => resolve());
    await new Promise(resolve => setTimeout(resolve, 0));
  };
  const pendingDiscovery = [];
  const discovery = (value, kind) => (localStorage.getItem('icu-test-hold-discovery') ||
    (kind === 'apps' && localStorage.getItem('icu-test-hold-apps')))
    ? new Promise(resolve => pendingDiscovery.push(() => resolve(value))) : Promise.resolve(value);
  window.__ICU_RELEASE_DISCOVERY__ = async () => {
    localStorage.removeItem('icu-test-hold-discovery');
    localStorage.removeItem('icu-test-hold-apps');
    pendingDiscovery.splice(0).forEach(resolve => resolve());
    await new Promise(resolve => setTimeout(resolve, 0));
  };
  window.__ICU_MEM__ = {
    settings: { chatMemories: true, skysight: false, paused: false, retentionDays: 7, idleMinutes: 30, provider: "", extractModel: null, consolidationModel: null },
    list: {
      entries: [
        { id: "e-1", scope: "com.apple.iWork.Pages", kind: "preference", text: "The user wants document titles in all caps.", untrusted: true, evidence: ["s-1"] },
        { id: "e-2", scope: "com.apple.iWork.Pages", kind: "app_fact", text: "In Pages the title field sits above the body text.", untrusted: true, evidence: ["s-1"] },
      ],
      summaries: [{ id: "s-1", app_name: "Pages", title: "Setting a report title", asked: "The user asked to set the title, then corrected it to all caps.", ended: 1, untrusted: true, notes: [] }],
      pending: [],
      skysight: { summaries: 0, profile: [] },
    },
  };
  window.__TAURI_INTERNALS__ = {
    convertFileSrc(path, protocol) { return `http://${protocol}.localhost/${path}`; },
    transformCallback(cb) { const id = ++cbid; cbs[id] = cb; return id; },
    unregisterCallback(id) { delete cbs[id]; },
    invoke(cmd, args) {
      window.__ICU_CALLS__.push({ cmd, args: args || {} });
      switch (cmd) {
        case "cua_status": return Promise.resolve(true);
        case "cua_display_count": return Promise.resolve(1);
        case "cua_list_windows": return discovery({ windows: WINDOWS });
        case "cua_list_apps": return discovery({ apps: APPS }, 'apps');
        case "cua_launch_app": return localStorage.getItem('icu-test-hold-launch')
          ? new Promise(resolve => pendingLaunch.push(() => resolve({pid:LAUNCHED.pid,windows:[LAUNCHED]})))
          : Promise.resolve({ pid: LAUNCHED.pid, windows: [LAUNCHED] });
        case "cua_get_state":
          return Promise.resolve({ elements: [], window_bounds: LAUNCHED.bounds, window_title: "" });
        case "cua_window_shot": return Promise.resolve("");
        case "cua_activate": return Promise.resolve({ on_current_space: true });
        case "cua_click":
          if (!localStorage.getItem('icu-test-manual')) return Promise.reject(new Error('manual fixture disabled'));
          return new Promise((resolve, reject) => { releaseClick = fail => {
            if (fail) reject(new Error('held click failed'));
            else { manual.selected = 'after'; resolve({}); }
          }; });
        case "cua_scroll":
        case "cua_press_key":
          if (!localStorage.getItem('icu-test-manual')) return Promise.reject(new Error('manual fixture disabled'));
          return Promise.resolve({});
        case "cua_type":
          if (!localStorage.getItem('icu-test-manual')) return Promise.reject(new Error('manual fixture disabled'));
          manual[manual.selected] += args.text;
          return Promise.resolve({});
        case "agent_run": {
          if (!listeners.agent) return Promise.reject(new Error('agent listener is not ready'));
          if (readTask()) return Promise.reject(new Error('a task is already running'));
          const task = {...args, runId: args.runId || 'legacy-run', stopping: false};
          saveTask(task);
          return new Promise(resolve => setTimeout(() => resolve(task.runId),
            Number(localStorage.getItem('icu-test-start-delay') || 0)));
        }
        case "agent_stop": {
          const task = readTask();
          if (!task || task.runId !== args.runId) return Promise.resolve(false);
          task.stopping = true; saveTask(task); return Promise.resolve(true);
        }
        case "agent_status": {
          if (localStorage.getItem('icu-test-status-error')) return Promise.reject(new Error('status unavailable'));
          const task = readTask();
          if (task && localStorage.getItem('icu-test-status-race')) {
            window.__ICU_EMIT__({runId:task.runId,kind:'done',data:{summary:'completed during restore'}});
          }
          return Promise.resolve(task);
        }
        case "plugin:event|listen":
          if (localStorage.getItem('icu-test-listen-error')) return Promise.reject(new Error('events unavailable'));
          return new Promise(resolve => {
          setTimeout(() => { listeners[args.event] = cbs[args.handler]; resolve(1); },
            Number(localStorage.getItem('icu-test-listen-delay') || 0));
        });
        case "plugin:event|unlisten": return Promise.resolve();
        case "plugin:updater|check": return Promise.reject(new Error("no endpoint in test"));
        case "cua_permissions":
          return Promise.resolve(JSON.parse(localStorage.getItem('icu-test-permissions') ||
            '{"supported":true,"accessibility":true,"screen_recording":true}'));
        case "memory_status": {
          const m = window.__ICU_MEM__;
          return Promise.resolve({ state: m.settings.paused ? 'paused' : 'on', settings: m.settings, ratePausedUntil: null, lastError: null });
        }
        case "memory_list": return Promise.resolve(window.__ICU_MEM__.list);
        case "memory_pause": window.__ICU_MEM__.settings.paused = args.paused; return Promise.resolve(window.__ICU_MEM__.settings);
        case "memory_settings_set": Object.assign(window.__ICU_MEM__.settings, args.settings); return Promise.resolve(window.__ICU_MEM__.settings);
        case "memory_delete": {
          const l = window.__ICU_MEM__.list;
          l.entries = l.entries.filter(e => e.id !== args.id);
          l.summaries = l.summaries.filter(e => e.id !== args.id);
          return Promise.resolve(1);
        }
        case "cua_request_permissions": {
          const current = JSON.parse(localStorage.getItem('icu-test-permissions') || '{}');
          const next = JSON.parse(localStorage.getItem('icu-test-request-permissions') || 'null') ||
            {...current, supported:true, accessibility:true, screen_recording:true, error:null};
          localStorage.setItem('icu-test-permissions', JSON.stringify(next));
          return Promise.resolve(next);
        }
        case "model_onboarding_status":
          return Promise.resolve({ needsOnboarding: localStorage.getItem('icu-test-needs-onboarding') === '1',
            anthropic: false, openai: false, claudeCli: false, storedAnthropic: false, storedOpenai: false });
        case "set_model_key": {
          // Once a key is saved the app is configured; echo a status that reflects it.
          localStorage.setItem('icu-test-needs-onboarding', '0');
          return Promise.resolve({ needsOnboarding: false,
            anthropic: args.provider === 'anthropic', openai: args.provider === 'openai',
            claudeCli: false, storedAnthropic: args.provider === 'anthropic', storedOpenai: args.provider === 'openai' });
        }
        default: return Promise.reject(new Error(`unmocked command: ${cmd}`));
      }
    },
  };
})();
"""


class _Handler(http.server.SimpleHTTPRequestHandler):
    def guess_type(self, path):  # force correct MIME so ES modules load
        if str(path).endswith(".js") or str(path).endswith(".mjs"):
            return "text/javascript"
        if str(path).endswith(".css"):
            return "text/css"
        return super().guess_type(path)

    def log_message(self, *_):  # quiet
        pass


@pytest.fixture(scope="session")
def base_url():
    # Build the frontend into dist/ so the smoke runs against real bundle output.
    subprocess.run(["npx", "vite", "build"], cwd=APP_DIR, check=True,
                   stdout=subprocess.DEVNULL, stderr=subprocess.STDOUT)
    handler = lambda *a, **k: _Handler(*a, directory=str(DIST), **k)  # noqa: E731
    with socketserver.TCPServer(("127.0.0.1", 0), handler) as httpd:
        port = httpd.socket.getsockname()[1]
        t = threading.Thread(target=httpd.serve_forever, daemon=True)
        t.start()
        try:
            yield f"http://127.0.0.1:{port}/"
        finally:
            httpd.shutdown()


def _chromium_exe():
    cache = Path.home() / "Library/Caches/ms-playwright"
    for d in sorted(cache.glob("chromium_headless_shell-*"), reverse=True):
        exe = d / "chrome-mac/headless_shell"
        if exe.exists():
            return str(exe)
    return None


@pytest.fixture
def page(base_url):
    with sync_playwright() as p:
        exe = _chromium_exe()
        browser = p.chromium.launch(headless=True, executable_path=exe) if exe else p.chromium.launch(headless=True)
        pg = browser.new_page()
        # Exercise fetch -> blob -> decoded <img>, including the protocol CORS
        # boundary. This fixture has no connection to a native desktop.
        frame_requests = []
        png = base64.b64decode("iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mP8/x8AAwMCAO+jRZkAAAAASUVORK5CYII=")
        pg._frame_png = png
        def frame(route):
            frame_requests.append(route.request.url)
            route.fulfill(status=200, content_type="image/png",
                          headers={"Access-Control-Allow-Origin": "*"}, body=pg._frame_png)
        pg.route("http://frame.localhost/**", frame)
        pg._frame_requests = frame_requests
        pg.add_init_script(INIT)
        pg.goto(base_url)
        pg.wait_for_selector("#chat-input", timeout=10000)
        yield pg
        browser.close()


def _click_tile(page):
    # Tile clicks map against the decoded screenshot size; before the first
    # frame decodes that size is 0 and the click is (correctly) dropped.
    page.wait_for_function("() => { const i = document.querySelector('.tile img.shot'); return i && i.complete && i.naturalWidth > 0; }")
    page.locator('.tile img.shot').click()


def _calls(page, cmd):
    return page.evaluate("(c) => window.__ICU_CALLS__.filter(x => x.cmd === c)", cmd)


def test_launch_row_launches_and_references(page):
    """@Calc -> a LAUNCH row for Calculator -> select -> cua_launch_app + referenced."""
    inp = page.locator("#chat-input")
    inp.click()
    inp.fill("")
    inp.type("@Calc")
    # A launch row for Calculator (dimmed .mention-launch), not a window row.
    row = page.locator(".mention-pop .mention-row.mention-launch", has_text="Calculator")
    row.wait_for(state="visible", timeout=5000)
    row.dispatch_event("mousedown")
    # cua_launch_app was invoked for Calculator...
    page.wait_for_function("() => window.__ICU_CALLS__.some(c => c.cmd === 'cua_launch_app')", timeout=5000)
    launches = _calls(page, "cua_launch_app")
    assert launches, "cua_launch_app was never called"
    app_arg = str(launches[0]["args"].get("app", "")).lower()
    assert "calc" in app_arg, f"launched wrong app: {launches[0]['args']}"
    # ...and the launched window is referenced in the command bar.
    page.wait_for_function("() => document.querySelector('#chat-input').value.includes('@Calculator')", timeout=5000)
    assert "@Calculator" in inp.input_value()


def _submit_goal(page):
    inp = page.locator("#chat-input")
    inp.click()
    inp.fill("")
    inp.type("in @TextEdit")  # no trailing space — that would end the @token and close the popover
    win_row = page.locator(".mention-pop .mention-row", has_text="TextEdit").first
    win_row.wait_for(state="visible", timeout=5000)
    win_row.dispatch_event("mousedown")  # selecting inserts "@TextEdit " (with the space) and references it
    page.wait_for_function("() => document.querySelector('#chat-input').value.includes('@TextEdit')", timeout=5000)
    inp.focus()
    inp.type("type hello")
    page.locator("#chat-form").evaluate("f => f.requestSubmit ? f.requestSubmit() : f.dispatchEvent(new Event('submit', {cancelable: true, bubbles: true}))")
    page.wait_for_function("() => window.__ICU_CALLS__.some(c => c.cmd === 'agent_run')", timeout=5000)
    runs = _calls(page, "agent_run")
    assert runs, "agent_run was never called"
    a = runs[-1]["args"]
    assert a.get("pid") == 700 and a.get("windowId") == 1452, f"agent_run wrong target: {a}"
    assert "hello" in str(a.get("goal", "")).lower(), f"goal not carried: {a}"

    return a


def test_mention_open_window_and_run(page):
    """@TextEdit (already open) + a goal reaches the matching backend target."""
    _submit_goal(page)


def test_live_tile_displays_decoded_frame_without_a_second_capture(page):
    _submit_goal(page)
    page.wait_for_selector("img.shot[data-ok='1']", timeout=10000)
    assert page.locator('.capturing').first.is_hidden(), "loaded frame is still covered by the capture placeholder"
    shot = page.locator("img.shot[data-ok='1']").first
    assert shot.evaluate("img => img.complete && img.naturalWidth > 0")
    assert shot.get_attribute("src").startswith("blob:")
    urls = page._frame_requests
    assert urls, "the tile never requested a frame"
    assert len(urls) == len(set(urls)), "a frame URL triggered duplicate captures"


def test_stop_targets_the_current_run_and_waits_for_terminal_event(page):
    a = _submit_goal(page)
    button = page.locator('#chat-form button[type=submit]')
    assert button.inner_text() == 'Stop'
    assert page.locator('#chat-input').is_disabled()
    # The first activation is intentionally protected against duplicate AX/AT-SPI
    # submission. This case tests a separate Stop press, not that duplicate.
    page.wait_for_timeout(STOP_GRACE_MS + 50)
    button.click()
    page.wait_for_function("() => window.__ICU_CALLS__.some(c => c.cmd === 'agent_stop')")
    assert _calls(page, 'agent_stop')[-1]['args']['runId'] == a['runId']
    assert button.inner_text() == 'Stopping…'
    page.evaluate("id => window.__ICU_EMIT__({runId:id,kind:'stopped',data:{reason:'requested'}})", a['runId'])
    assert button.inner_text() == 'Send'
    assert page.locator('#chat-input').is_enabled()


def test_stale_events_do_not_finish_the_current_run(page):
    a = _submit_goal(page)
    page.evaluate("() => window.__ICU_EMIT__({runId:'older-run',kind:'done',data:{summary:'stale'}})")
    assert page.locator('#chat-form button[type=submit]').inner_text() == 'Stop'
    assert 'stale' not in page.locator('#chat-log').inner_text()
    page.evaluate("id => window.__ICU_EMIT__({runId:id,kind:'done',data:{summary:'current'}})", a.get('runId'))
    assert page.locator('#chat-form button[type=submit]').inner_text() == 'Send'


def test_stop_while_start_reply_is_pending_is_not_lost(page):
    page.evaluate("() => localStorage.setItem('icu-test-start-delay','2000')")
    a = _submit_goal(page)
    page.wait_for_timeout(STOP_GRACE_MS + 50)
    page.locator('#chat-form button[type=submit]').click()
    page.wait_for_function("() => window.__ICU_CALLS__.some(c => c.cmd === 'agent_stop')", timeout=5000)
    assert _calls(page, 'agent_stop')[-1]['args']['runId'] == a['runId']
    assert page.evaluate("() => JSON.parse(localStorage.getItem('icu-test-task')).stopping")


def test_missing_macos_permissions_are_named_and_requested_on_demand(page):
    # A granted app shows no notice and never asks the OS to prompt.
    page.wait_for_function("() => window.__ICU_CALLS__.some(c => c.cmd === 'cua_permissions')", timeout=5000)
    assert page.locator('#permission-notice').count() == 0
    page.evaluate("""() => localStorage.setItem('icu-test-permissions',
        '{"supported":true,"accessibility":false,"screen_recording":false}')""")
    page.reload()
    notice = page.locator('#permission-notice')
    notice.wait_for(state='visible', timeout=5000)
    assert 'Accessibility and Screen Recording' in notice.inner_text()
    assert not _calls(page, 'cua_request_permissions'), 'system prompts were requested without a user action'
    notice.locator('button', has_text='Grant access').click()
    page.wait_for_function("() => !document.querySelector('#permission-notice')", timeout=5000)
    assert len(_calls(page, 'cua_request_permissions')) == 1


def _set_permissions(page, payload):
    page.evaluate("p => localStorage.setItem('icu-test-permissions', JSON.stringify(p))", payload)
    page.reload()
    page.wait_for_function("() => window.__ICU_CALLS__.some(c => c.cmd === 'cua_permissions')", timeout=5000)


def test_an_external_driver_owns_its_own_permission_banner(page):
    # When an external driver holds capture, its grants are read from the driver
    # (source "driver"). The app cannot grant them, so no grant-the-app banner
    # appears, and a genuine gap or an unreadable status reads as a quiet note.
    page.wait_for_function("() => window.__ICU_CALLS__.some(c => c.cmd === 'cua_permissions')", timeout=5000)

    # The driver has what it needs: no banner at all, even though the app's own
    # preflight (unknown here) is never consulted.
    _set_permissions(page, {"supported": True, "accessibility": True, "screen_recording": True, "source": "driver"})
    page.wait_for_timeout(200)
    assert page.locator('#permission-notice').count() == 0

    # A genuine gap on the driver: a quiet note naming it as the driver's, with no
    # grant-the-app button and no system prompt.
    _set_permissions(page, {"supported": True, "accessibility": False, "screen_recording": False, "source": "driver"})
    notice = page.locator('#permission-notice')
    notice.wait_for(state='visible', timeout=5000)
    text = notice.inner_text()
    assert 'driver' in text and 'not to Copilot' in text, text
    assert 'Copilot needs' not in text, text
    assert notice.locator('.perm-grant').is_hidden(), 'an external driver must not offer a grant-the-app button'
    assert notice.evaluate("n => n.classList.contains('quiet')"), 'a driver note reads quietly'
    assert not _calls(page, 'cua_request_permissions'), 'an external driver never triggers an app prompt'

    # The driver's status could not be read: a quiet unavailable note, no demand.
    _set_permissions(page, {"supported": True, "accessibility": False, "screen_recording": False, "source": "driver", "error": "The selected driver's access could not be checked."})
    notice.wait_for(state='visible', timeout=5000)
    text = notice.inner_text()
    assert "Capture driver: The selected driver's access could not be checked." in text, text
    assert 'Copilot needs' not in text and 'not to Copilot' not in text, text
    assert notice.locator('.perm-grant').is_hidden()
    assert notice.locator('.perm-retry').is_visible()
    assert notice.locator('.perm-accessibility').is_hidden()
    assert notice.locator('.perm-screen').is_hidden()
    assert not _calls(page, 'cua_request_permissions')
    assert not _calls(page, 'cua_open_privacy_settings')

    # An old backend payload without `source` still behaves as the app's own.
    _set_permissions(page, {"supported": True, "accessibility": False, "screen_recording": True})
    notice.wait_for(state='visible', timeout=5000)
    assert 'Copilot needs Accessibility access' in notice.inner_text()
    assert notice.locator('.perm-grant').is_visible(), 'the app-owned case keeps its grant button'


@pytest.mark.parametrize('source', ['app', 'driver', None])
def test_capture_denial_requires_an_explicit_retry_even_with_grants(page, source):
    error = 'Screen capture was denied. Automatic capture is paused. Choose Retry access in Copilot after updating capture access.'
    status = {"supported": True, "accessibility": True, "screen_recording": True, "error": error}
    if source:
        status['source'] = source
    _set_permissions(page, status)
    notice = page.locator('#permission-notice')
    notice.wait_for(state='visible', timeout=5000)
    assert error in notice.inner_text()
    assert notice.inner_text().startswith('Capture driver:' if source == 'driver' else 'Copilot:')
    assert notice.evaluate("n => n.classList.contains('quiet')")
    assert notice.locator('.perm-retry').is_visible()
    for selector in ['.perm-grant', '.perm-accessibility', '.perm-screen']:
        assert notice.locator(selector).is_hidden()
    # Rechecking status on focus never clears a denial or requests access.
    page.evaluate("() => window.dispatchEvent(new Event('focus'))")
    page.wait_for_timeout(100)
    assert not _calls(page, 'cua_request_permissions')
    assert not _calls(page, 'cua_open_privacy_settings')
    # A refusal that persists after a click stays visible and is not retried.
    page.evaluate("s => localStorage.setItem('icu-test-request-permissions', JSON.stringify(s))", status)
    notice.locator('.perm-retry').click()
    page.wait_for_function("() => window.__ICU_CALLS__.filter(c => c.cmd === 'cua_request_permissions').length === 1")
    assert notice.is_visible()
    page.wait_for_timeout(100)
    assert len(_calls(page, 'cua_request_permissions')) == 1
    # When the backend clears the problem, only another user click removes it.
    page.evaluate("() => localStorage.removeItem('icu-test-request-permissions')")
    notice.locator('.perm-retry').click()
    page.wait_for_function("() => !document.querySelector('#permission-notice')")
    assert len(_calls(page, 'cua_request_permissions')) == 2
    assert not _calls(page, 'cua_open_privacy_settings')


def test_capture_error_takes_priority_without_hiding_real_host_grants(page):
    _set_permissions(page, {"supported": True, "accessibility": False, "screen_recording": False,
                            "source": "app", "error": "Screen capture was denied."})
    notice = page.locator('#permission-notice')
    notice.wait_for(state='visible', timeout=5000)
    assert notice.locator('.perm-text').inner_text() == 'Copilot: Screen capture was denied.'
    for selector in ['.perm-retry', '.perm-grant', '.perm-accessibility', '.perm-screen']:
        assert notice.locator(selector).is_visible()
    assert not _calls(page, 'cua_request_permissions')
    assert not _calls(page, 'cua_open_privacy_settings')


def test_reload_recovers_a_backend_task(page):
    a = _submit_goal(page)
    page.reload()
    page.wait_for_function("() => document.querySelector('#chat-form button[type=submit]').textContent === 'Stop'", timeout=5000)
    assert page.locator('#chat-input').is_disabled()
    page.locator('#chat-form button[type=submit]').click()
    page.wait_for_function("() => window.__ICU_CALLS__.some(c => c.cmd === 'agent_stop')")
    assert _calls(page, 'agent_stop')[-1]['args']['runId'] == a['runId']


def test_start_waits_for_event_subscription(page):
    page.evaluate("() => localStorage.setItem('icu-test-listen-delay','2000')")
    page.reload()
    _submit_goal(page)
    page.wait_for_function("() => JSON.parse(localStorage.getItem('icu-test-task') || 'null') !== null", timeout=5000)
    assert 'listener is not ready' not in page.locator('#chat-log').inner_text()


def test_terminal_event_during_status_restore_prevents_stale_adoption(page):
    _submit_goal(page)
    page.evaluate("() => localStorage.setItem('icu-test-status-race','1')")
    page.reload()
    page.wait_for_function("() => window.__ICU_CALLS__.some(c => c.cmd === 'agent_status')")
    assert page.locator('#chat-form button[type=submit]').inner_text() == 'Send'
    assert page.locator('#chat-input').is_enabled()


@pytest.mark.parametrize('failure', ['status', 'listen'])
def test_failed_task_restore_blocks_input_until_retry_succeeds(page, failure):
    task = _submit_goal(page)
    page.evaluate("failure => { localStorage.setItem(`icu-test-${failure}-error`,'1'); localStorage.setItem('icu-test-manual','1'); }", failure)
    page.reload()
    page.locator('#chat-log').get_by_text('Could not restore task status:', exact=False).wait_for()
    page.locator('#add-window').click()
    page.locator('#picker-list .pick-row', has_text='TextEdit').click()
    _click_tile(page)
    page.keyboard.type('x')
    page.locator('#chat-form').evaluate('form => form.requestSubmit()')
    page.wait_for_timeout(100)
    assert not _calls(page, 'cua_click'), 'manual click dispatched while backend task status was unknown'
    assert not _calls(page, 'cua_type')
    assert not _calls(page, 'agent_run')
    assert page.locator('#chat-input').is_disabled()
    assert page.locator('.tile button.ctl').is_disabled()
    assert page.locator('.tile.controlling').count() == 0
    page.evaluate("failure => localStorage.removeItem(`icu-test-${failure}-error`)", failure)
    button = page.locator('#chat-form button[type=submit]')
    assert button.inner_text() == 'Retry status'
    button.click()
    page.wait_for_function("() => document.querySelector('#chat-form button[type=submit]').textContent === 'Stop'")
    assert page.locator('#chat-input').is_disabled()
    assert page.locator('.tile button.ctl').is_disabled()
    button.click()
    page.wait_for_function("() => window.__ICU_CALLS__.some(c => c.cmd === 'agent_stop')")
    assert _calls(page, 'agent_stop')[-1]['args']['runId'] == task['runId']
    page.evaluate("runId => window.__ICU_EMIT__({runId,kind:'stopped',data:{}})", task['runId'])
    assert button.inner_text() == 'Send'
    assert page.locator('#chat-input').is_enabled()
    page.locator('.tile button.ctl').click()
    page.keyboard.type('ok')
    page.wait_for_function("() => window.__ICU_CALLS__.filter(c => c.cmd === 'cua_type').length === 2")
    assert [c['args']['text'] for c in _calls(page, 'cua_type')] == ['o','k']


def test_status_retry_recovers_idle_without_duplicate_subscription(page):
    page.evaluate("() => localStorage.setItem('icu-test-status-error','1')")
    page.reload()
    button = page.locator('#chat-form button[type=submit]')
    page.wait_for_function("() => document.querySelector('#chat-form button[type=submit]').textContent === 'Retry status'")
    artifact_dir = os.environ.get('ICU_GUI_ARTIFACT_DIR')
    if artifact_dir:
        Path(artifact_dir).mkdir(parents=True, exist_ok=True)
        page.screenshot(path=str(Path(artifact_dir) / 'retry-status.png'), full_page=True)
    assert page.locator('#chat-input').is_disabled()
    button.click()  # A second failed query must stay blocked.
    page.wait_for_function("() => window.__ICU_CALLS__.filter(c => c.cmd === 'agent_status').length === 2")
    assert button.inner_text() == 'Retry status'
    assert page.locator('#chat-input').is_disabled()
    page.evaluate("() => localStorage.removeItem('icu-test-status-error')")
    button.click()
    page.wait_for_function("() => document.querySelector('#chat-form button[type=submit]').textContent === 'Send'")
    assert page.locator('#chat-input').is_enabled()
    assert len(_calls(page, 'plugin:event|listen')) == 1
    assert len(_calls(page, 'agent_status')) == 3
    assert not _calls(page, 'agent_run')


def test_absent_space_metadata_is_not_labelled_off_space(page):
    _submit_goal(page)
    assert page.locator('.tile .badge.activate').count() == 0


def test_explicit_off_space_metadata_keeps_the_activation_control(page):
    page.evaluate("() => localStorage.setItem('icu-test-off-space','1')")
    page.reload()
    _submit_goal(page)
    assert page.locator('.tile .badge.activate').count() == 1


def test_remove_and_readd_during_capture_releases_the_old_frame(page):
    """Exercise the actual capture loop with a delayed browser response."""
    pending = []
    page.route("http://frame.localhost/**", lambda route: pending.append(route))
    page.evaluate("""() => {
        window.__LIVE_FRAME_BLOBS__ = new Set();
        const create = URL.createObjectURL.bind(URL), revoke = URL.revokeObjectURL.bind(URL);
        URL.createObjectURL = blob => { const u = create(blob); window.__LIVE_FRAME_BLOBS__.add(u); return u; };
        URL.revokeObjectURL = url => { window.__LIVE_FRAME_BLOBS__.delete(url); revoke(url); };
    }""")
    page.locator('#add-window').click()
    page.locator('#picker-list .pick-row', has_text='TextEdit').click()
    for _ in range(50):
        if pending:
            break
        page.wait_for_timeout(50)
    assert len(pending) == 1, 'first native frame request did not start'
    page.locator('.tile button.x').click()
    page.locator('#add-window').click()
    page.locator('#picker-list .pick-row', has_text='TextEdit').click()
    png = base64.b64decode("iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mP8/x8AAwMCAO+jRZkAAAAASUVORK5CYII=")
    pending[0].fulfill(status=200, content_type='image/png',
                       headers={'Access-Control-Allow-Origin': '*'}, body=png)
    for _ in range(50):
        if len(pending) >= 2:
            break
        page.wait_for_timeout(50)
    assert len(pending) >= 2, 'replacement tile never requested its own frame'
    pending[1].fulfill(status=200, content_type='image/png',
                       headers={'Access-Control-Allow-Origin': '*'}, body=png)
    page.wait_for_selector('img.shot[data-ok="1"]')
    live = page.evaluate('() => [...window.__LIVE_FRAME_BLOBS__]')
    current = page.locator('img.shot').get_attribute('src')
    assert live == [current], 'removed tile retained a live frame blob'
    page.locator('.tile button.x').click()
    assert page.evaluate('() => window.__LIVE_FRAME_BLOBS__.size') == 0


@pytest.mark.parametrize('query,name', [('@TextEdit', 'TextEdit'), ('@Calc', 'Calculator')])
@pytest.mark.parametrize('dismiss', [False, True])
def test_delayed_discovery_updates_only_requested_mentions(page, query, name, dismiss):
    page.evaluate("() => localStorage.setItem('icu-test-hold-discovery','1')")
    page.reload()
    page.wait_for_function("() => !document.querySelector('#chat-input').disabled")
    page.locator('#chat-input').fill(query)
    assert page.locator('#mention-pop').is_hidden()
    if dismiss:
        page.locator('#chat-input').press('Escape')
    page.evaluate('() => window.__ICU_RELEASE_DISCOVERY__()')
    if dismiss:
        assert page.locator('#mention-pop').is_hidden(), 'late discovery reopened a dismissed popover'
    else:
        page.locator('#mention-pop .mention-row', has_text=name).wait_for(state='visible', timeout=5000)


@pytest.mark.parametrize('reference', ['UnresolvedWindow', ''])
def test_unresolved_reference_never_drives_a_different_watched_window(page, reference):
    page.locator('#add-window').click()
    page.locator('#picker-list .pick-row', has_text='TextEdit').click()
    draft = f'in @{reference} type hello'
    page.locator('#chat-input').fill(draft)
    page.locator('#chat-form').evaluate('form => form.requestSubmit()')
    page.wait_for_timeout(100)
    assert not _calls(page, 'agent_run'), 'unresolved reference silently selected TextEdit'
    assert draft == page.locator('#chat-input').input_value()


def test_selected_name_with_at_sign_remains_a_resolvable_reference(page):
    page.evaluate("() => localStorage.setItem('icu-test-window-name','Review@Tool')")
    page.reload()
    inp = page.locator('#chat-input')
    inp.fill('in @Review')
    page.locator('#mention-pop .mention-row', has_text='Review@Tool').dispatch_event('mousedown')
    inp.type('type hello')
    page.locator('#chat-form').evaluate('form => form.requestSubmit()')
    page.wait_for_function("() => window.__ICU_CALLS__.some(c => c.cmd === 'agent_run')")
    args = _calls(page, 'agent_run')[-1]['args']
    assert (args['pid'], args['windowId']) == (700, 1452)
    assert 'Review@Tool' in args['goal']


def test_selecting_same_name_window_does_not_rebind_existing_reference(page):
    page.evaluate("() => localStorage.setItem('icu-test-extra-windows', JSON.stringify([{title:'second.txt',window_id:1453}]))")
    page.reload()
    inp = page.locator('#chat-input')
    inp.fill('@TextEdit')
    page.locator('#mention-pop .mention-row', has_text='notes.txt').dispatch_event('mousedown')
    first_reference = inp.input_value().strip()
    inp.type('@TextEdit')
    page.locator('#mention-pop .mention-row', has_text='second.txt').dispatch_event('mousedown')
    both_references = inp.input_value()
    second_reference = both_references.split()[-1]
    page.locator('#chat-form').evaluate('form => form.requestSubmit()')
    page.wait_for_timeout(100)
    assert not _calls(page, 'agent_run'), 'two selected windows must not silently become one target'
    assert inp.input_value() == both_references
    inp.fill(first_reference + ' type hello')
    page.locator('#chat-form').evaluate('form => form.requestSubmit()')
    page.wait_for_function("() => window.__ICU_CALLS__.some(c => c.cmd === 'agent_run')")
    args = _calls(page, 'agent_run')[-1]['args']
    assert (args['pid'], args['windowId']) == (700, 1452), f'first reference rebound after selecting second window: {both_references!r}'
    page.evaluate("runId => window.__ICU_EMIT__({runId,kind:'done',data:{summary:'finished'}})", args['runId'])
    inp.fill(second_reference + ' type goodbye')
    page.locator('#chat-form').evaluate('form => form.requestSubmit()')
    page.wait_for_function("() => window.__ICU_CALLS__.filter(c => c.cmd === 'agent_run').length === 2")
    args = _calls(page, 'agent_run')[-1]['args']
    assert (args['pid'], args['windowId']) == (700, 1453)


def test_delayed_apps_do_not_reset_keyboard_window_selection(page):
    page.evaluate("""() => {
      localStorage.setItem('icu-test-hold-apps', '1');
      localStorage.setItem('icu-test-extra-windows', JSON.stringify([{title:'second.txt',window_id:1453}]));
    }""")
    page.reload()
    inp = page.locator('#chat-input')
    inp.fill('@')
    page.locator('#mention-pop .mention-row', has_text='second.txt').wait_for(state='visible')
    inp.press('ArrowDown')
    assert 'second.txt' in page.locator('#mention-pop .sel').inner_text()
    page.evaluate('() => window.__ICU_RELEASE_DISCOVERY__()')
    page.locator('#mention-pop .mention-launch', has_text='Calculator').wait_for(state='visible')
    assert 'second.txt' in page.locator('#mention-pop .sel').inner_text(), 'discovery changed the selected target'
    inp.press('Enter')
    inp.type('type hello')
    page.locator('#chat-form').evaluate('form => form.requestSubmit()')
    page.wait_for_function("() => window.__ICU_CALLS__.some(c => c.cmd === 'agent_run')")
    args = _calls(page, 'agent_run')[-1]['args']
    assert (args['pid'], args['windowId']) == (700, 1453)


@pytest.mark.parametrize('removed', [False, True])
def test_discovery_preserves_target_identity_across_reorder_or_removal(page, removed):
    page.evaluate("() => localStorage.setItem('icu-test-extra-windows', JSON.stringify([{title:'second.txt',window_id:1453}]))")
    page.reload()
    inp = page.locator('#chat-input')
    # First settle discovery, then hold the refresh triggered by the real @ key.
    inp.fill('@')
    page.locator('#mention-pop .mention-launch', has_text='Calculator').wait_for(state='visible')
    inp.press('Escape')
    inp.fill('')
    page.evaluate("() => localStorage.setItem('icu-test-hold-discovery','1')")
    inp.type('@')
    inp.press('ArrowDown')
    assert 'second.txt' in page.locator('#mention-pop .sel').inner_text()
    page.evaluate("removed => removed ? window.__ICU_WINDOWS__.pop() : window.__ICU_WINDOWS__.reverse()", removed)
    page.evaluate('() => window.__ICU_RELEASE_DISCOVERY__()')
    if removed:
        assert page.locator('#mention-pop .sel').count() == 0
        inp.press('Enter')
        assert inp.input_value() == '@', 'disappeared selection was silently replaced'
        assert not _calls(page, 'agent_run')
        inp.press('ArrowDown')
        assert 'notes.txt' in page.locator('#mention-pop .sel').inner_text()
    else:
        assert 'second.txt' in page.locator('#mention-pop .sel').inner_text()
    inp.press('Enter')
    inp.type('type hello')
    page.locator('#chat-form').evaluate('form => form.requestSubmit()')
    page.wait_for_function("() => window.__ICU_CALLS__.some(c => c.cmd === 'agent_run')")
    args = _calls(page, 'agent_run')[-1]['args']
    assert args['windowId'] == (1452 if removed else 1453)


@pytest.mark.parametrize('changed', [False, True])
def test_delayed_launch_respects_current_command(page, changed):
    page.evaluate("() => localStorage.setItem('icu-test-hold-launch', '1')")
    inp = page.locator('#chat-input')
    inp.fill('@Calc')
    page.locator('#mention-pop .mention-launch', has_text='Calculator').dispatch_event('mousedown')
    page.wait_for_function("() => window.__ICU_CALLS__.some(c => c.cmd === 'cua_launch_app')")
    if changed:
        inp.fill('@Text')
        page.locator('#mention-pop .mention-row', has_text='notes.txt').dispatch_event('mousedown')
        inp.type('type hello')
    draft = inp.input_value()
    page.evaluate('() => window.__ICU_RELEASE_LAUNCH__()')
    if changed:
        assert inp.input_value() == draft, 'late launch rewrote a newer window reference'
    else:
        assert inp.input_value() == '@Calculator '
        inp.type('type hello')
    page.locator('#chat-form').evaluate('form => form.requestSubmit()')
    page.wait_for_function("() => window.__ICU_CALLS__.some(c => c.cmd === 'agent_run')")
    args = _calls(page, 'agent_run')[-1]['args']
    assert args['windowId'] == (1452 if changed else 9001)


@pytest.mark.parametrize('change', ['caret', 'escape', 'focus', 'edit_restore'])
def test_delayed_launch_does_not_reclaim_cancelled_input(page, change):
    page.evaluate("() => localStorage.setItem('icu-test-hold-launch', '1')")
    inp = page.locator('#chat-input')
    inp.fill('@Calc')
    page.locator('#mention-pop .mention-launch', has_text='Calculator').dispatch_event('mousedown')
    page.wait_for_function("() => window.__ICU_CALLS__.some(c => c.cmd === 'cua_launch_app')")
    if change == 'caret':
        inp.press('ArrowLeft')
    elif change == 'escape':
        inp.press('Escape')
    elif change == 'focus':
        page.locator('#add-window').click()
    else:
        inp.fill('another draft')
        inp.fill('@Calc')
    before = page.evaluate("() => ({active:document.activeElement.id, caret:document.querySelector('#chat-input').selectionStart})")
    page.evaluate('() => window.__ICU_RELEASE_LAUNCH__()')
    assert inp.input_value() == '@Calc'
    assert page.evaluate("() => ({active:document.activeElement.id, caret:document.querySelector('#chat-input').selectionStart})") == before
    assert not _calls(page, 'agent_run')


def test_manual_typing_waits_for_preceding_click(page):
    page.evaluate("() => localStorage.setItem('icu-test-manual','1')")
    page.locator('#add-window').click()
    page.locator('#picker-list .pick-row', has_text='TextEdit').click()
    _click_tile(page)
    page.wait_for_function("() => window.__ICU_CALLS__.some(c => c.cmd === 'cua_click')")
    page.keyboard.type('ab')
    page.evaluate('() => window.__ICU_RELEASE_CLICK__()')
    page.wait_for_function("() => window.__ICU_MANUAL__.before + window.__ICU_MANUAL__.after === 'ab'")
    effect = page.evaluate('() => window.__ICU_MANUAL__')
    assert effect == {'selected':'after','before':'','after':'ab'}, 'typing overtook its pending click'


@pytest.mark.parametrize('outcome', ['failure', 'escape', 'remove'])
def test_pending_manual_typing_is_discarded_after_failure_or_release(page, outcome):
    page.evaluate("() => localStorage.setItem('icu-test-manual','1')")
    page.locator('#add-window').click()
    page.locator('#picker-list .pick-row', has_text='TextEdit').click()
    _click_tile(page)
    page.wait_for_function("() => window.__ICU_CALLS__.some(c => c.cmd === 'cua_click')")
    page.keyboard.type('ab')
    if outcome == 'escape':
        page.keyboard.press('Escape')
    elif outcome == 'remove':
        page.locator('.tile button.x').click()
    page.evaluate('fail => window.__ICU_RELEASE_CLICK__(fail)', outcome == 'failure')
    if outcome == 'failure':
        page.locator('.tile-status', has_text='held click failed').wait_for(state='visible')
    assert not _calls(page, 'cua_type'), 'cancelled typing was still dispatched'
    effect = page.evaluate('() => window.__ICU_MANUAL__')
    assert effect['before'] == effect['after'] == ''


def test_switching_manual_target_discards_old_queued_typing(page):
    page.evaluate("""() => {
      localStorage.setItem('icu-test-manual','1');
      localStorage.setItem('icu-test-extra-windows', JSON.stringify([{title:'second.txt',window_id:1453}]));
    }""")
    page.reload()
    page.locator('#add-window').click()
    page.locator('#picker-list .pick-row', has_text='notes.txt').click()
    _click_tile(page)
    page.wait_for_function("() => window.__ICU_CALLS__.some(c => c.cmd === 'cua_click')")
    page.keyboard.type('ab')
    page.locator('#add-window').click()
    page.locator('#picker-list .pick-row', has_text='second.txt').click()
    page.locator('.tile[data-k="700:1453"] button.ctl').click()
    page.keyboard.type('c')
    page.evaluate('() => window.__ICU_RELEASE_CLICK__()')
    page.wait_for_function("() => window.__ICU_MANUAL__.after === 'c'")
    assert [c['args'] for c in _calls(page, 'cua_type')] == [{'pid':700,'windowId':1453,'text':'c'}]


def test_task_waits_for_manual_input_without_losing_its_draft(page):
    page.evaluate("() => localStorage.setItem('icu-test-manual','1')")
    page.locator('#add-window').click()
    page.locator('#picker-list .pick-row', has_text='TextEdit').click()
    _click_tile(page)
    page.wait_for_function("() => window.__ICU_CALLS__.some(c => c.cmd === 'cua_click')")
    inp = page.locator('#chat-input')
    inp.fill('type hello')
    page.locator('#chat-form').evaluate('form => form.requestSubmit()')
    assert not _calls(page, 'agent_run')
    assert inp.input_value() == 'type hello'
    page.evaluate('() => window.__ICU_RELEASE_CLICK__()')
    page.locator('#chat-form').evaluate('form => form.requestSubmit()')
    page.wait_for_function("() => window.__ICU_CALLS__.some(c => c.cmd === 'agent_run')")


@pytest.mark.parametrize('composing,key_code', [(True, 13), (False, 229)])
def test_composition_enter_does_not_select_a_window_suggestion(page, composing, key_code):
    inp = page.locator('#chat-input')
    inp.fill('@Text')
    page.locator('#mention-pop .mention-row', has_text='TextEdit').wait_for(state='visible')
    allowed = inp.evaluate("""(el, args) => el.dispatchEvent(new KeyboardEvent('keydown', {
      key:'Enter', isComposing:args.composing, keyCode:args.key_code,
      bubbles:true, cancelable:true
    }))""", {'composing':composing,'key_code':key_code})
    assert inp.input_value() == '@Text', 'IME Enter selected a window instead of committing composition'
    assert allowed, 'mention handler consumed the composition key'
    assert not _calls(page, 'agent_run')


def _solid_frame(width, height):
    def chunk(kind, data):
        return struct.pack('!I',len(data)) + kind + data + struct.pack('!I',zlib.crc32(kind + data))
    scanlines = (b'\x00' + b'\xff\xff\xff' * width) * height
    return b'\x89PNG\r\n\x1a\n' + chunk(b'IHDR',struct.pack('!IIBBBBB',width,height,8,2,0,0,0)) + chunk(b'IDAT',zlib.compress(scanlines)) + chunk(b'IEND',b'')


@pytest.mark.parametrize('window_size', [(1280, 960), (320, 240)])
def test_pointer_coordinates_use_decoded_screenshot_pixels(page, window_size):
    page._frame_png = _solid_frame(640, 480)
    page.evaluate("""size => {
      localStorage.setItem('icu-test-manual','1');
      localStorage.setItem('icu-test-window-bounds', JSON.stringify({width:size[0],height:size[1]}));
    }""", window_size)
    page.reload()
    page.locator('#add-window').click()
    page.locator('#picker-list .pick-row', has_text='TextEdit').click()
    shot = page.locator('.tile img.shot')
    shot.wait_for(state='visible')
    assert shot.evaluate('img => [img.naturalWidth,img.naturalHeight]') == [640,480]
    page.locator('.tile button.ctl').click()
    shot.click()
    page.wait_for_function("() => window.__ICU_CALLS__.some(c => c.cmd === 'cua_click')")
    click = _calls(page, 'cua_click')[-1]['args']
    page.evaluate('() => window.__ICU_RELEASE_CLICK__()')
    shot.hover()
    page.mouse.wheel(0, 120)
    page.wait_for_function("() => window.__ICU_CALLS__.some(c => c.cmd === 'cua_scroll')")
    scroll = _calls(page, 'cua_scroll')[-1]['args']
    assert abs(click['x'] - 320) <= 1 and abs(click['y'] - 240) <= 1, 'click used logical window size instead of decoded screenshot size'
    assert abs(scroll['x'] - 320) <= 1 and abs(scroll['y'] - 240) <= 1, 'wheel used logical window size instead of decoded screenshot size'


@pytest.mark.parametrize('engage', ['before_run', 'during_run'])
def test_agent_handoff_does_not_leave_manual_keyboard_control(page, engage):
    page.evaluate("() => localStorage.setItem('icu-test-manual','1')")
    page.locator('#add-window').click()
    page.locator('#picker-list .pick-row', has_text='TextEdit').click()
    if engage == 'before_run':
        page.locator('.tile button.ctl').click()
    inp = page.locator('#chat-input')
    inp.fill('complete the task')
    page.locator('#chat-form').evaluate('form => form.requestSubmit()')
    page.wait_for_function("() => window.__ICU_CALLS__.some(c => c.cmd === 'agent_run')")
    run_id = _calls(page, 'agent_run')[-1]['args']['runId']
    assert page.locator('.tile button.ctl').is_disabled()
    if engage == 'during_run':
        _click_tile(page)
        assert not _calls(page, 'cua_click'), 'manual pointer input was admitted during a task'
    page.evaluate("runId => window.__ICU_EMIT__({runId,kind:'done',data:{summary:'finished'}})", run_id)
    # Disabling the command input for the run leaves focus outside the textbox.
    assert page.evaluate('() => document.activeElement.tagName') not in ['INPUT','TEXTAREA','SELECT']
    page.keyboard.type('next')
    page.wait_for_timeout(100)
    assert not _calls(page, 'cua_type'), 'old manual control captured typing after the agent task ended'
    assert page.locator('.tile.controlling').count() == 0
    page.locator('.tile button.ctl').click()
    page.keyboard.type('ok')
    page.wait_for_function("() => window.__ICU_CALLS__.filter(c => c.cmd === 'cua_type').length === 2")
    assert [c['args']['text'] for c in _calls(page, 'cua_type')] == ['o','k']


@pytest.mark.parametrize('flags', [{'isComposing': True}, {'keyCode': 229}])
def test_manual_control_leaves_composition_keys_to_the_input_method(page, flags):
    page.evaluate("() => localStorage.setItem('icu-test-manual','1')")
    page.locator('#add-window').click()
    page.locator('#picker-list .pick-row', has_text='TextEdit').click()
    page.locator('.tile button.ctl').click()
    prevented = page.evaluate("""async flags => {
      const prevented = [];
      for (const key of ['x','Enter','Escape']) {
        const event = new KeyboardEvent('keydown', {key, bubbles:true, cancelable:true, ...flags});
        window.dispatchEvent(event);
        prevented.push(event.defaultPrevented);
        await new Promise(resolve => setTimeout(resolve, 0));
      }
      return prevented;
    }""", flags)
    assert not _calls(page, 'cua_type'), 'uncommitted composition text was forwarded to the target'
    assert not _calls(page, 'cua_press_key'), 'composition confirmation was forwarded as a target command'
    assert prevented == [False, False, False], 'composition was cancelled by the manual key handler'
    assert page.locator('.tile.controlling').count() == 1, 'IME Escape unexpectedly released manual control'
    page.keyboard.press('Enter')
    page.wait_for_function("() => window.__ICU_CALLS__.some(c => c.cmd === 'cua_press_key')")
    assert [c['args']['key'] for c in _calls(page, 'cua_press_key')] == ['return']
    page.keyboard.press('Escape')
    assert page.locator('.tile.controlling').count() == 0


def _commit_manual_text(page, method, text):
    if method == 'paste':
        return page.evaluate("""text => {
          const data = new DataTransfer(); data.setData('text/plain', text);
          const event = new ClipboardEvent('paste', {clipboardData:data, bubbles:true, cancelable:true});
          document.activeElement.dispatchEvent(event);
          return event.defaultPrevented;
        }""", text)
    session = page.context.new_cdp_session(page)
    try:
        if method == 'ime':
            session.send('Input.imeSetComposition', {'text':'にほん', 'selectionStart':0, 'selectionEnd':3})
            assert not _calls(page, 'cua_type'), 'provisional IME text escaped before commit'
        session.send('Input.insertText', {'text':text})
    finally:
        session.detach()


@pytest.mark.parametrize('method,text', [
    ('ime', '日本語'), ('insert', '🙂 café'), ('paste', '  first\nsecond\t🙂  '),
])
def test_manual_committed_text_waits_for_its_click_and_arrives_once(page, method, text):
    page.evaluate("() => localStorage.setItem('icu-test-manual','1')")
    page.locator('#add-window').click()
    page.locator('#picker-list .pick-row', has_text='TextEdit').click()
    _click_tile(page)
    page.wait_for_function("() => window.__ICU_CALLS__.some(c => c.cmd === 'cua_click')")
    _commit_manual_text(page, method, text)
    assert not _calls(page, 'cua_type'), 'committed text overtook its pending click'
    page.evaluate('() => window.__ICU_RELEASE_CLICK__()')
    page.wait_for_function('text => window.__ICU_MANUAL__.after === text', arg=text, timeout=2000)
    assert page.evaluate('() => window.__ICU_MANUAL__.before') == ''
    assert [c['args'] for c in _calls(page, 'cua_type')] == [{'pid':700, 'windowId':1452, 'text':text}]
    assert page.locator('.action-flash').inner_text() == 'type', 'committed content leaked into local feedback'


@pytest.mark.parametrize('method', ['ime', 'paste'])
@pytest.mark.parametrize('outcome', ['failure', 'escape', 'remove'])
def test_committed_manual_text_is_cancelled_with_its_control(page, method, outcome):
    page.evaluate("() => localStorage.setItem('icu-test-manual','1')")
    page.locator('#add-window').click()
    page.locator('#picker-list .pick-row', has_text='TextEdit').click()
    _click_tile(page)
    page.wait_for_function("() => window.__ICU_CALLS__.some(c => c.cmd === 'cua_click')")
    _commit_manual_text(page, method, '日本語')
    if outcome == 'escape':
        page.keyboard.press('Escape')
    elif outcome == 'remove':
        page.locator('.tile button.x').click()
    page.evaluate('fail => window.__ICU_RELEASE_CLICK__(fail)', outcome == 'failure')
    if outcome == 'failure':
        page.locator('.tile-status', has_text='held click failed').wait_for(state='visible')
    page.wait_for_timeout(50)
    assert not _calls(page, 'cua_type')
    assert page.locator('.tile.controlling').count() == 0


def test_late_composition_after_a_failed_click_cannot_reenter_the_queue(page):
    page.evaluate("() => localStorage.setItem('icu-test-manual','1')")
    page.locator('#add-window').click()
    page.locator('#picker-list .pick-row', has_text='TextEdit').click()
    _click_tile(page)
    page.wait_for_function("() => window.__ICU_CALLS__.some(c => c.cmd === 'cua_click')")
    editor = page.locator('.manual-text-input').element_handle()
    editor.evaluate("el => { el.dispatchEvent(new CompositionEvent('compositionstart')); el.value='日本語'; }")
    page.evaluate('() => window.__ICU_RELEASE_CLICK__(true)')
    page.locator('.tile-status', has_text='held click failed').wait_for(state='visible')
    editor.evaluate("el => el.dispatchEvent(new CompositionEvent('compositionend', {data:'日本語'}))")
    page.wait_for_timeout(50)
    assert not _calls(page, 'cua_type'), 'failed-click generation accepted a later IME commit'


@pytest.mark.parametrize('handoff', ['switch', 'task', 'prompt', 'gesture'])
def test_old_composition_cannot_follow_a_control_handoff(page, handoff):
    page.evaluate("""() => {
      localStorage.setItem('icu-test-manual','1');
      localStorage.setItem('icu-test-extra-windows', JSON.stringify([{title:'second.txt',window_id:1453}]));
    }""")
    page.reload()
    page.locator('#add-window').click()
    page.locator('#picker-list .pick-row', has_text='notes.txt').click()
    page.locator('.tile button.ctl').click()
    editor = page.locator('.manual-text-input').element_handle()
    session = page.context.new_cdp_session(page)
    session.send('Input.imeSetComposition', {'text':'にほん', 'selectionStart':0, 'selectionEnd':3})
    session.detach()
    if handoff == 'switch':
        page.locator('#add-window').click()
        page.locator('#picker-list .pick-row', has_text='second.txt').click()
        page.locator('.tile[data-k="700:1453"] button.ctl').click()
    elif handoff == 'gesture':
        _click_tile(page)
        page.wait_for_function("() => window.__ICU_CALLS__.some(c => c.cmd === 'cua_click')")
    else:
        page.locator('#chat-input').fill('do the task')
        if handoff == 'task':
            page.locator('#chat-form').evaluate('form => form.requestSubmit()')
            page.wait_for_function("() => window.__ICU_CALLS__.some(c => c.cmd === 'agent_run')")
    editor.evaluate("""el => {
      el.value='日本語';
      el.dispatchEvent(new CompositionEvent('compositionend', {data:'日本語'}));
      el.dispatchEvent(new InputEvent('input', {data:'日本語',inputType:'insertFromComposition'}));
    }""")
    page.wait_for_timeout(50)
    assert not _calls(page, 'cua_type'), 'old composition was delivered after a handoff'
    if handoff in ['switch', 'gesture']:
        _commit_manual_text(page, 'insert', 'fresh🙂')
        if handoff == 'gesture':
            page.evaluate('() => window.__ICU_RELEASE_CLICK__()')
        page.wait_for_function("() => window.__ICU_CALLS__.some(c => c.cmd === 'cua_type')")
        window_id = 1453 if handoff == 'switch' else 1452
        assert [c['args'] for c in _calls(page, 'cua_type')] == [{'pid':700,'windowId':window_id,'text':'fresh🙂'}]
    elif handoff == 'prompt':
        _commit_manual_text(page, 'insert', '🙂')
        assert page.locator('#chat-input').input_value() == 'do the task🙂'
        assert not _calls(page, 'cua_type')


def test_cancelled_ime_leaves_no_text_for_the_next_commit(page):
    page.evaluate("() => localStorage.setItem('icu-test-manual','1')")
    page.locator('#add-window').click()
    page.locator('#picker-list .pick-row', has_text='TextEdit').click()
    page.locator('.tile button.ctl').click()
    session = page.context.new_cdp_session(page)
    try:
        session.send('Input.imeSetComposition', {'text':'にほん', 'selectionStart':0, 'selectionEnd':3})
        session.send('Input.imeSetComposition', {'text':'', 'selectionStart':0, 'selectionEnd':0})
        page.wait_for_timeout(50)
        assert not _calls(page, 'cua_type')
        session.send('Input.insertText', {'text':'fresh🙂'})
    finally:
        session.detach()
    page.wait_for_function("() => window.__ICU_CALLS__.some(c => c.cmd === 'cua_type')")
    assert [c['args']['text'] for c in _calls(page, 'cua_type')] == ['fresh🙂']


def test_compositionend_before_final_input_sends_only_the_final_value(page):
    page.evaluate("() => localStorage.setItem('icu-test-manual','1')")
    page.locator('#add-window').click()
    page.locator('#picker-list .pick-row', has_text='TextEdit').click()
    page.locator('.tile button.ctl').click()
    page.locator('.manual-text-input').evaluate("""el => {
      el.dispatchEvent(new CompositionEvent('compositionstart'));
      el.value='に';
      el.dispatchEvent(new InputEvent('input', {data:'に',isComposing:true,inputType:'insertCompositionText'}));
      el.dispatchEvent(new CompositionEvent('compositionend', {data:'日本語'}));
      el.value='日本語';
      el.dispatchEvent(new InputEvent('input', {data:'日本語',inputType:'insertFromComposition'}));
    }""")
    page.wait_for_timeout(50)
    assert [c['args']['text'] for c in _calls(page, 'cua_type')] == ['日本語']


def test_memory_panel_shows_state_pauses_and_deletes(page):
    """The chip shows the memory state; the panel pauses, resumes and deletes one memory."""
    chip = page.locator("#memory-chip")
    page.wait_for_function("() => document.querySelector('#memory-text').textContent === 'Memory on'")
    chip.click()
    page.locator("#memory-list .mem-row", has_text="document titles in all caps").wait_for(state="visible")
    assert page.locator("#memory-list").inner_text().count("observed") == 2
    assert "only the local model answered is summarized on the local model" in page.locator("#mem-provider-note").inner_text(), "the panel says where memory is written"
    if os.environ.get("ICU_GUI_SHOT"):
        page.set_viewport_size({"width": 1280, "height": 820})
        page.wait_for_timeout(400)
        page.screenshot(path=os.environ["ICU_GUI_SHOT"])
    page.locator("#memory-pause").click()
    page.wait_for_function("() => document.querySelector('#memory-text').textContent === 'Memory paused'")
    assert _calls(page, "memory_pause")[-1]["args"] == {"paused": True}
    assert page.locator("#memory-pause").inner_text() == "Resume"
    # With the provider setting named, the note says every pass runs there.
    page.evaluate("() => { window.__ICU_MEM__.settings.provider = 'claude-code'; }")
    page.locator("#memory-pause").click()
    page.wait_for_function("() => document.querySelector('#mem-provider-note').textContent.startsWith('Every memory pass runs on claude-code')")
    page.locator('#memory-list [data-del="e-1"]').click()
    page.wait_for_function("() => !document.querySelector('#memory-list').textContent.includes('document titles in all caps')")
    assert _calls(page, "memory_delete")[-1]["args"] == {"id": "e-1"}
    page.locator("#mem-retention").fill("0")
    page.locator("#mem-save").click()
    assert "1 to 3650" in page.locator("#mem-note").inner_text()
    assert not _calls(page, "memory_settings_set")
    # A save sends only what the form changed, so another instance's pause is not written back.
    page.locator("#mem-retention").fill("20")
    page.locator("#mem-save").click()
    page.wait_for_function("() => document.querySelector('#mem-note').textContent === 'Saved.'")
    assert _calls(page, "memory_settings_set")[-1]["args"] == {"settings": {"retentionDays": 20}}


def test_run_carries_memory_switches_and_logs_what_was_used(page):
    page.locator("#run-remember").uncheck()
    a = _submit_goal(page)
    assert a.get("useMemory") is True and a.get("remember") is False, a
    page.evaluate("""id => window.__ICU_EMIT__({runId:id,kind:'memory',data:{state:'used',chars:60,
        used:[{id:'e-1',scope:'com.apple.iWork.Pages',kind:'preference',text:'The user wants <i>caps</i>.',untrusted:true}]}})""", a["runId"])
    log = page.locator("#action-log")
    log.locator("text=given to the model as background").wait_for()
    assert "preference · observed: The user wants <i>caps</i>." in log.inner_text()
    assert log.locator("i").count() == 0, "memory text is escaped"


def test_onboarding_prompts_for_a_key_and_saves_it(page):
    """A fresh launch with no model configured shows the onboarding overlay;
    entering a key calls set_model_key for the chosen provider and dismisses it."""
    page.evaluate("() => localStorage.setItem('icu-test-needs-onboarding','1')")
    page.reload()
    overlay = page.locator("#onboarding-overlay")
    overlay.wait_for(state="visible", timeout=10000)
    page.locator("#onboard-key").fill("sk-ant-smoke")
    page.locator("#onboard-save").click()
    page.wait_for_function("() => window.__ICU_CALLS__.some(c => c.cmd === 'set_model_key')", timeout=5000)
    args = _calls(page, "set_model_key")[-1]["args"]
    assert args["provider"] == "anthropic" and args["key"] == "sk-ant-smoke", args
    overlay.wait_for(state="hidden", timeout=5000)


def test_onboarding_hidden_when_a_model_is_configured(page):
    """With a model already configured, the first run shows no onboarding overlay."""
    page.wait_for_selector("#chat-input", timeout=10000)
    page.wait_for_function("() => window.__ICU_CALLS__.some(c => c.cmd === 'model_onboarding_status')", timeout=5000)
    page.wait_for_timeout(200)
    assert page.locator("#onboarding-overlay").is_hidden()
    assert not _calls(page, "set_model_key")
