// @vitest-environment jsdom
// The tile's real wheel listener (wireDrive), driven with DOM wheel events: what it sends the driver. The click path has its
// own test (interact.test.ts, driveClick); this is the wheel path, which carried no test of its own.
import { describe, it, expect } from "vitest";
import { wireDrive, type Rect, type Target } from "./interact";

const TILE: Rect = { left: 0, top: 0, width: 320, height: 200 };
const WIN = { width: 1280, height: 800 };

function tile(target: Target, extra: { blocked?: boolean } = {}) {
  const el = document.createElement("div");
  const calls: [string, Record<string, unknown>][] = [];
  const scrolls: string[] = [];
  wireDrive(el, {
    invoke: (cmd, args) => { calls.push([cmd, args]); },
    rect: () => TILE,
    target: () => target,
    blocked: () => !!extra.blocked,
    onScroll: (_p, dir) => scrolls.push(dir),
  });
  const wheel = (deltaY: number, clientX = 160, clientY = 100) =>
    el.dispatchEvent(new WheelEvent("wheel", { deltaY, clientX, clientY, bubbles: true, cancelable: true }));
  return { calls, scrolls, wheel };
}

describe("tile wheel: what the gesture sends the driver", () => {
  it("names the window, the frame the point was read from, the mapped point and the natural direction", () => {
    const t = tile({ pid: 42, window_id: 7, bounds: WIN, screenshot_id: "f0000abcd-12" });
    t.wheel(100);
    expect(t.calls).toEqual([["cua_scroll", { pid: 42, windowId: 7, x: 640, y: 400, dy: -3, amount: 3, screenshotId: "f0000abcd-12" }]]);
    expect(t.scrolls).toEqual(["down"]);
  });

  it("sends no frame id when the tile has none yet, and nothing at all when the tile is blocked or the delta is 0", () => {
    const none = tile({ pid: 42, window_id: 7, bounds: WIN });
    none.wheel(-100);
    expect("screenshotId" in none.calls[0][1]).toBe(false);
    expect(none.calls[0][1].dy).toBe(3);
    const blocked = tile({ pid: 42, window_id: 7, bounds: WIN, screenshot_id: "f1-1" }, { blocked: true });
    blocked.wheel(100);
    expect(blocked.calls).toEqual([]);
    const flat = tile({ pid: 42, window_id: 7, bounds: WIN, screenshot_id: "f1-1" });
    flat.wheel(0);
    expect(flat.calls).toEqual([]);
  });
});
