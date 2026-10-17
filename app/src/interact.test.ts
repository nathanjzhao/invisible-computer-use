import { describe, it, expect } from "vitest";
import { mapToWindow, scrollDelta, planKey, driveClick, mentionQuery, applyMention, parseMentions, readableGoal, resolveMentionTarget, WindowReferences, launchableApps, permissionNotice, permissionErrorNotice, driverNotice, actLine, malformedLine, type Rect } from "./interact";

// A decoded 1280x800 screenshot displayed in a 320x200 tile.
const TILE: Rect = { left: 0, top: 0, width: 320, height: 200 };
const WIN = { width: 1280, height: 800 };

describe("mapToWindow — click-through coordinate mapping", () => {
  it("rejects non-finite geometry and coordinates without dispatching", () => {
    for (const bad of [NaN, Infinity, -Infinity]) {
      expect(mapToWindow(TILE, WIN, bad, 10)).toBeNull();
      expect(mapToWindow({ ...TILE, left: bad }, WIN, 10, 10)).toBeNull();
      expect(mapToWindow(TILE, { ...WIN, width: bad }, 10, 10)).toBeNull();
    }
  });
  it("maps the image center to the window center", () => {
    expect(mapToWindow(TILE, WIN, 160, 100)).toEqual({ x: 640, y: 400 });
  });

  it("maps the corners exactly", () => {
    expect(mapToWindow(TILE, WIN, 0, 0)).toEqual({ x: 0, y: 0 });
    expect(mapToWindow(TILE, WIN, 320, 200)).toEqual({ x: 1279, y: 799 });
    expect(mapToWindow(TILE, WIN, 319.99, 199.99)).toEqual({ x: 1279, y: 799 });
  });

  it("is scale-independent (a 2x-sized tile maps the same fraction)", () => {
    const big: Rect = { left: 0, top: 0, width: 640, height: 400 };
    expect(mapToWindow(big, WIN, 160, 100)).toEqual({ x: 320, y: 200 }); // quarter point
  });

  it("accounts for a tile offset in the viewport (scrolled feed / second monitor tile)", () => {
    // Tile rendered at left=400, top=100. Clicking its center must still hit the
    // window's center, proving the map subtracts the tile's own origin.
    const offset: Rect = { left: 400, top: 100, width: 300, height: 150 };
    expect(mapToWindow(offset, { width: 600, height: 300 }, 400 + 150, 100 + 75)).toEqual({ x: 300, y: 150 });
  });

  it("maps correctly for a window with a negative/second-monitor origin (bounds carry the origin, not us)", () => {
    // The window is 800x600; a click near its top-left quarter maps to (200,150)
    // regardless of where the window physically sits — the driver adds the origin.
    expect(mapToWindow(TILE, { width: 800, height: 600 }, 80, 50)).toEqual({ x: 200, y: 150 });
  });

  it("clamps out-of-bounds clicks to the window edges", () => {
    expect(mapToWindow(TILE, WIN, -50, -50)).toEqual({ x: 0, y: 0 });
    expect(mapToWindow(TILE, WIN, 99999, 99999)).toEqual({ x: 1279, y: 799 });
  });

  it("returns null when the image has not laid out yet (zero-size rect) — no NaN to the driver", () => {
    expect(mapToWindow({ left: 0, top: 0, width: 0, height: 0 }, WIN, 10, 10)).toBeNull();
  });

  it("returns null for a zero-size or missing window bounds", () => {
    expect(mapToWindow(TILE, { width: 0, height: 0 }, 10, 10)).toBeNull();
    expect(mapToWindow(TILE, undefined, 10, 10)).toBeNull();
  });
});

describe("driveClick — full click-through dispatch (map → driver)", () => {
  const capture = () => {
    const calls: Array<[string, Record<string, unknown>]> = [];
    return { calls, invoke: (cmd: string, args: Record<string, unknown>) => { calls.push([cmd, args]); return Promise.resolve(); } };
  };

  it("maps the click and dispatches cua_click with window-logical coords", () => {
    const { calls, invoke } = capture();
    const p = driveClick(TILE, { pid: 42, window_id: 7, bounds: WIN }, 160, 100, "left", invoke);
    expect(p).toEqual({ x: 640, y: 400 });
    expect(calls).toEqual([["cua_click", { pid: 42, windowId: 7, x: 640, y: 400, button: "left" }]]);
  });

  it("dispatches nothing when the frame has not laid out", () => {
    const { calls, invoke } = capture();
    const p = driveClick({ left: 0, top: 0, width: 0, height: 0 }, { pid: 42, window_id: 7, bounds: WIN }, 10, 10, "left", invoke);
    expect(p).toBeNull();
    expect(calls).toEqual([]);
  });

  it("passes the button through", () => {
    const { calls, invoke } = capture();
    driveClick(TILE, { pid: 1, window_id: 2, bounds: WIN }, 320, 200, "right", invoke);
    expect(calls[0][1].button).toBe("right");
  });

  it("binds the click to the capture the tile is showing", () => {
    const { calls, invoke } = capture();
    driveClick(TILE, { pid: 42, window_id: 7, bounds: WIN, screenshot_id: "f0000abcd-12" }, 160, 100, "left", invoke);
    expect(calls).toEqual([["cua_click", { pid: 42, windowId: 7, x: 640, y: 400, button: "left", screenshotId: "f0000abcd-12" }]]);
  });

  it("sends no id when the displayed frame carried none", () => {
    const { calls, invoke } = capture();
    driveClick(TILE, { pid: 42, window_id: 7, bounds: WIN }, 160, 100, "left", invoke);
    expect("screenshotId" in calls[0][1]).toBe(false);
  });
});

describe("scrollDelta — natural wheel direction", () => {
  it("ignores horizontal-only and invalid wheel events", () => {
    expect(scrollDelta(0)).toBe(0);
    expect(scrollDelta(NaN)).toBe(0);
  });
  it("wheel down scrolls content down (negative), wheel up positive", () => {
    expect(scrollDelta(120)).toBe(-3);
    expect(scrollDelta(-120)).toBe(3);
  });
});

it("does not forward modified navigation keys as plain keys", () => {
  expect(planKey({ key: "Backspace", metaKey: true })).toEqual({ kind: "ignore" });
  expect(planKey({ key: "ArrowLeft", altKey: true })).toEqual({ kind: "ignore" });
});

describe("@-mention command parsing", () => {
  it("keeps aliases stable and distinct across normalized and generated name collisions", () => {
    const refs = new WindowReferences();
    const first = { pid: 1, window_id: 2, app_name: "Text Edit" };
    const suffixName = { pid: 1, window_id: 3, app_name: "TEXTEDIT-2" };
    const second = { pid: 1, window_id: 4, app_name: "text@edit" };
    const otherProcess = { ...first, pid: 9 };
    expect(refs.register(first)).toBe("TextEdit");
    expect(refs.register(suffixName)).toBe("TEXTEDIT-2");
    expect(refs.register(second)).toBe("textedit-3");
    expect(refs.register(otherProcess)).toBe("TextEdit-4");
    expect(refs.register({ ...first, app_name: "renamed" })).toBe("TextEdit");
    expect(refs.get("TEXTEDIT")?.app_name).toBe("renamed");
    expect(refs.get("textedit-2")).toBe(suffixName);
    expect(refs.get("TEXTEDIT-3")).toBe(second);
    expect(refs.get("TextEdit-4")).toBe(otherProcess);
    expect(refs.register({ pid: 5, window_id: 6, app_name: " @ " })).toBe("Window");
  });
  it("finds the @token being typed right before the caret", () => {
    expect(mentionQuery("in @Calc", 8)).toEqual({ at: 3, query: "Calc" });
    expect(mentionQuery("@", 1)).toEqual({ at: 0, query: "" }); // bare @ opens the list
  });

  it("has no active token once the mention is finished (space after it)", () => {
    expect(mentionQuery("in @Calculator do 5", 19)).toBeNull();
  });

  it("only considers the token at the caret, not earlier ones", () => {
    // caret sits after "compute"; the earlier @Calculator is complete, so no popover
    expect(mentionQuery("in @Calculator compute", 22)).toBeNull();
    // caret inside a second @token opens on that one
    expect(mentionQuery("@A vs @Bee", 10)).toEqual({ at: 6, query: "Bee" });
  });

  it("inserts @slug and a trailing space at the token, moving the caret past it", () => {
    expect(applyMention("in @Ca", 6, "Calculator")).toEqual({ value: "in @Calculator ", caret: 15 });
    // keeps text after the caret intact
    expect(applyMention("in @Ca do 5", 6, "Calculator")).toEqual({ value: "in @Calculator  do 5", caret: 15 });
  });

  it("inserts at the caret when there is no active @token", () => {
    expect(applyMention("hello ", 6, "Notes")).toEqual({ value: "hello @Notes ", caret: 13 });
  });

  it("extracts referenced slugs in order", () => {
    expect(parseMentions("in @Calculator and @Notes do x")).toEqual(["Calculator", "Notes"]);
    expect(parseMentions("no mentions here")).toEqual([]);
    expect(parseMentions("in @ type hello")).toEqual([""]);
    expect(parseMentions("@Calculator and @")).toEqual(["Calculator", ""]);
  });

  it("rewrites slugs to human names, leaving unknown ones as typed", () => {
    const nameOf = (s: string) => ({ googlechrome: "Google Chrome" })[s.toLowerCase()];
    expect(readableGoal("in @GoogleChrome open a tab", nameOf)).toBe("in Google Chrome open a tab");
    expect(readableGoal("in @Mystery do x", nameOf)).toBe("in @Mystery do x");
  });
  it("preserves email addresses and quoted literal handles", () => {
    const text = 'type user@TextEdit and "@handle" in @Editor';
    expect(parseMentions(text)).toEqual(["Editor"]);
    expect(mentionQuery("user@TextEdit", 13)).toBeNull();
    expect(readableGoal(text, s => s === "Editor" ? "TextEdit" : "wrong")).toBe('type user@TextEdit and "@handle" in TextEdit');
  });
  it("does not substitute a watched window for unresolved or mixed references", () => {
    const first = { pid: 1, window_id: 2 }, second = { pid: 1, window_id: 3 };
    const lookup = (s: string) => ({ first, second })[s as "first" | "second"];
    expect(resolveMentionTarget(["missing"], lookup, [first])).toEqual({ error: "unresolved" });
    expect(resolveMentionTarget(["first", "missing"], lookup, [first])).toEqual({ error: "unresolved" });
    expect(resolveMentionTarget(["first", "second"], lookup, [first])).toEqual({ error: "multiple" });
    expect(resolveMentionTarget(["first", "first"], lookup, [])).toEqual({ target: first });
    expect(resolveMentionTarget([], lookup, [first])).toEqual({ target: first });
    expect(resolveMentionTarget([], lookup, [first, second])).toEqual({ error: "required" });
  });
});

describe("launchableApps — @-bar launch rows", () => {
  const apps = [
    { name: "Calculator", bundle_id: "com.apple.calculator" },
    { name: "Notes", bundle_id: "com.apple.Notes" },
    { name: "Warp", bundle_id: "dev.warp.Warp-Stable" }, // already open below
    { name: "Numbers", bundle_id: "com.apple.iWork.Numbers" },
  ];
  it("excludes apps that already have an open window", () => {
    const out = launchableApps(apps, ["Warp"], "");
    expect(out.map((a) => a.name)).not.toContain("Warp");
    expect(out.map((a) => a.name)).toContain("Calculator");
  });
  it("filters by query (case-insensitive substring)", () => {
    expect(launchableApps(apps, [], "num").map((a) => a.name)).toEqual(["Numbers"]);
    expect(launchableApps(apps, [], "calc").map((a) => a.name)).toEqual(["Calculator"]);
  });
  it("dedupes by name and respects the limit", () => {
    const dupes = [{ name: "A" }, { name: "A" }, { name: "B" }, { name: "C" }];
    expect(launchableApps(dupes, [], "", 2).map((a) => a.name)).toEqual(["A", "B"]);
  });
  it("drops entries with no name", () => {
    expect(launchableApps([{ name: "" }, { name: "Ok" }], [], "").map((a) => a.name)).toEqual(["Ok"]);
  });
});

describe("planKey — keystroke routing", () => {
  it.each([{ isComposing: true }, { keyCode: 229 }])("ignores composition keys: %j", flags => {
    for (const key of ["x", "Enter", "Escape"]) {
      expect(planKey({ key, ...flags })).toEqual({ kind: "ignore" });
    }
  });

  it("routes printable characters to type_text", () => {
    expect(planKey({ key: "a" })).toEqual({ kind: "type", text: "a" });
    expect(planKey({ key: "7" })).toEqual({ kind: "type", text: "7" });
    expect(planKey({ key: " " })).toEqual({ kind: "type", text: " " });
  });

  it("routes named keys to press_key with the driver's vocabulary", () => {
    expect(planKey({ key: "Enter" })).toEqual({ kind: "key", key: "return" });
    expect(planKey({ key: "Backspace" })).toEqual({ kind: "key", key: "backspace" });
    expect(planKey({ key: "ArrowLeft" })).toEqual({ kind: "key", key: "left" });
    expect(planKey({ key: "Escape" })).toEqual({ kind: "key", key: "escape" });
  });

  it("ignores modifier combos so app/OS shortcuts pass through", () => {
    expect(planKey({ key: "c", metaKey: true })).toEqual({ kind: "ignore" });
    expect(planKey({ key: "r", ctrlKey: true })).toEqual({ kind: "ignore" });
  });
});

describe("permissionNotice: macOS grants for the embedded driver", () => {
  it("names each missing grant and how to fix it", () => {
    const both = permissionNotice({ supported: true, accessibility: false, screen_recording: false })!;
    expect(both).toContain("Accessibility and Screen Recording");
    expect(both).toContain("Privacy & Security");
    expect(permissionNotice({ supported: true, accessibility: true, screen_recording: false })).toMatch(/needs Screen Recording access/);
    expect(permissionNotice({ supported: true, accessibility: false, screen_recording: true })).toMatch(/needs Accessibility access/);
  });

  it("stays silent when granted, unsupported or unknown", () => {
    expect(permissionNotice({ supported: true, accessibility: true, screen_recording: true })).toBeNull();
    expect(permissionNotice({ supported: false, accessibility: true, screen_recording: true })).toBeNull();
    expect(permissionNotice(null)).toBeNull();
  });

  it("never uses an em dash in user-facing copy", () => {
    expect(permissionNotice({ supported: true, accessibility: false, screen_recording: false })).not.toContain("—");
  });

  it("stays out of an external driver's grants, whatever they say", () => {
    // The app cannot grant a driver it does not own, so the grant-the-app banner
    // never speaks for an external driver, even when the driver lacks a grant.
    expect(permissionNotice({ supported: true, accessibility: false, screen_recording: false, source: "driver" })).toBeNull();
    expect(permissionNotice({ supported: true, accessibility: true, screen_recording: true, source: "driver" })).toBeNull();
    // An absent source is an older backend reporting the app's own: unchanged.
    expect(permissionNotice({ supported: true, accessibility: false, screen_recording: true })).toMatch(/needs Accessibility access/);
    expect(permissionNotice({ supported: true, accessibility: false, screen_recording: true, source: "app" })).toMatch(/needs Accessibility access/);
  });
});

describe("permissionErrorNotice: explicit capture retry", () => {
  it.each(["app", "driver", undefined] as const)("keeps a denial visible with granted booleans for %s", source => {
    const error = "Screen capture was denied. Automatic capture is paused.";
    const status = { supported: true, accessibility: true, screen_recording: true, source, error };
    expect(permissionErrorNotice(status)).toBe(`${source === "driver" ? "Capture driver" : "Copilot"}: ${error}`);
    expect(permissionNotice(status)).toBeNull();
  });

  it("keeps unknown access distinct from a missing grant", () => {
    const status = { supported: true, accessibility: false, screen_recording: false, source: "driver" as const, error: "The selected driver's access could not be checked." };
    expect(permissionErrorNotice(status)).toContain("Capture driver: The selected driver's access could not be checked.");
    expect(permissionErrorNotice(status)).not.toContain("Copilot needs");
    expect(permissionNotice(status)).toBeNull();
  });

  it("does not invent a retry when there is no backend error", () => {
    expect(permissionErrorNotice({ supported: true, accessibility: false, screen_recording: false, error: null })).toBeNull();
    expect(permissionErrorNotice({ supported: true, accessibility: true, screen_recording: true, error: " " })).toBeNull();
    expect(permissionErrorNotice(null)).toBeNull();
  });
});

describe("driverNotice: an external driver's own grants", () => {
  it("stays silent when the driver has what it needs", () => {
    expect(driverNotice({ supported: true, accessibility: true, screen_recording: true, source: "driver" })).toBeNull();
    // A non-macOS driver has no such grants to miss.
    expect(driverNotice({ supported: false, accessibility: false, screen_recording: false, source: "driver" })).toBeNull();
  });

  it("names a genuine gap as the driver's, to grant to the driver not the app", () => {
    const both = driverNotice({ supported: true, accessibility: false, screen_recording: false, source: "driver" })!;
    expect(both).toContain("Accessibility and Screen Recording");
    expect(both).toContain("to the driver, not to Copilot");
    // Never the app-grant wording, so no grant-the-app CTA can be inferred.
    expect(both).not.toContain("Copilot needs");
    expect(driverNotice({ supported: true, accessibility: true, screen_recording: false, source: "driver" })).toMatch(/does not have Screen Recording/);
  });

  it("names an unreadable status quietly, without demanding a grant", () => {
    const note = driverNotice({ supported: true, accessibility: false, screen_recording: false, source: "driver", error: "The selected driver's access could not be checked." })!;
    expect(note).toContain("Capture driver: The selected driver's access could not be checked.");
    expect(note).not.toContain("Copilot needs");
    expect(note).not.toContain("to the driver, not to Copilot");
  });

  it("stays out of the app-owned case, source present or absent", () => {
    expect(driverNotice({ supported: true, accessibility: false, screen_recording: false, source: "app" })).toBeNull();
    expect(driverNotice({ supported: true, accessibility: false, screen_recording: false })).toBeNull();
    expect(driverNotice(null)).toBeNull();
  });

  it("never uses an em dash in user-facing copy", () => {
    expect(driverNotice({ supported: true, accessibility: false, screen_recording: false, source: "driver" })).not.toContain("—");
    expect(driverNotice({ supported: true, accessibility: true, screen_recording: true, source: "driver", error: "x" })).not.toContain("—");
  });
});

describe("actLine — agent step log", () => {
  const esc = (s: string) => s.replace(/</g, "&lt;");
  it("shows a refused step with its code, distinct from delivered and failed", () => {
    const refused = actLine({ action: "set \"Volume\" = \"150\"", ok: false, status: "refused", code: "set_value_out_of_range", error: "value <150> too high", step: 1, of: 2 }, esc);
    expect(refused).toContain("act·refused");
    expect(refused).toContain("(step 1 of 2)");
    expect(refused).toContain("set_value_out_of_range: value &lt;150> too high");
    expect(refused).toContain("sent back to the model");
    expect(actLine({ action: "click \"OK\"", ok: false, status: "not_run", step: 2, of: 2 }, esc)).toContain("act·not run</b> click \"OK\"");
    expect(actLine({ action: "click \"OK\"", ok: true, status: "delivered" }, esc)).toBe("<b>act</b> click \"OK\"");
    expect(actLine({ action: "type \"x\"", ok: false, status: "failed", error: "response lost" }, esc)).toBe("<b>act·fail</b> type \"x\" · response lost");
  });
  it("reads an event from before the status field by its ok flag", () => {
    expect(actLine({ action: "a", ok: true }, esc)).toBe("<b>act</b> a");
    expect(actLine({ action: "a", ok: false, error: "e" }, esc)).toBe("<b>act·fail</b> a · e");
  });
});

describe("malformedLine — a reply the parser refused", () => {
  it("names the parser error and says nothing ran", () => {
    const line = malformedLine({ error: "bad action: no kind and an unknown field `done`: <x>" }, (s) => s.replace(/</g, "&lt;"));
    expect(line).toContain("reply·malformed");
    expect(line).toContain("unknown field `done`: &lt;x>");
    expect(line).toContain("nothing of it ran");
  });
});
