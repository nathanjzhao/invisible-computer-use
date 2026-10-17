import { describe, expect, it } from "vitest";
import { InputQueue } from "./input-queue";

describe("manual input ordering", () => {
  it("holds later gestures until earlier work completes", async () => {
    const queue = new InputQueue(), events: string[] = [];
    let release!: () => void;
    const first = queue.enqueue(() => new Promise<void>(resolve => {
      events.push("click started");
      release = () => { events.push("click finished"); resolve(); };
    }));
    const second = queue.enqueue(async () => { events.push("type"); });
    await Promise.resolve();
    expect(events).toEqual(["click started"]);
    expect(queue.busy).toBe(true);
    release();
    await Promise.all([first, second]);
    expect(events).toEqual(["click started", "click finished", "type"]);
    expect(queue.busy).toBe(false);
  });

  it("discards queued input after failure and accepts a later explicit gesture", async () => {
    const queue = new InputQueue(), events: string[] = [];
    const first = queue.enqueue(async () => { throw new Error("click failed"); });
    const second = queue.enqueue(async () => { events.push("unsafe type"); });
    await expect(first).rejects.toThrow("click failed");
    await second;
    expect(events).toEqual([]);
    await queue.enqueue(async () => { events.push("new gesture"); });
    expect(events).toEqual(["new gesture"]);
    expect(queue.busy).toBe(false);
  });

  it("cancels pending work without claiming to undo in-flight input", async () => {
    const queue = new InputQueue(), events: string[] = [];
    let release!: () => void;
    const first = queue.enqueue(() => new Promise<void>(resolve => {
      release = () => { events.push("in-flight finished"); resolve(); };
    }));
    const cancelled = queue.enqueue(async () => { events.push("cancelled"); });
    await Promise.resolve();
    queue.cancel();
    const next = queue.enqueue(async () => { events.push("new target"); });
    expect(queue.busy).toBe(true);
    release();
    await Promise.all([first, cancelled, next]);
    expect(events).toEqual(["in-flight finished", "new target"]);
    expect(queue.busy).toBe(false);
  });
});
