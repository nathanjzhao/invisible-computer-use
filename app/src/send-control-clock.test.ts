import { describe, it, expect, vi } from "vitest";
import { sendControl } from "./send-control";

describe("the Stop grace clock", () => {
  it("is monotonic: a wall-clock step back does not keep Stop dead", () => {
    const mono = vi.spyOn(performance, "now");
    const wall = vi.spyOn(Date, "now");
    let t = 5_000;
    mono.mockImplementation(() => t);
    wall.mockImplementation(() => 1_000_000);
    let running = false; let stops = 0;
    const form = sendControl({ ready: () => true, statusError: () => false, retryStatus: () => {}, running: () => running,
      stop: () => { stops++; }, start: () => { running = true; return true; } });
    const submit = () => form({ preventDefault() {} } as unknown as Event);
    submit();
    wall.mockImplementation(() => 1_000_000 - 3_600_000); // the wall clock jumps back an hour
    t += 1_000;
    submit();
    expect(stops).toBe(1);
    mono.mockRestore(); wall.mockRestore();
  });

  it("tells the user when a Stop inside the grace window is ignored", () => {
    let t = 0; let running = false; let stops = 0; let ignored = 0;
    const form = sendControl({ ready: () => true, statusError: () => false, retryStatus: () => {}, running: () => running,
      stop: () => { stops++; }, start: () => { running = true; return true; }, now: () => t, ignored: () => { ignored++; } });
    const submit = () => form({ preventDefault() {} } as unknown as Event);
    submit(); t = 500; submit();
    expect([stops, ignored]).toEqual([0, 1]);
    t = 900; submit();
    expect([stops, ignored]).toEqual([1, 1]);
  });
});
