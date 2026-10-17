import { toggleUtilityPanel, wireUtilityPanelKeyboard } from "./panel-navigation";
import { wireRunPreferences } from "./run-preferences";
import { takeForegroundChoice, resetForegroundChoice } from "./foreground-choice";
import { renderSavedTasks, type SavedTask } from "./task-progress-ui";
import { loadFrame, releaseFrame } from "./frames";
import { InputQueue } from "./input-queue";
import { ManualTextInput } from "./manual-text";
import { sendControl } from "./send-control";
import { deleteAllMemories } from "./inline-confirm";
import { invoke, convertFileSrc, Channel } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { check } from "@tauri-apps/plugin-updater";
import { findUpdate, fitsInstall, showUpdateToast, withOwnInstall, type InstallResult, type UpdateEvent } from "./updater";
import { relaunch } from "@tauri-apps/plugin-process";
import { confirmHtml, confirmedText, memoryChip, memoryLine, memoryListHtml, providerNote, runMemoryOptions, type MemoryList, type MemoryStatus, memoryStatusSuffix } from "./memory-ui";
import { memorySettingsForm } from "./memory-settings-form";
import { wireTrajectoryImport } from "./trajectory-ui";
import { wireWorkflows, type WorkflowInvocation } from "./workflow-ui";
import { windowLabel } from "./window-label";
import { wireAudio } from "./audio-ui";
import { wireHistory } from "./history-ui";
import { wireHistorySummaries } from "./history-summary-ui";
import { wireHistoryDeletion } from "./history-delete-ui";
import { wireMemorySources } from "./memory-source-ui";
import { wireOnboarding } from "./onboarding";
import { planKey, wireDrive, mentionQuery, applyMention, parseMentions, readableGoal, resolveMentionTarget, WindowReferences, launchableApps, permissionNotice, permissionErrorNotice, driverNotice, uiAccessNotice, actLine, malformedLine, type AppEntry, type InvokeFn, type PermissionStatus } from "./interact";

type Win = {
  app_name: string;
  title?: string;
  pid: number;
  window_id: number;
  on_current_space?: boolean;
  is_on_screen?: boolean;
  bounds: { x: number; y: number; width: number; height: number };
};

const $ = <T extends HTMLElement>(sel: string) => document.querySelector(sel) as T;
const feedEl = () => $("#feed");
const chatLog = () => $("#chat-log");
const actionLog = () => $("#action-log");
const sleep = (ms: number) => new Promise((r) => setTimeout(r, ms));

type Entry = { win: Win; tile: HTMLElement; visible: boolean; kind: "window" | "desktop"; gone?: number; display?: number };
let displayCount = 1; // how many monitors; >1 offers a whole-screen tile per display
// What the single whole-screen tile captures (Linux X11 captures every monitor).
let desktopLabel: [string, string] = ["Main display", "primary screen"];
const controlled = new Map<string, Entry>();
const key = (w: { pid: number; window_id: number }) => `${w.pid}:${w.window_id}`;
let activeAgent: { key: string; win: Win; runId: string; starting: boolean; stopRequested: boolean } | null = null;
let agentEventsReady: Promise<unknown> | undefined;
let taskStateReady = false;
let taskStateError = false;
let restoringTask = false;
const terminalRunIds = new Set<string>();
// The one window tile currently taking your click/type/scroll passthrough.
let controllingKey: string | null = null;
const manualInputs = new InputQueue();
let manualTextInput: ManualTextInput | null = null;

const io = new IntersectionObserver(
  (ents) => {
    for (const e of ents) {
      const c = controlled.get((e.target as HTMLElement).dataset.k!);
      if (c) c.visible = e.isIntersecting;
    }
  },
  { root: feedEl(), threshold: 0.05 },
);

const frameUrl = (c: Entry) => {
  if (c.kind === "desktop") {
    // Display 1 uses the daemon (grant-free); a secondary display uses
    // screencapture -D<n> (the only path that can address a non-primary monitor).
    return c.display && c.display > 1
      ? `${convertFileSrc("", "frame")}display/${c.display}/1100?t=${Date.now()}`
      : `${convertFileSrc("", "frame")}desktop/1100?t=${Date.now()}`;
  }
  return `${convertFileSrc("", "frame")}win/${c.win.pid}/${c.win.window_id}/640?t=${Date.now()}`;
};

let loopRunning = false;
async function captureLoop() {
  if (loopRunning) return;
  loopRunning = true;
  while (controlled.size > 0) {
    if (document.hidden) { await sleep(1000); continue; }
    let captured = 0;
    for (const [k, c] of [...controlled]) {
      if (!controlled.has(k) || !c.visible || document.hidden) continue;
      if (c.tile.classList.contains("dead")) continue; // window closed
      const img = c.tile.querySelector("img.shot") as HTMLImageElement;
      const err = c.tile.querySelector(".err") as HTMLElement;
      const cap = c.tile.querySelector(".capturing") as HTMLElement | null;
      const ok = await loadFrame(img, frameUrl(c));
      if (controlled.get(k) !== c) { releaseFrame(img); continue; }
      if (ok) {
        img.hidden = false;
        err.hidden = true;
        if (cap) cap.hidden = true;
        img.dataset.ok = "1";
      } else if (img.dataset.frameRefusal) {
        // The window exists but has no frame right now, for a stated reason.
        // Showing the last good frame would pass off stale pixels as live.
        img.hidden = true;
        if (cap) cap.hidden = true;
        err.hidden = false;
        err.textContent = img.dataset.frameRefusal;
      } else if (!img.dataset.ok) {
        // no good frame yet. keep the neutral "capturing…" placeholder, not a
        // broken image, unless the capture is genuinely refused.
        img.hidden = true;
        if (cap) cap.hidden = true;
        err.hidden = false;
        err.textContent = "capture unavailable (off-Space or refused)";
      }
      captured++;
      await sleep(40);
    }
    await sleep(captured ? 250 : 700);
  }
  loopRunning = false;
}

// Keep watched window metadata fresh and notice closure. Pointer coordinates
// come from the decoded image dimensions, not these independently polled bounds.
let refreshingMeta = false;
async function refreshControlledMeta() {
  if (refreshingMeta || document.hidden) return;
  refreshingMeta = true;
  try {
    let wins: Win[];
    try {
      const res = await invoke<{ windows: Win[] }>("cua_list_windows");
      wins = res.windows ?? [];
      setDriverDot(true); // list_windows succeeded → driver is live (keeps the dot honest)
    } catch {
      setDriverDot(false);
      return;
    }
    if (![...controlled.values()].some((c) => c.kind === "window")) return;
    const byKey = new Map(wins.map((w) => [key(w), w]));
    for (const [k, c] of controlled) {
      if (c.kind !== "window") continue;
      const fresh = byKey.get(k);
      if (fresh) {
        c.win.bounds = fresh.bounds;
        c.win.on_current_space = fresh.on_current_space;
        c.gone = 0;
        if (c.tile.classList.contains("dead")) reviveTile(c);
        updateTileSpace(c, fresh.on_current_space !== false);
      } else if ((c.gone = (c.gone ?? 0) + 1) >= 3) {
        markClosed(c); // gone for ~3 scans running: treat as closed
      }
    }
  } finally { refreshingMeta = false; }
}

function offSpaceBadgeHtml() {
  // Clickable: bringing an off-Space window over is the ONE deliberate focus move,
  // and only happens when the user clicks it. nothing steals focus implicitly.
  return `<button class="badge activate" title="On another Space. Click to bring this window to the current Space.">off-Space</button>`;
}

function updateTileSpace(c: Entry, onCur: boolean) {
  const head = c.tile.querySelector(".tile-head");
  if (!head) return;
  const badge = head.querySelector(".badge") as HTMLElement | null;
  if (!onCur && !badge) {
    const tmp = document.createElement("template");
    tmp.innerHTML = offSpaceBadgeHtml();
    head.insertBefore(tmp.content.firstChild!, head.querySelector("button.ctl"));
  } else if (onCur && badge) {
    badge.remove();
  }
}

// The only path that intentionally moves focus, and only on an explicit click.
async function activateTile(c: Entry, btn: HTMLElement) {
  const label = btn.textContent;
  btn.textContent = "activating…";
  try {
    const r = await manualInputs.enqueue(() => invoke<{ on_current_space: boolean }>("cua_activate", {
      pid: c.win.pid, windowId: c.win.window_id, appName: c.win.app_name,
    }));
    if (r?.on_current_space) {
      c.win.on_current_space = true;
      updateTileSpace(c, true);
      logAction(`<b>activate</b> ${escapeHtml(c.win.app_name)}`);
    } else {
      btn.textContent = label;
    }
  } catch {
    btn.textContent = label;
  }
}

function markClosed(c: Entry) {
  if (c.tile.classList.contains("dead")) return;
  c.tile.classList.add("dead");
  if (controllingKey === key(c.win)) setControlling(null);
  (c.tile.querySelector("img.shot") as HTMLImageElement).hidden = true;
  const err = c.tile.querySelector(".err") as HTMLElement;
  err.hidden = false;
  err.textContent = "window closed · remove this tile";
}

function reviveTile(c: Entry) {
  c.tile.classList.remove("dead");
  (c.tile.querySelector(".err") as HTMLElement).hidden = true;
}

// Main-thread stall watchdog (taiku freeze-watchdog idea): a 1s self-check that
// only logs when a task blocked the event loop past a threshold. no spam.
function installStallWatchdog() {
  let last = performance.now();
  setInterval(() => {
    const now = performance.now();
    const drift = now - last - 1000;
    last = now;
    // 1.5-8s late = a real main-thread block worth flagging. A larger gap is the
    // OS suspending us (App Nap when our Space/display is backgrounded), not a
    // freeze. flag it separately, don't call it a stall (taiku's suspend-gap idea).
    if (document.hidden) return;
    if (drift > 1500 && drift < 8000) logAction(`<b>stall</b> main thread blocked ~${Math.round(drift)}ms`);
    else if (drift >= 8000) logAction(`<span class="muted">resumed after ${(drift / 1000).toFixed(0)}s suspended</span>`);
  }, 1000);
}

function logAction(html: string) {
  actionLog().querySelector(".empty")?.remove();
  const row = document.createElement("div");
  row.className = "row";
  row.innerHTML = `<time>${new Date().toLocaleTimeString([], { hour12: false })}</time>${html}`;
  actionLog().prepend(row);
  while (actionLog().childElementCount > 200) actionLog().lastElementChild?.remove();
}

function chat(role: "you" | "agent" | "sys", text: string) {
  const m = document.createElement("div");
  m.className = `msg ${role}`;
  m.textContent = text;
  chatLog().append(m);
  chatLog().scrollTop = chatLog().scrollHeight;
}

function feedEmpty() {
  if (!controlled.size)
    feedEl().innerHTML = `<div class="feed-empty">Add a window to watch and drive it.</div>`;
}

let driverWasDown = false;
function setDriverDot(ok: boolean) {
  const dot = $("#driver-dot"); const txt = $("#driver-text");
  dot.classList.toggle("ok", ok);
  dot.classList.toggle("err", !ok);
  txt.textContent = ok ? "driver connected" : "driver offline";
  if (!ok && !driverWasDown) {
    chat("sys", "Driver not reachable. Check the app permissions and bundled driver, then try again");
  }
  driverWasDown = !ok;
}

// macOS attributes the owned driver's Accessibility and Screen Recording use
// to this app, and the embedded driver never prompts. Say what is missing. When
// an external driver owns capture its grants are its own, so the status is read
// from the driver and shown as a quiet note, never a grant-the-app button.
function showPermissions(status: PermissionStatus | null) {
  const problem = permissionErrorNotice(status);
  const grant = permissionNotice(status);
  const text = problem ?? grant ?? driverNotice(status) ?? uiAccessNotice(status);
  let notice = document.querySelector<HTMLElement>("#permission-notice");
  if (!text) { notice?.remove(); return; }
  if (!notice) {
    notice = document.createElement("div");
    notice.id = "permission-notice";
    notice.className = "msg sys permission";
    notice.innerHTML = `<span class="perm-text"></span> <button type="button" class="perm-retry">Retry access</button>
      <button type="button" class="perm-grant">Grant access</button>
      <button type="button" class="perm-accessibility">Accessibility settings</button>
      <button type="button" class="perm-screen">Screen Recording settings</button>`;
    for (const button of notice.querySelectorAll<HTMLButtonElement>(".perm-grant, .perm-retry")) {
      button.addEventListener("click", async () => {
        // Only an explicit click may request grants or clear a capture denial.
        // The backend routes an external driver's retry without host prompts.
        button.disabled = true;
        try { showPermissions(await invoke<PermissionStatus>("cua_request_permissions")); }
        catch (error) { chat("sys", `Could not request access: ${error}`); }
        finally { button.disabled = false; }
      });
    }
    for (const [selector, pane] of [[".perm-accessibility", "accessibility"], [".perm-screen", "screen_recording"]]) {
      notice.querySelector(selector)!.addEventListener("click", async () => {
        try { await invoke("cua_open_privacy_settings", { pane }); }
        catch (error) { chat("sys", `Could not open privacy settings: ${error}`); }
      });
    }
    chatLog().prepend(notice);
  }
  notice.querySelector(".perm-text")!.textContent = text;
  // An explicit retry is available for either source. Only missing host grants
  // get the app grant and privacy-settings controls.
  notice.classList.toggle("quiet", !!problem || status?.source === "driver");
  notice.querySelector<HTMLElement>(".perm-retry")!.hidden = !problem;
  notice.querySelector<HTMLElement>(".perm-grant")!.hidden = !grant;
  notice.querySelector<HTMLElement>(".perm-accessibility")!.hidden = !grant || !!status?.accessibility;
  notice.querySelector<HTMLElement>(".perm-screen")!.hidden = !grant || !!status?.screen_recording;
  if (!grant && text !== lastQuietNotice) { lastQuietNotice = text; chat("sys", text); }
}
let lastQuietNotice: string | null = null;

async function checkPermissions() {
  try { showPermissions(await invoke<PermissionStatus>("cua_permissions")); }
  catch { /* status unavailable: say nothing rather than guess */ }
}

async function setDriverStatus() {
  try {
    await invoke<boolean>("cua_status");
    setDriverDot(true);
  } catch {
    setDriverDot(false);
  }
}

// The window picker is a searchable popover (opened by "+ window"), not a wall
// of chips. We cache the last scan so opening it is instant, then rescan live.
let lastWindows: Win[] = [];
// Set when the driver reports a class of windows it cannot list (native
// Wayland windows while its Wayland backend is off).
let windowCoverage: { code?: string; listed?: string; message?: string } | null = null;
let installedApps: AppEntry[] = [];

// Installed-app list for the @-bar's launch rows. Fetched once, lazily, the first
// time the mention popover opens; cheap and static enough not to poll.
async function loadApps(): Promise<void> {
  if (installedApps.length) return;
  try {
    const res = await invoke<{ apps: AppEntry[] }>("cua_list_apps");
    installedApps = res.apps ?? [];
    // loadApps is fired async as the popover opens; if it's still open once the
    // apps arrive, re-render so the launch rows appear without another keystroke.
    refreshRequestedMentions();
  } catch { /* daemon down. launch rows just won't show */ }
}

async function loadWindows(): Promise<void> {
  const list = $("#picker-list");
  if (!lastWindows.length) list.innerHTML = `<div class="pick-empty">scanning windows…</div>`;
  try {
    const res = await invoke<{ windows: Win[]; window_coverage?: typeof windowCoverage }>("cua_list_windows");
    lastWindows = res.windows ?? [];
    windowCoverage = res.window_coverage ?? null;
    renderPicker(($("#picker-search") as HTMLInputElement)?.value || "");
    refreshRequestedMentions();
  } catch (e) {
    list.innerHTML = `<div class="pick-empty">list failed: ${escapeHtml(String(e))}</div>`;
  }
}

function renderPicker(filter: string) {
  const list = $("#picker-list");
  const q = filter.trim().toLowerCase();
  list.innerHTML = "";
  for (let n = 1; n <= Math.max(1, displayCount); n++) {
    if (controlled.has(`display:${n}`)) continue;
    const label = displayCount > 1 ? `Display ${n}` : desktopLabel[0];
    if (q && !`${label} whole screen display monitor`.toLowerCase().includes(q)) continue;
    const row = pickRow("🖥", label, displayCount > 1 ? (n === 1 ? "primary screen" : "secondary screen") : desktopLabel[1], false);
    row.onclick = () => { addDisplay(n, label); closePicker(); };
    list.append(row);
  }
  let shown = 0;
  for (const w of lastWindows) {
    if (controlled.has(key(w))) continue;
    if (q && !`${w.app_name} ${w.title || ""}`.toLowerCase().includes(q)) continue;
    const row = pickRow("", w.app_name, windowLabel(w,lastWindows), w.on_current_space === false);
    row.onclick = () => { addWindow(w); closePicker(); };
    list.append(row);
    shown++;
  }
  if (!list.childElementCount)
    list.innerHTML = `<div class="pick-empty">${q ? `no windows match “${escapeHtml(filter)}”` : "no other windows open"}</div>`;
  const notice = coverageNotice(windowCoverage);
  if (notice) {
    const row = document.createElement("div");
    row.className = "pick-notice";
    row.setAttribute("role", "note");
    row.textContent = notice;
    list.append(row);
  }
}

function coverageNotice(coverage: typeof windowCoverage): string | null {
  if (!coverage) return null;
  if (coverage.code === "native_wayland_windows_not_listed")
    return coverage.listed === "none"
      ? "Native Wayland windows can't be listed, and this session has no XWayland, so no windows can be watched."
      : "Native Wayland windows can't be listed. Only apps running under XWayland show up here.";
  return coverage.message ?? null;
}

function pickRow(glyph: string, app: string, sub: string, off: boolean): HTMLButtonElement {
  const row = document.createElement("button");
  row.className = "pick-row";
  const badge = off ? `<span class="off">off-Space</span>` : "";
  row.innerHTML = `<span class="pick-glyph">${glyph || escapeHtml((app.trim()[0] || "•").toUpperCase())}</span><span class="pick-name">${escapeHtml(app)}</span><span class="pick-sub">${escapeHtml(sub.slice(0, 52))}</span>${badge}`;
  return row;
}

function openPicker() {
  $("#picker-pop").hidden = false;
  const search = $("#picker-search") as HTMLInputElement;
  search.value = "";
  search.focus();
  renderPicker(""); // show cached instantly
  loadWindows();    // then refresh live
}

function closePicker() { $("#picker-pop").hidden = true; }
function togglePicker() { if ($("#picker-pop").hidden) openPicker(); else closePicker(); }

function addWindow(w: Win) {
  const k = key(w);
  if (controlled.has(k)) return;
  feedEl().querySelector(".feed-empty")?.remove();

  const tile = document.createElement("div");
  tile.className = "tile";
  tile.id = `tile-${k}`;
  tile.dataset.k = k;
  const badge = w.on_current_space === false ? offSpaceBadgeHtml() : "";
  tile.innerHTML = `
    <div class="tile-head">
      <span class="idot"></span>
      <span class="t"><b>${escapeHtml(w.app_name)}</b><span class="app">${escapeHtml(windowLabel(w,lastWindows).slice(0, 52))}</span></span>
      ${badge}
      <button class="ctl" title="Take control: click, type and scroll land on the real window">control</button>
      <button class="x" title="Stop watching">✕</button>
    </div>
    <div class="shot-wrap">
      <img class="shot" alt="" draggable="false" hidden />
      <div class="err" hidden></div>
      <div class="capturing">capturing…</div>
      <div class="ctl-hint">controlling · click · type · scroll go through · <b>Esc</b> to release</div>
    </div>
    <div class="tile-status" hidden></div>`;
  tile.querySelector("button.x")!.addEventListener("click", () => removeWindow(w));
  feedEl().prepend(tile);
  logAction(`watch <b>${escapeHtml(w.app_name)}</b> · win ${w.window_id}`);
  controlled.set(k, { win: w, tile, visible: true, kind: "window" });
  wireControl(k, w, tile);
  io.observe(tile);
  captureLoop();
}

function addDisplay(n: number, label: string) {
  const k = `display:${n}`;
  if (controlled.has(k)) return;
  feedEl().querySelector(".feed-empty")?.remove();
  const w: Win = { app_name: label, pid: -1, window_id: -1, on_current_space: true, bounds: { x: 0, y: 0, width: 0, height: 0 } };
  const tile = document.createElement("div");
  tile.className = "tile";
  tile.id = `tile-${k}`;
  tile.dataset.k = k;
  tile.innerHTML = `
    <div class="tile-head"><span class="idot"></span><span class="t"><b>${escapeHtml(label)}</b></span><span class="view-only">view only</span><button class="x" title="Stop watching">✕</button></div>
    <div class="shot-wrap">
      <img class="shot" alt="" draggable="false" hidden />
      <div class="err" hidden></div>
      <div class="capturing">capturing…</div>
    </div>`;
  tile.querySelector("button.x")!.addEventListener("click", () => {
    const c = controlled.get(k); if (c) io.unobserve(c.tile);
    releaseFrame(tile.querySelector("img.shot")!);
    controlled.delete(k); tile.remove(); feedEmpty();
  });
  feedEl().prepend(tile);
  logAction(`watch <b>${escapeHtml(label.toLowerCase())}</b>`);
  controlled.set(k, { win: w, tile, visible: true, kind: "desktop", display: n });
  io.observe(tile);
  captureLoop();
}

function removeWindow(w: Win) {
  const k = key(w);
  if (controllingKey === k) setControlling(null);
  const c = controlled.get(k);
  if (c) io.unobserve(c.tile);
  controlled.delete(k);
  const tile = document.getElementById(`tile-${k}`);
  const img = tile?.querySelector<HTMLImageElement>("img.shot");
  if (img) releaseFrame(img);
  tile?.remove();
  logAction(`unwatch <b>${escapeHtml(w.app_name)}</b>`);
  feedEmpty();
}

function tileStatus(k: string, text: string | null) {
  // The tile id embeds the window key ("<pid>:<window_id>"); the ':' is not a
  // valid CSS id selector, so resolve by id (getElementById tolerates it) first.
  const el = document.getElementById(`tile-${k}`)?.querySelector(".tile-status") as HTMLElement | null;
  if (!el) return;
  if (text === null) { el.hidden = true; el.textContent = ""; }
  else { el.hidden = false; el.textContent = text; }
}

function updateRunControls() {
  const button = document.querySelector<HTMLButtonElement>('#chat-form button[type=submit]');
  const input = document.querySelector<HTMLInputElement>('#chat-input');
  if (input) input.disabled = !taskStateReady || activeAgent !== null;
  document.querySelectorAll<HTMLButtonElement>(".tile button.ctl")
    .forEach(control => { control.disabled = manualBlockMessage() !== null; });
  if (button) {
    if (!taskStateReady) {
      button.textContent = taskStateError ? "Retry status" : "Checking status…";
      button.disabled = !taskStateError;
      button.title = "Check whether a task is still running before sending input";
      return;
    }
    button.textContent = activeAgent ? (activeAgent.stopRequested ? "Stopping…" : "Stop") : "Send";
    button.disabled = !!activeAgent?.stopRequested;
    button.title = activeAgent ? "Stop this task. An in-flight operation may finish." : "Start a task";
  }
}

async function restoreRunningTask() {
  if (restoringTask) return;
  restoringTask = true;
  taskStateReady = false;
  taskStateError = false;
  setControlling(null);
  updateRunControls();
  try {
    agentEventsReady ??= listen<{ runId: string; kind: string; data: any }>("agent", (e) => onAgentEvent(e.payload))
      .catch(error => { agentEventsReady = undefined; throw error; });
    await agentEventsReady;
    const state = await invoke<{runId: string; goal: string; pid: number; windowId: number; appName: string; stopping: boolean;
      pendingConfirm?: { id: string; action: string; why: string; app: string } | null} | null>("agent_status");
    if (state && !activeAgent && !terminalRunIds.has(state.runId)) {
      const isRunWindow = (w: Win) => w.pid === state.pid && w.window_id === state.windowId;
      // A reload empties the feed. The window list is not loaded yet, so ask for it
      // once to put the run's real window back (with its title and bounds).
      if (!lastWindows.some(isRunWindow)) await loadWindows().catch(() => {});
      if (terminalRunIds.has(state.runId)) { taskStateReady = true; return; } // it ended while the list loaded
      const win = lastWindows.find(isRunWindow) ?? {
        pid: state.pid, window_id: state.windowId, app_name: state.appName,
        bounds: { x: 0, y: 0, width: 0, height: 0 },
      };
      activeAgent = { key: key(win), win, runId: state.runId, starting: false, stopRequested: state.stopping };
      // Watch it again, so the feed is not empty and the next goal has a window to name.
      addWindow(win);
      chat("agent", `Task still running in ${state.appName}: ${state.goal}`);
      // A question that was open when the window reloaded is asked again; it is still waiting.
      const pending = state.pendingConfirm;
      if (pending && !chatLog().querySelector(`.msg.confirm[data-run="${CSS.escape(state.runId)}"][data-id="${CSS.escape(pending.id)}"]`)) {
        askConfirmation(state.runId, pending);
        tileStatus(activeAgent.key, "waiting for your answer");
      }
    }
    taskStateReady = true;
  } catch (error) {
    taskStateError = true;
    chat("sys", `Could not restore task status: ${error}. Use Retry status before sending input.`);
  } finally {
    restoringTask = false;
    updateRunControls();
    void refreshMemory();
  }
}

async function stopAgent() {
  const task = activeAgent;
  if (!task || task.stopRequested) return;
  task.stopRequested = true;
  updateRunControls();
  tileStatus(task.key, "stopping after the current operation…");
  if (task.starting) return;
  await sendStop(task);
}

async function sendStop(task: NonNullable<typeof activeAgent>) {
  try {
    const requested = await invoke<boolean>("agent_stop", { runId: task.runId });
    if (!requested && activeAgent?.runId === task.runId) finishAgent(task.key, "no longer running");
  } catch (error) {
    if (activeAgent?.runId !== task.runId) return;
    task.stopRequested = false;
    chat("sys", `Could not request stop: ${error}`);
    updateRunControls();
  }
}

async function runAgent(w: Win, goal: string, resumeId?:string, workflow?:WorkflowInvocation) {
  if (activeAgent) { chat("sys", "One task at a time. Stop the current task first."); return; }
  setControlling(null);
  const allowForeground=takeForegroundChoice(document.querySelector<HTMLInputElement>("#run-allow-foreground"));
  const useScreenshots=(document.querySelector<HTMLInputElement>("#run-use-screenshots"))?.checked===true;
  const k = key(w);
  const runId = `run-${crypto.getRandomValues(new Uint32Array(4)).join("-")}`;
  activeAgent = { key: k, win: w, runId, starting: true, stopRequested: false };
  updateRunControls();
  chat("you", `${w.app_name}: ${goal}`);
  tileStatus(k, "starting…");
  logAction(`<b>run</b> on ${escapeHtml(w.app_name)}: ${escapeHtml(goal)}`);
  const brain = ($("#brain-select") as HTMLSelectElement)?.value || "auto";
  try {
    await agentEventsReady;
    if (activeAgent?.runId !== runId) return;
    if (activeAgent.stopRequested) { finishAgent(k, "stopped before submission"); return; }
    const mem = runMemoryOptions(($("#run-use-memory") as HTMLInputElement)?.checked ?? true, ($("#run-remember") as HTMLInputElement)?.checked ?? true);
    if(allowForeground)chat("sys",`Foreground input allowed for this task in ${w.app_name}. Its window may come forward or switch Spaces.`);
    await invoke("agent_run", { allowForeground, useScreenshots, goal, pid: w.pid, windowId: w.window_id, appName: w.app_name, brain, runId, resumeId, workflow, decisionBudget:Number((document.querySelector("#task-budget") as HTMLSelectElement)?.value || 18), ...mem });
    if (activeAgent?.runId !== runId) return;
    activeAgent.starting = false;
    if (activeAgent.stopRequested) await sendStop(activeAgent);
  } catch (e) {
    if (activeAgent?.runId !== runId) return;
    chat("agent", `couldn't start: ${e}`);
    finishAgent(k, "start failed");
    await restoreRunningTask();
  }
}

function onAgentEvent(ev: { runId: string; kind: string; data: any }) {
  if (!ev.runId) return;
  if (["done", "fail", "error", "stopped"].includes(ev.kind)) {
    terminalRunIds.add(ev.runId);
    if (terminalRunIds.size > 64) terminalRunIds.delete(terminalRunIds.values().next().value!);
  }
  if (!activeAgent || ev.runId !== activeAgent.runId) return;
  const k = activeAgent?.key;
  const d = ev.data || {};
  switch (ev.kind) {
    case "start":
      chat("agent", `on it (brain: ${d.brain}).`);
      if (k) tileStatus(k, "thinking…");
      break;
    case "observe":
      if (k) tileStatus(k, `step ${d.step} · observing`);
      break;
    case "think":
      if (d.fallback) logAction(`<b>provider</b> ${escapeHtml(d.fallback)}`);
      logAction(`<b>think</b> ${escapeHtml(d.thought || "")} → ${escapeHtml(d.action || "")}`);
      if (k) tileStatus(k, d.action || "acting"); // textContent: escaping here would show &quot;
      break;
    case "act":
      logAction(actLine(d, escapeHtml));
      break;
    case "malformed":
      logAction(malformedLine(d, escapeHtml));
      break;
    case "memory":
      logAction(memoryLine(d, escapeHtml));
      break;
    case "confirm":
      askConfirmation(ev.runId, d);
      if (k) tileStatus(k, "waiting for your answer");
      break;
    case "confirmed":
      settleConfirmation(ev.runId, d.id, d.answer);
      logAction(`<b>confirm</b> ${escapeHtml(d.answer || "")}`);
      break;
    case "done":
      chat("agent", `done. ${d.summary || ""}`);
      finishAgent(k, "done");
      break;
    case "fail":
      chat("agent", `can't do that: ${d.reason || ""}`);
      finishAgent(k, "failed");
      break;
    case "stopped":
      chat("agent", `stopped: ${d.reason || ""}${d.resumable?". Progress saved. Open Saved tasks to continue.":""}`);
      finishAgent(k, "stopped");
      break;
    case "error":
      chat("agent", `error: ${d.msg || ""}${d.resumable?". Progress saved; inspect the window before continuing.":""}`);
      finishAgent(k, "error");
      break;
  }
}

// A step of a run given unverified memory that sends, pays, deletes, submits or
// publishes waits here for the user's answer. The backend decides what is asked
// and times the request out; this only shows the question and sends the answer.
function askConfirmation(runId: string, d: { id: string; action: string; why: string; app: string }) {
  const box = document.createElement("div");
  box.className = "msg sys confirm";
  box.dataset.run = runId;
  box.dataset.id = d.id;
  box.innerHTML = confirmHtml(d, escapeHtml);
  box.querySelectorAll<HTMLButtonElement>("button").forEach((b) => b.addEventListener("click", async () => {
    box.querySelectorAll<HTMLButtonElement>("button").forEach((x) => (x.disabled = true));
    const approve = b.classList.contains("confirm-allow");
    try {
      const taken = await invoke<boolean>("agent_confirm", { runId, requestId: d.id, approve });
      if (!taken) settleConfirmation(runId, d.id, "gone");
    } catch (e) {
      chat("sys", `Could not send your answer: ${e}`);
      box.querySelectorAll<HTMLButtonElement>("button").forEach((x) => (x.disabled = false));
    }
  }));
  chatLog().append(box);
  chatLog().scrollTop = chatLog().scrollHeight;
}

function settleConfirmation(runId: string, id: string, answer: string) {
  const box = [...chatLog().querySelectorAll<HTMLElement>(".msg.confirm")].find((b) => b.dataset.run === runId && b.dataset.id === id);
  if (!box || box.dataset.settled) return;
  box.dataset.settled = "1";
  box.querySelector(".confirm-actions")?.remove();
  const note = document.createElement("div");
  note.className = "confirm-answer";
  note.textContent = confirmedText(answer);
  box.append(note);
}

function finishAgent(k: string | undefined, _why: string) {
  if (k) tileStatus(k, null);
  setControlling(null);
  activeAgent = null;
  updateRunControls();
  // The chip says "waits for the task" while one runs; do not leave it for the 20 s poll.
  void refreshMemory();
}

// ── @-mention command bar ───────────────────────────────────────────────────
// One input. Type "@" to reference a window (autocomplete), which also opens it
// in the feed. e.g. "in @Calculator compute 6×7" runs the task on Calculator.
const mentionTargets = new WindowReferences<Win>();
let mentionSel = -1;
let mentionSelectedId: string | null = null;
let mentionRequested = false;
let commandRevision = 0;

// The "@…" token immediately before the cursor, if any.
const activeMention = (input: HTMLInputElement) =>
  mentionQuery(input.value, input.selectionStart ?? input.value.length);

function refreshRequestedMentions() {
  const input = $("#chat-input") as HTMLInputElement | null;
  if (!mentionRequested || !input || input.disabled || document.activeElement !== input) return;
  const am = activeMention(input);
  if (am) openMentions(am.query, true);
}

function openMentions(query: string, preserveSelection = false) {
  if (!preserveSelection) mentionSelectedId = null;
  const pop = $("#mention-pop");
  const q = query.trim().toLowerCase();
  const wins = lastWindows.filter((w) => !q || `${w.app_name} ${w.title || ""}`.toLowerCase().includes(q)).slice(0, 8);
  // Codex-parity: also offer installed apps that aren't open yet. selecting one
  // launches it in the background (focus-free) and references its window.
  // Copilot is open by definition; never offer to launch it into itself.
  const openNames = [...lastWindows.map((w) => w.app_name), "Copilot"];
  const apps = launchableApps(installedApps, openNames, query, 6);
  if (!wins.length && !apps.length) {
    // No results yet is different from the user dismissing the query.
    pop.hidden = true; mentionSel = -1;
    return;
  }
  pop.innerHTML = "";
  wins.forEach((w) => {
    const row = pickRow("", w.app_name, windowLabel(w,lastWindows), w.on_current_space === false);
    row.type = "button"; // it lives inside the form; don't let a click submit
    row.classList.add("mention-row");
    row.dataset.target = `window:${key(w)}`;
    row.onmousedown = (e) => { e.preventDefault(); selectMention(w); };
    pop.append(row);
  });
  apps.forEach((a) => {
    const row = pickRow("", a.name, "launch", false);
    row.type = "button";
    row.classList.add("mention-row", "mention-launch");
    row.dataset.target = `app:${a.bundle_id || a.name.toLowerCase()}`;
    row.onmousedown = (e) => { e.preventDefault(); launchAndReference(a); };
    pop.append(row);
  });
  pop.hidden = false;
  const rows = [...pop.querySelectorAll<HTMLElement>(".mention-row")];
  mentionSel = mentionSelectedId === null ? 0 : rows.findIndex(r => r.dataset.target === mentionSelectedId);
  rows.forEach((r, i) => r.classList.toggle("sel", i === mentionSel));
  if (mentionSel >= 0) mentionSelectedId = rows[mentionSel].dataset.target!;
}

function closeMentions() { $("#mention-pop").hidden = true; mentionSel = -1; mentionSelectedId = null; mentionRequested = false; }

// Launch an app in the background (focus-free) then reference its window in the
// command bar, so you can @-mention an app that wasn't open yet. The daemon
// returns the launched window(s); if the window isn't ready yet we poll briefly.
async function launchAndReference(a: AppEntry): Promise<void> {
  closeMentions();
  const inputEl = $("#chat-input") as HTMLInputElement;
  const revision = ++commandRevision;
  const draft = inputEl.value;
  const prevCaret = inputEl.selectionStart ?? inputEl.value.length;
  const prevEnd = inputEl.selectionEnd;
  try {
    const res = await invoke<{ pid?: number; windows?: Win[] }>("cua_launch_app", { app: a.bundle_id || a.name });
    let w: Win | undefined = (res.windows ?? [])[0];
    for (let tries = 0; !w && tries < 8; tries++) {
      await sleep(400);
      await loadWindows();
      w = lastWindows.find((x) => x.app_name.toLowerCase() === a.name.toLowerCase() || (res.pid != null && x.pid === res.pid));
    }
    if (!w) { setDriverDot(true); return; }
    addWindow(w);
    // Launch completion belongs to the original draft, never a later edit or
    // selection. Opening the requested window must not reclaim input focus.
    if (commandRevision !== revision || inputEl.value !== draft || inputEl.disabled
      || document.activeElement !== inputEl || inputEl.selectionStart !== prevCaret
      || inputEl.selectionEnd !== prevEnd) return;
    const slug = mentionTargets.register(w);
    const next = applyMention(inputEl.value, prevCaret, slug);
    inputEl.value = next.value;
    inputEl.setSelectionRange(next.caret, next.caret);
  } catch {
    /* launch failed (daemon down / unknown app). leave the bar as typed */
  }
}

function selectMention(w: Win) {
  commandRevision++;
  const input = $("#chat-input") as HTMLInputElement;
  const slug = mentionTargets.register(w);
  const pos = input.selectionStart ?? input.value.length;
  const next = applyMention(input.value, pos, slug);
  input.value = next.value;
  input.setSelectionRange(next.caret, next.caret);
  addWindow(w); // the @ opens the page to view
  closeMentions();
  input.focus();
}

function onCommandInput() {
  commandRevision++;
  const input = $("#chat-input") as HTMLInputElement;
  const am = activeMention(input);
  if (am) {
    mentionRequested = true;
    if (!lastWindows.length) loadWindows();
    loadApps();
    openMentions(am.query);
  }
  else closeMentions();
}

// Arrow/Enter/Escape navigation while the mention popover is open.
function onCommandKey(e: KeyboardEvent) {
  // Composition keys belong to the text input, including the legacy IME code
  // used by webviews around compositionend when isComposing may be false.
  if (e.isComposing || e.keyCode === 229) return;
  const pop = $("#mention-pop");
  if (e.key === "Escape") {
    commandRevision++;
    if (mentionRequested || !pop.hidden) e.preventDefault();
    closeMentions();
    return;
  }
  if (pop.hidden) { if (e.key === "@") { loadWindows(); loadApps(); } return; }
  const rows = [...pop.querySelectorAll(".mention-row")] as HTMLElement[];
  if (!rows.length) return;
  if (e.key === "ArrowDown" || e.key === "ArrowUp") {
    e.preventDefault();
    mentionSel = mentionSel < 0 ? (e.key === "ArrowDown" ? 0 : rows.length - 1)
      : (mentionSel + (e.key === "ArrowDown" ? 1 : -1) + rows.length) % rows.length;
    mentionSelectedId = rows[mentionSel].dataset.target!;
    rows.forEach((r, i) => r.classList.toggle("sel", i === mentionSel));
    rows[mentionSel].scrollIntoView({ block: "nearest" });
  } else if (e.key === "Enter" || e.key === "Tab") {
    e.preventDefault();
    rows[mentionSel]?.dispatchEvent(new MouseEvent("mousedown"));
  }
}

// Send and Stop are one button; sendControl keeps one activation from doing both.
const onChatSubmit = sendControl({
  ready: () => taskStateReady,
  statusError: () => taskStateError,
  retryStatus: () => void restoreRunningTask(),
  running: () => activeAgent !== null,
  stop: () => void stopAgent(),
  start: startFromCommand,
  ignored: () => { if (activeAgent) tileStatus(activeAgent.key, "just started, press Stop again to stop it"); },
});

/** Start a task from the command field; true when one started. */
function startFromCommand(): boolean {
  const input = $("#chat-input") as HTMLInputElement;
  const text = input.value.trim();
  if (!text) return false;
  if (manualInputs.busy) {
    chat("sys", "Wait for manual input to finish before starting a task.");
    return false;
  }
  // Resolve the @mention(s) to a target window.
  const selection = resolveMentionTarget(parseMentions(text), s => mentionTargets.get(s),
    [...controlled.values()].filter(c => c.kind === "window").map(c => c.win));
  if ("error" in selection) {
    const messages = {
      unresolved: "Select each @window from suggestions before starting, or quote literal @text.",
      multiple: "Choose one window for this task.",
      required: "Reference a window with @. Type @ to pick one.",
    };
    chat("sys", messages[selection.error]);
    return false;
  }
  const target = selection.target;
  commandRevision++;
  input.value = "";
  closeMentions();
  // Read the goal with @slugs shown as their real app names. Collapse the extra
  // space the mention insert leaves when you keep typing after picking a window.
  const goal = readableGoal(text, (s) => mentionTargets.get(s)?.app_name).replace(/\s{2,}/g, " ").trim();
  const resumeId=pendingResumeId;pendingResumeId=undefined;$("#task-resume-notice").hidden=true;
  const workflow=pendingWorkflow;pendingWorkflow=undefined;pendingWorkflowGoal=undefined;
  void runAgent(target, goal, resumeId, workflow);
  return true;
}

let pendingResumeId:string|undefined;
let pendingWorkflow:WorkflowInvocation|undefined;
let pendingWorkflowGoal:string|undefined;
async function showSavedTasks(){
 const panel=$("#task-progress-panel"),list=$("#task-progress-list");panel.hidden=!panel.hidden;
 if(panel.hidden)return;
 try{
  const rows=await invoke<SavedTask[]>("agent_checkpoints");
  renderSavedTasks(list,rows??[],row=>{
   pendingWorkflow=undefined;pendingWorkflowGoal=undefined;
   resetForegroundChoice(document.querySelector<HTMLInputElement>("#run-allow-foreground"));
   pendingResumeId=row.id;$("#task-resume-notice").hidden=false;$("#task-resume-notice span").textContent="Saved task loaded. Review the goal and target.";($("#chat-input") as HTMLInputElement).value=row.goal;panel.hidden=true;
   chat("sys",`Select the original ${row.app_name} window, review the goal, then Send to resume.`);
  },async id=>{
   await invoke("agent_checkpoint_discard",{id});
   if(pendingResumeId===id){pendingResumeId=undefined;$("#task-resume-notice").hidden=true;}
  },error=>chat("sys",error));
 }catch(error){list.textContent=String(error);}
}

// ── Memory: the chip shows the state; its panel holds the controls and the list. ──
let memoryStatus: MemoryStatus | null = null;
const memoryForm=memorySettingsForm(document);
let memoryRefreshEpoch=0;

async function refreshMemory() {
  const epoch=++memoryRefreshEpoch;
  try {
    const status=await invoke<MemoryStatus>("memory_status");
    if(epoch!==memoryRefreshEpoch)return;
    memoryStatus=status;
  } catch (e) {
    if(epoch===memoryRefreshEpoch)$("#memory-text").textContent = "Memory unavailable";
    return;
  }
  const chip = memoryChip(memoryStatus);
  $("#memory-text").textContent = chip.text;
  $("#memory-dot").className = `dot ${chip.tone === "ok" ? "ok" : chip.tone === "warn" ? "warn" : ""}`;
  const pop = $("#memory-pop");
  if (pop.hidden) return;
  const s = memoryStatus.settings;
  $("#memory-state").textContent = chip.text + memoryStatusSuffix(memoryStatus);
  $("#memory-pause").textContent = s.paused ? "Resume" : "Pause";
  $("#mem-provider-note").textContent = providerNote(s);
  const editingMemorySetting=document.activeElement?.matches("input,textarea,select") && document.activeElement.closest(".mem-settings");
  memoryForm.refresh(s,Boolean(editingMemorySetting));
  const list = await invoke<MemoryList>("memory_list").catch(() => null);
  if (list&&epoch===memoryRefreshEpoch) $("#memory-list").innerHTML = memoryListHtml(list, escapeHtml);
}

function wireMemory() {
  wireUtilityPanelKeyboard(document);
  wireMemorySources(document,invoke,()=>void refreshMemory());
  wireTrajectoryImport($("#trajectory-tools"), invoke, refreshMemory);
  wireWorkflows($("#trajectory-tools"),invoke,(entry,request)=>{
    pendingResumeId=undefined;pendingWorkflow=request;
    resetForegroundChoice(document.querySelector<HTMLInputElement>("#run-allow-foreground"));
    $("#task-resume-notice").hidden=false;
    $("#task-resume-notice span").textContent=`Workflow ready: ${entry.workflow.plan.title}. Select its app and review the task before Send.`;
    pendingWorkflowGoal=`Use the reviewed workflow ${entry.workflow.plan.title} with the supplied inputs and verify its result.`;
    ($("#chat-input") as HTMLInputElement).value=pendingWorkflowGoal;
    chat("sys",`Select the ${entry.workflow.app_name} window, review the task and model, then Send. Workflow inputs are shared only when you start the task.`);
  });
  const clearPreparedWorkflow=()=>{
    if(!pendingWorkflow)return;
    const input=$("#chat-input") as HTMLInputElement;
    if(input.value===pendingWorkflowGoal)input.value="";
    pendingWorkflow=undefined;pendingWorkflowGoal=undefined;
    $("#task-resume-notice").hidden=true;$("#task-resume-notice span").textContent="";
  };
  document.addEventListener("icu-history-cleared",clearPreparedWorkflow);
  document.addEventListener("icu-memory-cleared",clearPreparedWorkflow);
  const clearMemoryView=()=>{
    memoryRefreshEpoch++;memoryStatus=null;$("#memory-list").replaceChildren();
    void refreshMemory();
  };
  document.addEventListener("icu-history-cleared",clearMemoryView);
  document.addEventListener("icu-memory-cleared",clearMemoryView);
  wireAudio(document, invoke);
  wireHistory(document, invoke);
  wireHistorySummaries(document,invoke);
  wireHistoryDeletion(document,invoke);
  $("#memory-chip").addEventListener("click", () => {
    if (toggleUtilityPanel(document,"memory-pop")) void refreshMemory();
  });
  $("#memory-pause").addEventListener("click", async () => {
    if (!memoryStatus) return;
    await invoke("memory_pause", { paused: !memoryStatus.settings.paused }).catch(e => chat("sys", `Memory: ${e}`));
    await refreshMemory();
  });
  $("#mem-save").addEventListener("click", async () => {
    if (!memoryStatus) return;
    const patch = memoryForm.patch();
    const note = $("#mem-note");
    if (patch===null) return;
    if (typeof patch === "string") { note.textContent = patch; return; }
    try { await invoke("memory_settings_set", { settings: patch }); note.textContent = "Saved."; }
    catch (e) { note.textContent = String(e); }
    await refreshMemory();
  });
  $("#memory-list").addEventListener("click", async (ev) => {
    const id = (ev.target as HTMLElement).closest<HTMLElement>("[data-del]")?.dataset.del;
    if (!id) return;
    try {
      await invoke("memory_delete", { id });
      if(id==="skysight"||id==="all")document.dispatchEvent(new CustomEvent("icu-memory-cleared"));
    } catch(e) { chat("sys",`Memory: ${e}`); }
    await refreshMemory();
  });
  $("#mem-delete-all").addEventListener("click", async () => {
    // window.confirm is a silent no-op in the macOS webview, so the question is asked in the page.
    try {
      const deleted = await deleteAllMemories($("#mem-delete-all").parentElement!,()=>invoke("memory_delete",{id:"all"}));
      if(deleted){document.dispatchEvent(new CustomEvent("icu-memory-cleared"));await refreshMemory();}
    } catch(e) { chat("sys",`Memory: ${e}`); }
  });
  void refreshMemory();
  setInterval(() => void refreshMemory(), 20_000);
}

function escapeHtml(s: string) {
  return s.replace(/[&<>"]/g, (c) => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;" })[c]!);
}

// ── Auto-update: quiet check on startup, then a toast to install on confirm. ──
// The flow is detect → download → verify-signature → install → relaunch; the UI half is in
// updater.ts and the plugin's checks are exercised by src-tauri/src/updater_tests.rs.
async function checkForUpdate() {
  const offered = await findUpdate(check);
  // An MSI install is only offered an MSI (see fitsInstall); a failed question is "not an MSI", as before.
  const kind = offered ? await invoke<string>("cua_update_install_kind").catch(() => "unknown") : "unknown";
  const found = fitsInstall(offered, kind);
  const update = found && await withOwnInstall(found, {
    owns: () => invoke<boolean>("cua_update_owns_install"),
    install: (onEvent) => {
      const channel = new Channel<UpdateEvent>();
      channel.onmessage = onEvent;
      return invoke<InstallResult>("cua_install_update", { onEvent: channel });
    },
  });
  // A .deb or .rpm install: the backend checks the downloaded package's dependencies before the plugin installs it, so a
  // package that needs something this machine lacks is refused with its name in the toast, not "Failed to install package".
  const linuxPkg = update && await withOwnInstall(update, {
    owns: async () => !!(await invoke<string | null>("cua_update_linux_package")),
    install: (onEvent) => {
      const channel = new Channel<UpdateEvent>();
      channel.onmessage = onEvent;
      return invoke<InstallResult>("cua_install_linux_package", { onEvent: channel });
    },
  });
  if (linuxPkg) showUpdateToast(linuxPkg, {
    relaunch, log: logAction, busy: () => activeAgent !== null,
    blocked: () => invoke<string | null>("cua_update_blocked"),
  });
}

// ── Direct interaction: click / type / scroll pass through the tile onto the
// real window using coordinates in the decoded screenshot. ──

function manualBlockMessage(): string | null {
  if (taskStateError) return "Task status is unavailable. Use Retry status before sending input.";
  if (!taskStateReady) return "Waiting for task status.";
  if (activeAgent) return "Stop the current task before using manual input.";
  return null;
}

function setControlling(k: string | null) {
  const blocked = manualBlockMessage();
  if (k && blocked) { tileStatus(k, blocked); return; }
  if (controllingKey !== k) manualInputs.cancel();
  if (controllingKey && controllingKey !== k) controlled.get(controllingKey)?.tile.classList.remove("controlling");
  controllingKey = k;
  manualTextInput?.dispose();
  manualTextInput = null;
  if (k) {
    const c = controlled.get(k);
    c?.tile.classList.add("controlling");
    if (c?.kind === "window") {
      const wrap = c.tile.querySelector<HTMLElement>(".shot-wrap")!;
      const editor: ManualTextInput = new ManualTextInput(wrap, `Type into ${c.win.app_name}`,
        () => manualTextInput === editor && controllingKey === k && controlled.get(k) === c,
        text => {
          const blocked = manualBlockMessage();
          if (blocked) { tileStatus(k, blocked); return; }
          void sendManualInput(k, "cua_type", { pid: c.win.pid, windowId: c.win.window_id, text });
          // Committed text can be a pasted password. Show the requested action
          // without reflecting its content into the app's feedback surface.
          flashAction(wrap, "type");
        });
      manualTextInput = editor;
    }
  }
}

function ripple(wrap: HTMLElement, img: HTMLImageElement, clientX: number, clientY: number) {
  const r = img.getBoundingClientRect();
  const dot = document.createElement("span");
  dot.className = "ripple";
  dot.style.left = `${clientX - r.left}px`;
  dot.style.top = `${clientY - r.top}px`;
  wrap.appendChild(dot);
  setTimeout(() => dot.remove(), 550);
}

// A brief pill on a tile showing requested manual input (click / scroll /
// type / key), including requests waiting behind an earlier operation.
const flashTimers = new WeakMap<HTMLElement, number>();
function flashAction(wrap: HTMLElement, label: string) {
  let pill = wrap.querySelector(".action-flash") as HTMLElement | null;
  if (!pill) {
    pill = document.createElement("div");
    pill.className = "action-flash";
    wrap.appendChild(pill);
  }
  pill.textContent = label;
  pill.classList.add("show");
  const prev = flashTimers.get(pill);
  if (prev) clearTimeout(prev);
  flashTimers.set(pill, window.setTimeout(() => pill!.classList.remove("show"), 850));
}

// The shot-wrap of the tile currently taking keyboard control (for type/key flashes).
function controllingWrap(): HTMLElement | null {
  if (!controllingKey) return null;
  return (controlled.get(controllingKey)?.tile.querySelector(".shot-wrap") as HTMLElement) ?? null;
}

function sendManualInput(k: string, cmd: string, args: Record<string, unknown>): Promise<unknown> {
  return manualInputs.enqueue(() => invoke(cmd, args))
    .catch(err => {
      // A composition may still be buffered outside the queue when its click
      // fails. Retire that editor too, before a late commit can be admitted.
      if (controllingKey === k) setControlling(null);
      tileStatus(k, "action failed: " + String(err).slice(0, 70));
    });
}

function wireControl(k: string, w: Win, tile: HTMLElement) {
  const wrap = tile.querySelector(".shot-wrap") as HTMLElement;
  const img = tile.querySelector("img.shot") as HTMLImageElement;
  const ctlBtn = tile.querySelector("button.ctl") as HTMLButtonElement;
  ctlBtn.disabled = manualBlockMessage() !== null;
  ctlBtn.addEventListener("click", () => setControlling(controllingKey === k ? null : k));

  const send: InvokeFn = (cmd, args) =>
    sendManualInput(k, cmd, args);

  // Real pointer gestures drive the window (see interact.ts::wireDrive). Feedback
  // hooks give the viewer a ripple + an action pill so you can see it land.
  wireDrive(img, {
    invoke: send,
    rect: () => img.getBoundingClientRect(),
    target: () => ({ pid: w.pid, window_id: w.window_id,
      bounds: { width: img.naturalWidth, height: img.naturalHeight },
      screenshot_id: img.dataset.screenshotId }),
    blocked: () => manualBlockMessage() !== null || w.on_current_space === false,
    onEngage: () => setControlling(k),
    onBlocked: () => tileStatus(k, manualBlockMessage() || "on another Space. Click the off-Space badge to bring it here first"),
    onClick: (p, e) => {
      ripple(wrap, img, e.clientX, e.clientY);
      flashAction(wrap, "click");
      logAction(`<b>click</b> ${escapeHtml(w.app_name)} @ ${p.x},${p.y}`);
    },
    onScroll: (_p, dir) => flashAction(wrap, `scroll ${dir === "down" ? "↓" : "↑"}`),
  });
}

function onControlKey(e: KeyboardEvent) {
  if (!controllingKey || !manualTextInput?.focused || manualTextInput.isComposing) return;
  const c = controlled.get(controllingKey);
  if (!c || c.kind !== "window") return;
  const plan = planKey(e);
  if (plan.kind === "ignore") return;
  if (e.key === "Escape") { setControlling(null); e.preventDefault(); return; } // Esc releases control
  const blocked = manualBlockMessage();
  if (blocked) { e.preventDefault(); tileStatus(key(c.win), blocked); return; }
  // Printable text is delivered by the editor's input event. Preventing its
  // keydown would prevent IME, dead-key and other browser editing commits.
  if (plan.kind === "type") return;
  e.preventDefault();
  const w = c.win;
  const wrap = controllingWrap();
  if (plan.kind === "key") {
    void sendManualInput(key(w), "cua_press_key", { pid: w.pid, windowId: w.window_id, key: plan.key });
    if (wrap) flashAction(wrap, `⌤ ${plan.key}`);
  }
}

window.addEventListener("DOMContentLoaded", () => {
  wireRunPreferences(document, () => window.sessionStorage);
  $("#task-start-fresh")?.addEventListener("click",()=>{pendingResumeId=undefined;pendingWorkflow=undefined;pendingWorkflowGoal=undefined;$("#task-resume-notice").hidden=true;});
  $("#task-progress-button")?.addEventListener("click",()=>void showSavedTasks());
  $("#chat-form").addEventListener("submit", onChatSubmit);
  $("#chat-input").addEventListener("input", onCommandInput);
  $("#chat-input").addEventListener("keydown", onCommandKey);
  // click outside the mention popover closes it
  document.addEventListener("click", (e) => {
    const pop = $("#mention-pop");
    if (!pop.hidden && !$(".cmd-wrap").contains(e.target as Node)) closeMentions();
  });
  $("#add-window").addEventListener("click", (e) => { e.stopPropagation(); togglePicker(); });
  $("#picker-search").addEventListener("input", (e) => renderPicker((e.target as HTMLInputElement).value));
  $("#picker-search").addEventListener("keydown", (e) => {
    if (e.key === "Escape") { closePicker(); }
    else if (e.key === "Enter") { (document.querySelector("#picker-list .pick-row") as HTMLButtonElement | null)?.click(); }
  });
  // click outside the popover closes it
  document.addEventListener("click", (e) => {
    const pop = $("#picker-pop");
    if (!pop.hidden && !pop.contains(e.target as Node) && (e.target as HTMLElement).id !== "add-window") closePicker();
  });
  actionLog().innerHTML = `<div class="empty">actions will appear here</div>`;
  feedEmpty();
  chat("sys", "Add a window, then click or type right on it.");
  void document.body.offsetHeight; // reflow so load-time enter animations actually start
  // Delegated: clicking an off-Space tile's Activate badge brings that window over
  // (the only deliberate focus move). Nothing else foregrounds a window.
  feedEl().addEventListener("click", (e) => {
    const btn = (e.target as HTMLElement).closest?.(".badge.activate") as HTMLElement | null;
    if (!btn) return;
    e.stopPropagation();
    const tile = btn.closest(".tile") as HTMLElement | null;
    const c = tile ? controlled.get(tile.dataset.k ?? "") : undefined;
    if (c) activateTile(c, btn);
  });
  document.addEventListener("visibilitychange", () => { if (!document.hidden) { captureLoop(); void checkPermissions(); } });
  window.addEventListener("focus", () => { void checkPermissions(); });
  window.addEventListener("keydown", onControlKey);
  void restoreRunningTask();
  installStallWatchdog();
  setInterval(refreshControlledMeta, 2500); // keep bounds/Space fresh, catch closed windows
  invoke<number>("cua_display_count").then((n) => { displayCount = n || 1; }).catch(() => {});
  invoke<[string, string]>("cua_desktop_label").then((l) => { desktopLabel = l; }).catch(() => {});
  setDriverStatus().then(loadWindows);
  void checkPermissions();
  checkForUpdate();
  wireMemory();
  // Offer model onboarding when a fresh (non-dev) launch has no model configured.
  void wireOnboarding(document, invoke).refresh();
});
