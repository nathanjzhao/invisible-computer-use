// @vitest-environment jsdom
//
// Why this file exists: the tile img click-through "isn't scriptable" by
// cua-driver because the driver's synthetic click is an AX press — it never
// emits a DOM pointer event into the WKWebView, so it can't fire the <img>'s
// mousedown/wheel listeners. A real trackpad/mouse does. So the honest place to
// prove the gesture path is correct is at the DOM-event layer: synthesize the
// exact events a real mouse fires and assert they reach the driver with the
// right screenshot-pixel coordinates. That's what wireDrive was extracted for.

import { describe, it, expect, vi } from "vitest";
import { wireDrive, type DriveHooks, type Rect, type Target } from "./interact";

const TILE: Rect = { left: 0, top: 0, width: 320, height: 200 };
const WIN = { width: 1280, height: 800 };

function harness(over: Partial<DriveHooks> = {}) {
  const calls: Array<[string, Record<string, unknown>]> = [];
  const invoke = (cmd: string, args: Record<string, unknown>) => { calls.push([cmd, args]); return Promise.resolve(); };
  const onEngage = vi.fn(), onBlocked = vi.fn(), onClick = vi.fn(), onScroll = vi.fn();
  const el = document.createElement("div");
  document.body.append(el);
  const target: Target = { pid: 42, window_id: 7, bounds: WIN };
  wireDrive(el, { invoke, rect: () => TILE, target: () => target, onEngage, onBlocked, onClick, onScroll, ...over });
  return { el, calls, onEngage, onBlocked, onClick, onScroll };
}

// A wheel event jsdom will dispatch with the fields wireDrive reads.
function wheel(deltaY: number, clientX: number, clientY: number): WheelEvent {
  const e = new Event("wheel", { cancelable: true, bubbles: true });
  Object.defineProperty(e, "deltaY", { value: deltaY });
  Object.defineProperty(e, "clientX", { value: clientX });
  Object.defineProperty(e, "clientY", { value: clientY });
  return e as WheelEvent;
}

describe("wireDrive — a synthesized pointer gesture reaches the driver", () => {
  it("a left mousedown at the tile center clicks the window center", () => {
    const { el, calls, onEngage, onClick } = harness();
    el.dispatchEvent(new MouseEvent("mousedown", { button: 0, clientX: 160, clientY: 100, cancelable: true }));
    expect(calls).toEqual([["cua_click", { pid: 42, windowId: 7, x: 640, y: 400, button: "left" }]]);
    expect(onEngage).toHaveBeenCalledOnce();
    expect(onClick).toHaveBeenCalledWith({ x: 640, y: 400 }, expect.any(MouseEvent));
  });

  it("a right mousedown sends button:right and suppresses the context menu", () => {
    const { el, calls } = harness();
    const ctx = new MouseEvent("contextmenu", { cancelable: true });
    el.dispatchEvent(ctx);
    expect(ctx.defaultPrevented).toBe(true);
    el.dispatchEvent(new MouseEvent("mousedown", { button: 2, clientX: 320, clientY: 200, cancelable: true }));
    expect(calls[0][1].button).toBe("right");
    expect(calls[0][1]).toMatchObject({ x: 1279, y: 799 });
  });

  it("a wheel gesture scrolls the window in the natural direction", () => {
    const { el, calls, onScroll } = harness();
    el.dispatchEvent(wheel(120, 160, 100));
    expect(calls).toEqual([["cua_scroll", { pid: 42, windowId: 7, x: 640, y: 400, dy: -3, amount: 3 }]]);
    expect(onScroll).toHaveBeenCalledWith({ x: 640, y: 400 }, "down", expect.anything());
  });

  it("a wheel gesture names the frame the tile shows, as a click does", () => {
    const { el, calls } = harness({ target: () => ({ pid: 42, window_id: 7, bounds: WIN, screenshot_id: "f0000abcd-12" }) });
    el.dispatchEvent(wheel(120, 160, 100));
    expect(calls).toEqual([["cua_scroll", { pid: 42, windowId: 7, x: 640, y: 400, dy: -3, amount: 3, screenshotId: "f0000abcd-12" }]]);
  });

  it("when blocked (off-Space) the gesture engages the UI but dispatches nothing", () => {
    const { el, calls, onEngage, onBlocked } = harness({ blocked: () => true });
    el.dispatchEvent(new MouseEvent("mousedown", { button: 0, clientX: 160, clientY: 100, cancelable: true }));
    expect(calls).toEqual([]);
    expect(onBlocked).toHaveBeenCalledOnce();
    expect(onEngage).toHaveBeenCalledOnce(); // taking control still registers
  });

  it("a gesture on a not-yet-laid-out frame dispatches nothing (no NaN to the driver)", () => {
    const { el, calls } = harness({ rect: () => ({ left: 0, top: 0, width: 0, height: 0 }) });
    el.dispatchEvent(new MouseEvent("mousedown", { button: 0, clientX: 10, clientY: 10, cancelable: true }));
    expect(calls).toEqual([]);
  });
});
