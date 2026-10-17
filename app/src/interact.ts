// Interaction math for driving a real window through its live tile. Kept pure
// and dependency-injected so it can be tested without a webview or the driver.

export type Bounds = { width: number; height: number };
export type Rect = { left: number; top: number; width: number; height: number };

/// Map a client-space point on a displayed image to decoded screenshot pixels,
/// the driver's coordinate contract. `bounds` is the decoded image size, which
/// can differ from the window's logical size after downscaling or on Retina. Returns null for
/// degenerate geometry (image not laid out yet, or a zero-size window) so callers
/// never send NaN or garbage coordinates to the driver.
export function mapToWindow(
  rect: Rect,
  bounds: Bounds | undefined,
  clientX: number,
  clientY: number,
): { x: number; y: number } | null {
  const bw = bounds?.width ?? 0;
  const bh = bounds?.height ?? 0;
  if (![rect.left, rect.top, rect.width, rect.height, bw, bh, clientX, clientY].every(Number.isFinite)) return null;
  if (rect.width <= 0 || rect.height <= 0 || bw <= 0 || bh <= 0) return null;
  const fx = Math.min(1, Math.max(0, (clientX - rect.left) / rect.width));
  const fy = Math.min(1, Math.max(0, (clientY - rect.top) / rect.height));
  // Width/height are extents, not valid last pixel coordinates. Rounding a
  // point just inside the right/bottom edge must not leave the target window.
  return {
    x: Math.min(Math.ceil(bw) - 1, Math.round(fx * bw)),
    y: Math.min(Math.ceil(bh) - 1, Math.round(fy * bh)),
  };
}

/// Natural-direction wheel delta for the driver's scroll (wheel down scrolls
/// content down). Kept tiny and testable.
export function scrollDelta(deltaY: number): number {
  if (!Number.isFinite(deltaY) || deltaY === 0) return 0;
  return deltaY > 0 ? -3 : 3;
}

export type InvokeFn = (cmd: string, args: Record<string, unknown>) => unknown;
/// `screenshot_id` names the capture the tile is showing (the driver returns
/// one per capture); a click carries it so the driver checks the pixel
/// against that frame instead of whichever capture of the window ran last.
export type Target = { pid: number; window_id: number; bounds?: Bounds; screenshot_id?: string };

/// The full click-through gesture: map a client point on the tile to the window's
/// screenshot pixel coordinate and dispatch it to the driver. Returns the mapped point, or
/// null (and dispatches nothing) for degenerate geometry. Invoke is injected so
/// the whole path is testable without a webview.
export function driveClick(
  rect: Rect,
  target: Target,
  clientX: number,
  clientY: number,
  button: "left" | "right" | "middle",
  invoke: InvokeFn,
): { x: number; y: number } | null {
  const p = mapToWindow(rect, target.bounds, clientX, clientY);
  if (!p) return null;
  const args: Record<string, unknown> = { pid: target.pid, windowId: target.window_id, x: p.x, y: p.y, button };
  if (target.screenshot_id) args.screenshotId = target.screenshot_id;
  invoke("cua_click", args);
  return p;
}

export type DriveHooks = {
  invoke: InvokeFn;
  rect: () => Rect;            // the displayed image's current bounding rect
  target: () => Target;        // window identity and decoded screenshot dimensions
  blocked?: () => boolean;     // true => don't drive (e.g. off-Space); UI in onBlocked
  onEngage?: () => void;       // any gesture begins (e.g. take control)
  onBlocked?: () => void;      // a gesture was attempted while blocked
  onClick?: (p: { x: number; y: number }, e: MouseEvent) => void;
  onScroll?: (p: { x: number; y: number }, dir: "up" | "down", e: WheelEvent) => void;
};

/// Wire a real pointer gesture on `el` so it drives the target window. Extracted
/// here so it can be exercised with synthesized DOM events: a real mouse fires
/// these listeners, but cua-driver's AX-based synthetic click cannot, so this is
/// the layer where the click-through is actually verifiable (see interact.test.ts).
export function wireDrive(el: HTMLElement, h: DriveHooks): void {
  el.addEventListener("contextmenu", (e) => e.preventDefault());
  el.addEventListener("mousedown", (e) => {
    e.preventDefault();
    h.onEngage?.();
    if (h.blocked?.()) { h.onBlocked?.(); return; }
    if (e.button < 0 || e.button > 2) return;
    const button = e.button === 2 ? "right" : e.button === 1 ? "middle" : "left";
    const p = driveClick(h.rect(), h.target(), (e as MouseEvent).clientX, (e as MouseEvent).clientY, button, h.invoke);
    if (p) h.onClick?.(p, e as MouseEvent);
  });
  let last = -Infinity;
  el.addEventListener(
    "wheel",
    (e) => {
      e.preventDefault();
      h.onEngage?.();
      if (h.blocked?.()) { h.onBlocked?.(); return; }
      const dy = scrollDelta((e as WheelEvent).deltaY);
      if (!dy) return;
      const now = typeof performance !== "undefined" ? performance.now() : Date.now();
      if (now - last < 80) return; // throttle so we don't flood the driver
      last = now;
      const t = h.target();
      const p = mapToWindow(h.rect(), t.bounds, (e as WheelEvent).clientX, (e as WheelEvent).clientY);
      if (!p) return;
      const args: Record<string, unknown> = { pid: t.pid, windowId: t.window_id, x: p.x, y: p.y, dy, amount: 3 };
      // As for a click: the point was read off the frame the tile shows.
      if (t.screenshot_id) args.screenshotId = t.screenshot_id;
      h.invoke("cua_scroll", args);
      h.onScroll?.(p, (e as WheelEvent).deltaY > 0 ? "down" : "up", e as WheelEvent);
    },
    { passive: false },
  );
}

// ── @-mention command parsing ────────────────────────────────────────────────
// The command bar lets you write "in @Calculator compute 6×7". These pure string
// ops back the autocomplete so the fiddly caret/token math is testable.

/// The "@…" token immediately before the caret, or null if the caret isn't in
/// one. `at` is where the "@" sits so a selection can replace the whole token.
export function mentionQuery(value: string, caret: number): { at: number; query: string } | null {
  const m = value.slice(0, caret).match(/(?:^|\s)@([^\s@]*)$/);
  return m ? { at: caret - m[1].length - 1, query: m[1] } : null;
}

/// Insert "@slug " over the active @token, returning the new value and caret.
/// With no active token (nothing typed yet) it inserts at the caret.
export function applyMention(value: string, caret: number, slug: string): { value: string; caret: number } {
  const am = mentionQuery(value, caret);
  const at = am ? am.at : caret;
  const insert = `@${slug} `;
  return { value: value.slice(0, at) + insert + value.slice(caret), caret: at + insert.length };
}

/// The @slugs referenced in a command, in order (deduped later by the caller).
export function parseMentions(text: string): string[] {
  // A bare @ is an unresolved reference, not permission to use a fallback.
  return [...text.matchAll(/(?:^|\s)@([^\s@]*)/g)].map((m) => m[1]);
}

/// Rewrite "@slug" tokens to human names via `nameOf` (slug → name | undefined)
/// so the agent reads "in Calculator compute…". Unknown slugs are left as typed.
export function readableGoal(text: string, nameOf: (slug: string) => string | undefined): string {
  return text.replace(/(^|\s)@([^\s@]+)/g, (m, prefix, s) => {
    const name = nameOf(s);
    return name === undefined ? m : prefix + name;
  });
}

/// Keep each alias bound to its selected window, including name collisions.
export class WindowReferences<T extends Target & { app_name: string }> {
  private bindings = new Map<string, { slug: string; target: T }>();

  register(target: T): string {
    for (const binding of this.bindings.values()) {
      if (binding.target.pid === target.pid && binding.target.window_id === target.window_id) {
        binding.target = target;
        return binding.slug;
      }
    }
    const base = target.app_name.replace(/[\s@]+/g, "") || "Window";
    let slug = base;
    for (let suffix = 2; this.bindings.has(slug.toLowerCase()); suffix++) slug = `${base}-${suffix}`;
    this.bindings.set(slug.toLowerCase(), { slug, target });
    return slug;
  }

  get(slug: string): T | undefined { return this.bindings.get(slug.toLowerCase())?.target; }
}

/// An explicit reference never falls back to a different watched window.
export function resolveMentionTarget<T extends Target>(
  mentions: string[], lookup: (slug: string) => T | undefined, watched: T[],
): { target: T } | { error: "unresolved" | "multiple" | "required" } {
  if (!mentions.length) return watched.length === 1 ? { target: watched[0] } : { error: "required" };
  const targets = new Map<string, T>();
  for (const slug of mentions) {
    const target = lookup(slug);
    if (!target) return { error: "unresolved" };
    targets.set(`${target.pid}:${target.window_id}`, target);
  }
  if (targets.size !== 1) return { error: "multiple" };
  return { target: targets.values().next().value! };
}

export type AppEntry = { name: string; bundle_id?: string; running?: boolean };

/// Apps worth offering to LAUNCH from the @-bar: installed apps that don't
/// already have an open window (so we don't duplicate a window row), matching the
/// query, name-deduped, capped. Selecting one launches it in the background.
export function launchableApps(
  apps: AppEntry[],
  openAppNames: string[],
  query: string,
  limit = 6,
): AppEntry[] {
  const open = new Set(openAppNames.map((n) => n.toLowerCase()));
  const q = query.trim().toLowerCase();
  const seen = new Set<string>();
  const out: AppEntry[] = [];
  for (const a of apps) {
    const name = (a.name || "").trim();
    if (!name) continue;
    const lower = name.toLowerCase();
    if (open.has(lower) || seen.has(lower)) continue;
    if (q && !lower.includes(q)) continue;
    seen.add(lower);
    out.push(a);
    if (out.length >= limit) break;
  }
  return out;
}

/// The driver's key name for a browser KeyboardEvent.key, or null if it's a
/// printable character (which should go through `type_text` instead).
const NAMED_KEYS: Record<string, string> = {
  Enter: "return", Backspace: "backspace", Tab: "tab", Escape: "escape", Delete: "delete",
  ArrowUp: "up", ArrowDown: "down", ArrowLeft: "left", ArrowRight: "right",
  Home: "home", End: "end", PageUp: "pageup", PageDown: "pagedown",
};

export type KeyPlan =
  | { kind: "key"; key: string }
  | { kind: "type"; text: string }
  | { kind: "ignore" };

/// Decide how a keystroke should be delivered to the window. Modifier combos
/// (Cmd/Ctrl/Alt) are ignored so app/OS shortcuts still work.
export function planKey(e: { key: string; metaKey?: boolean; ctrlKey?: boolean; altKey?: boolean; isComposing?: boolean; keyCode?: number }): KeyPlan {
  // Composition text and candidate-control keys belong to the input method.
  // Some webviews report keyCode 229 around compositionend without isComposing.
  if (e.isComposing || e.keyCode === 229) return { kind: "ignore" };
  if (e.metaKey || e.ctrlKey || e.altKey) return { kind: "ignore" };
  if (NAMED_KEYS[e.key]) return { kind: "key", key: NAMED_KEYS[e.key] };
  if (e.key.length === 1 && !e.metaKey && !e.ctrlKey && !e.altKey) return { kind: "type", text: e.key };
  return { kind: "ignore" };
}

/// `source` says whose grants the status describes: the app's own, when it owns
/// and bundles the driver, or an external driver's, read from the driver's health.
/// Absent means an older backend that only ever reported the app's own (treat as "app").
/// `error` can be an unreadable driver status or a capture denial latched for
/// either source, even when the grant booleans remain true.
export type PermissionStatus = { supported: boolean; accessibility: boolean; screen_recording: boolean; uia_note?: string | null; source?: "app" | "driver"; error?: string | null };

/// Whether the status is the app's own preflight (the app owns and bundles the
/// driver), the case the Grant-access banner is for. An external driver holds its
/// own grants, which the app cannot change, so that banner must not speak for it.
function appOwnsDriver(status: PermissionStatus | null | undefined): boolean {
  return status?.source !== "driver";
}

/// The backend supplies a capture-access problem independently of the grant
/// booleans. Keep it visible until an explicit request clears the problem.
export function permissionErrorNotice(status: PermissionStatus | null | undefined): string | null {
  if (!status?.error?.trim()) return null;
  const owner = appOwnsDriver(status) ? "Copilot" : "Capture driver";
  return `${owner}: ${status.error}`;
}

/// What to tell the user when macOS has not granted the app (and so its owned
/// driver) the access it needs, or null when nothing is missing. The driver is
/// reachable either way, so "driver connected" alone cannot reveal this. Only the
/// app-owned case: an external driver's grants belong to another identity, so
/// `driverNotice` speaks for it instead and this stays silent (no grant-the-app CTA).
export function permissionNotice(status: PermissionStatus | null | undefined): string | null {
  if (!appOwnsDriver(status) || !status?.supported) return null;
  const missing = [
    status.accessibility ? null : "Accessibility",
    status.screen_recording ? null : "Screen Recording",
  ].filter((name): name is string => name !== null);
  if (!missing.length) return null;
  return `Copilot needs ${missing.join(" and ")} access to see and drive windows. ` +
    "Choose Grant access. If no prompt appears, use the settings buttons below. " +
    "In System Settings > Privacy & Security, turn on Copilot (use + to add this app if missing), then quit and reopen Copilot.";
}

/// What to tell the user about an EXTERNAL driver's own grants (the app does not
/// own it, so it cannot be granted through Copilot). Null for the app-owned case
/// or when the driver has what it needs. A read error names that quietly rather
/// than demanding a grant; a genuine gap names the missing capability, to grant
/// to the driver, not to Copilot. No grant-the-app button is ever offered here.
export function driverNotice(status: PermissionStatus | null | undefined): string | null {
  if (appOwnsDriver(status) || !status) return null;
  if (status.error) return permissionErrorNotice(status);
  if (!status.supported) return null;
  const missing = [
    status.accessibility ? null : "Accessibility",
    status.screen_recording ? null : "Screen Recording",
  ].filter((name): name is string => name !== null);
  if (!missing.length) return null;
  return `The capture driver does not have ${missing.join(" and ")} access, so it cannot see and drive every window. ` +
    "Grant it to the driver, not to Copilot, in System Settings > Privacy & Security.";
}

/// Windows: the UIAccess helper cannot start from a per-user install. The backend
/// says so in `uia_note`; null when there is nothing to warn about.
export function uiAccessNotice(status: PermissionStatus | null | undefined): string | null {
  return status?.uia_note || null;
}

export type AgentAct = { action?: string; ok?: boolean; status?: string; code?: string; error?: string; step?: number; of?: number };

/// The log line for one step the agent attempted. A refused step (fed back to
/// the model, the run goes on) and a step that did not run read differently
/// from a delivered one and from a failure that ends the run.
export function actLine(d: AgentAct, escape: (s: string) => string): string {
  const action = escape(d.action || "");
  const at = d.step && d.of && d.of > 1 ? ` <span class="muted">(step ${d.step} of ${d.of})</span>` : "";
  switch (d.status ?? (d.ok ? "delivered" : "failed")) {
    case "delivered": return `<b>act</b> ${action}`;
    case "refused": return `<b>act·refused</b> ${action}${at} · ${escape(d.code || "refused")}: ${escape(d.error || "")} <span class="muted">(sent back to the model)</span>`;
    case "not_run": return `<span class="muted"><b>act·not run</b> ${action}${at}</span>`;
    default: return `<b>act·fail</b> ${action}${d.error ? " · " + escape(d.error) : ""}`;
  }
}

/// The log line for a model reply the parser refused. Nothing of it ran; the
/// error goes back to the model once before the run ends.
export function malformedLine(d: { error?: string }, escape: (s: string) => string): string {
  return `<b>reply·malformed</b> ${escape(d.error || "")} <span class="muted">(sent back to the model; nothing of it ran)</span>`;
}
