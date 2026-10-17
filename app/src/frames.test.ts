// @vitest-environment jsdom
import { afterEach, describe, expect, it, vi } from "vitest";
import { loadFrame, releaseFrame } from "./frames";

afterEach(() => { vi.useRealTimers(); vi.restoreAllMocks(); vi.unstubAllGlobals(); });

describe("native frame loading", () => {
  it("bounds image decoding as well as the network fetch", async () => {
    vi.useFakeTimers();
    vi.stubGlobal("fetch", vi.fn().mockResolvedValue({ ok: true, status: 200, blob: async () => new Blob(["image"]) }));
    vi.stubGlobal("Image", class { src = ""; decode() { return new Promise(() => {}); } });
    const revoke = vi.fn();
    vi.stubGlobal("URL", { createObjectURL: () => "blob:pending", revokeObjectURL: revoke });
    const pending = loadFrame(document.createElement("img"), "frame://localhost/win/1/2/640");
    await vi.advanceTimersByTimeAsync(4001);
    expect(await pending).toBe(false);
    expect(revoke).toHaveBeenCalledWith("blob:pending");
  });
  it("fetches once, shows the decoded bytes, and releases replaced frames", async () => {
    const fetcher = vi.fn().mockResolvedValue({ ok: true, status: 200, blob: async () => new Blob(["image"]) });
    vi.stubGlobal("fetch", fetcher);
    vi.stubGlobal("Image", class { src = ""; decode() { return Promise.resolve(); } });
    let serial = 0;
    const revoke = vi.fn();
    vi.stubGlobal("URL", { createObjectURL: () => `blob:frame-${++serial}`, revokeObjectURL: revoke });
    const img = document.createElement("img");
    expect(await loadFrame(img, "frame://localhost/win/1/2/640")).toBe(true);
    expect(fetcher).toHaveBeenCalledTimes(1);
    expect(img.src).toBe("blob:frame-1");
    expect(await loadFrame(img, "frame://localhost/win/1/2/640")).toBe(true);
    expect(revoke).toHaveBeenCalledWith("blob:frame-1");
    releaseFrame(img);
    expect(revoke).toHaveBeenCalledWith("blob:frame-2");
  });
  it("records the displayed frame's screenshot id with the frame", async () => {
    const headers = (id?: string) => ({ get: (name: string) => (name.toLowerCase() === "x-screenshot-id" ? id ?? null : null) });
    vi.stubGlobal("fetch", vi.fn()
      .mockResolvedValueOnce({ ok: true, status: 200, headers: headers("f0000abcd-1"), blob: async () => new Blob(["a"]) })
      .mockResolvedValueOnce({ ok: false, status: 422, headers: headers(), text: async () => "minimized (window_minimized)" })
      .mockResolvedValueOnce({ ok: true, status: 200, headers: headers(), blob: async () => new Blob(["b"]) }));
    vi.stubGlobal("Image", class { src = ""; decode() { return Promise.resolve(); } });
    let serial = 0;
    vi.stubGlobal("URL", { createObjectURL: () => `blob:frame-${++serial}`, revokeObjectURL: vi.fn() });
    const img = document.createElement("img");
    expect(await loadFrame(img, "frame://localhost/win/1/2/640")).toBe(true);
    expect(img.dataset.screenshotId).toBe("f0000abcd-1");
    // A refusal keeps the old frame on screen, so it keeps that frame's id.
    expect(await loadFrame(img, "frame://localhost/win/1/2/640")).toBe(false);
    expect(img.dataset.screenshotId).toBe("f0000abcd-1");
    // A frame without an id must not inherit the previous frame's.
    expect(await loadFrame(img, "frame://localhost/win/1/2/640")).toBe(true);
    expect(img.dataset.screenshotId).toBeUndefined();
    img.dataset.screenshotId = "stale";
    releaseFrame(img);
    expect(img.dataset.screenshotId).toBeUndefined();
  });
  it("revalidates with the last ETag and keeps the frame on 304", async () => {
    const headers = (h: Record<string, string>) => ({ get: (name: string) => h[name.toLowerCase()] ?? null });
    const fetcher = vi.fn()
      .mockResolvedValueOnce({ ok: true, status: 200, headers: headers({ etag: '"sid-1"', "x-screenshot-id": "sid-1" }), blob: async () => new Blob(["a"]) })
      .mockResolvedValueOnce({ ok: false, status: 304, headers: headers({ etag: '"sid-1"' }) });
    vi.stubGlobal("fetch", fetcher);
    vi.stubGlobal("Image", class { src = ""; decode() { return Promise.resolve(); } });
    vi.stubGlobal("URL", { createObjectURL: () => "blob:frame-1", revokeObjectURL: vi.fn() });
    const img = document.createElement("img");
    expect(await loadFrame(img, "frame://localhost/win/1/2/640")).toBe(true);
    expect(img.src).toBe("blob:frame-1");
    expect(img.dataset.frameEtag).toBe('"sid-1"');
    expect(img.dataset.screenshotId).toBe("sid-1");
    // Second poll: the stored ETag is offered, the server answers 304, and the
    // displayed frame and its id are kept with no new blob.
    expect(await loadFrame(img, "frame://localhost/win/1/2/640")).toBe(true);
    expect(fetcher.mock.calls[1][1].headers["If-None-Match"]).toBe('"sid-1"');
    expect(img.src).toBe("blob:frame-1");
    expect(img.dataset.screenshotId).toBe("sid-1");
    expect(img.dataset.frameEtag).toBe('"sid-1"');
  });
  it("keeps the last good frame on capture refusal", async () => {
    vi.stubGlobal("fetch", vi.fn().mockResolvedValue({ ok: true, status: 204 }));
    const img = document.createElement("img");
    img.src = "blob:previous";
    expect(await loadFrame(img, "frame://localhost/win/1/2/640")).toBe(false);
    expect(img.src).toBe("blob:previous");
  });
  it("records the driver's typed refusal and clears it once a frame arrives", async () => {
    vi.stubGlobal("fetch", vi.fn().mockResolvedValueOnce({ ok: false, status: 422, text: async () => "window is minimized. Restore it to see it here. (window_minimized)\n" })
      .mockResolvedValueOnce({ ok: true, status: 200, blob: async () => new Blob(["image"]) }));
    vi.stubGlobal("Image", class { src = ""; decode() { return Promise.resolve(); } });
    vi.stubGlobal("URL", { createObjectURL: () => "blob:restored", revokeObjectURL: vi.fn() });
    const img = document.createElement("img");
    img.src = "blob:previous";
    expect(await loadFrame(img, "frame://localhost/win/1/2/640")).toBe(false);
    expect(img.dataset.frameRefusal).toBe("window is minimized. Restore it to see it here. (window_minimized)");
    expect(img.src).toBe("blob:previous");
    expect(await loadFrame(img, "frame://localhost/win/1/2/640")).toBe(true);
    expect(img.dataset.frameRefusal).toBeUndefined();
  });
  it("does not treat an untyped capture failure as a refusal", async () => {
    vi.stubGlobal("fetch", vi.fn().mockResolvedValue({ ok: true, status: 204 }));
    const img = document.createElement("img");
    expect(await loadFrame(img, "frame://localhost/win/1/2/640")).toBe(false);
    expect(img.dataset.frameRefusal).toBeUndefined();
  });
  it("does not publish or retain a frame whose tile was disposed during decode", async () => {
    let started!: () => void;
    let finish!: () => void;
    const decoding = new Promise<void>(resolve => { started = resolve; });
    vi.stubGlobal("fetch", vi.fn().mockResolvedValue({ ok: true, status: 200, blob: async () => new Blob(["image"]) }));
    vi.stubGlobal("Image", class {
      src = "";
      decode() { started(); return new Promise<void>(resolve => { finish = resolve; }); }
    });
    const revoke = vi.fn();
    vi.stubGlobal("URL", { createObjectURL: () => "blob:disposed", revokeObjectURL: revoke });
    const img = document.createElement("img");
    const pending = loadFrame(img, "frame://localhost/win/1/2/640");
    await decoding;
    releaseFrame(img);
    finish();
    expect(await pending).toBe(false);
    expect(img.getAttribute("src")).toBeNull();
    expect(revoke).toHaveBeenCalledWith("blob:disposed");
  });
  it("keeps the newest result when an older decode finishes afterward", async () => {
    const finish: (() => void)[] = [];
    let firstReady!: () => void;
    let ready!: () => void;
    const firstDecoding = new Promise<void>(resolve => { firstReady = resolve; });
    const bothDecoding = new Promise<void>(resolve => { ready = resolve; });
    vi.stubGlobal("fetch", vi.fn().mockResolvedValue({ ok: true, status: 200, blob: async () => new Blob(["image"]) }));
    vi.stubGlobal("Image", class {
      src = "";
      decode() { return new Promise<void>(resolve => { finish.push(resolve); if (finish.length === 1) firstReady(); if (finish.length === 2) ready(); }); }
    });
    let serial = 0;
    const revoke = vi.fn();
    vi.stubGlobal("URL", { createObjectURL: () => `blob:frame-${++serial}`, revokeObjectURL: revoke });
    const img = document.createElement("img");
    const older = loadFrame(img, "frame://localhost/old");
    await firstDecoding;
    const newer = loadFrame(img, "frame://localhost/new");
    await bothDecoding;
    finish[1]();
    expect(await newer).toBe(true);
    finish[0]();
    expect(await older).toBe(false);
    expect(img.src).toBe("blob:frame-2");
    expect(revoke).toHaveBeenCalledWith("blob:frame-1");
    releaseFrame(img);
  });
});
