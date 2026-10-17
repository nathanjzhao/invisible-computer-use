// @vitest-environment jsdom
import { describe, it, expect, vi, beforeEach } from "vitest";
import { findUpdate, fitsInstall, showUpdateToast, withOwnInstall, type UpdateEvent, type UpdateLike } from "./updater";

beforeEach(() => { document.body.innerHTML = ""; vi.stubGlobal("requestAnimationFrame", (f: () => void) => f()); });

const toast = () => document.querySelector(".update-toast") as HTMLElement | null;
const click = (sel: string) => (toast()!.querySelector(sel) as HTMLElement).click();
const settle = () => new Promise((r) => setTimeout(r, 0));

function update(run: (emit: (ev: UpdateEvent) => void) => Promise<void>, version = "0.2.0"): UpdateLike {
  return { version, downloadAndInstall: (onEvent) => run(onEvent) };
}

describe("the quiet check", () => {
  it("is silent when the check fails for any reason", async () => {
    for (const why of [new Error("connection refused"), "timeout", { code: 500 }, undefined]) {
      expect(await findUpdate(() => Promise.reject(why))).toBeNull();
    }
    expect(toast()).toBeNull();
  });
  it("returns the update when there is one and null when there is none", async () => {
    const u = update(async () => {});
    expect(await findUpdate(async () => u)).toBe(u);
    expect(await findUpdate(async () => null)).toBeNull();
  });
});

describe("the install toast", () => {
  it("names the version, escaped, and shows once", () => {
    const deps = { relaunch: vi.fn(), log: vi.fn() };
    showUpdateToast(update(async () => {}, `0.2.0<img src=x onerror=alert(1)>`), deps);
    showUpdateToast(update(async () => {}), deps);
    expect(document.querySelectorAll(".update-toast").length).toBe(1);
    expect(toast()!.querySelector("img")).toBeNull();
    expect(toast()!.textContent).toContain("0.2.0<img");
  });

  it("downloads with progress and relaunches only after the install finished", async () => {
    const order: string[] = [];
    const deps = { relaunch: vi.fn(async () => { order.push("relaunch"); }), log: vi.fn() };
    showUpdateToast(update(async (emit) => {
      emit({ event: "Started", data: { contentLength: 200 } });
      emit({ event: "Progress", data: { chunkLength: 50 } });
      expect(toast()!.querySelector(".up-msg")!.textContent).toBe("Downloading 25%");
      emit({ event: "Progress", data: { chunkLength: 150 } });
      expect(toast()!.querySelector(".up-msg")!.textContent).toBe("Downloading 100%");
      emit({ event: "Finished" });
      order.push("installed");
    }), deps);
    click(".up-install");
    await settle();
    expect(order).toEqual(["installed", "relaunch"]);
    expect(deps.log).not.toHaveBeenCalled();
  });

  it("does not install or relaunch while a task is running, and installs once it is not", async () => {
    let running = true;
    const run = vi.fn(async () => {});
    const deps = { relaunch: vi.fn(async () => {}), log: vi.fn(), busy: () => running };
    showUpdateToast({ version: "0.2.0", downloadAndInstall: run }, deps);
    click(".up-install");
    await settle();
    expect(run).not.toHaveBeenCalled();
    expect(deps.relaunch).not.toHaveBeenCalled();
    expect(toast()!.querySelector(".up-msg")!.textContent).toContain("task is running");
    expect((toast()!.querySelector(".up-install") as HTMLButtonElement).disabled).toBe(false);
    running = false;
    click(".up-install");
    await settle();
    expect(run).toHaveBeenCalledTimes(1);
    expect(deps.relaunch).toHaveBeenCalledTimes(1);
  });

  it("tells a user who cannot elevate to ask an administrator, and never downloads", async () => {
    const reason = "Copilot is installed for all users. Ask an administrator to run the update.";
    const run = vi.fn(async () => {});
    const deps = { relaunch: vi.fn(async () => {}), log: vi.fn(), blocked: vi.fn(async () => reason) };
    showUpdateToast({ version: "0.2.0", downloadAndInstall: run }, deps);
    await settle();
    expect(toast()!.querySelector(".up-msg")!.textContent).toBe(reason);
    expect(toast()!.querySelector(".up-install")).toBeNull();
    expect(run).not.toHaveBeenCalled();
    expect(deps.relaunch).not.toHaveBeenCalled();
  });

  it("refuses at the click as well, when the answer arrives after the toast was shown", async () => {
    let answer: string | null = null;
    const run = vi.fn(async () => {});
    const deps = { relaunch: vi.fn(async () => {}), log: vi.fn(), blocked: async () => answer };
    showUpdateToast({ version: "0.2.0", downloadAndInstall: run }, deps);
    await settle();
    expect(toast()!.querySelector(".up-install")).not.toBeNull();
    answer = "ask an administrator";
    click(".up-install");
    await settle();
    expect(run).not.toHaveBeenCalled();
    expect(toast()!.querySelector(".up-msg")!.textContent).toBe("ask an administrator");
  });

  it("installs as before when the user can elevate or the check itself fails", async () => {
    for (const blocked of [async () => null, async () => { throw new Error("no such command"); }]) {
      document.body.innerHTML = "";
      const run = vi.fn(async () => {});
      const deps = { relaunch: vi.fn(async () => {}), log: vi.fn(), blocked };
      showUpdateToast({ version: "0.2.0", downloadAndInstall: run }, deps);
      await settle();
      click(".up-install");
      await settle();
      expect(run).toHaveBeenCalledTimes(1);
      expect(deps.relaunch).toHaveBeenCalledTimes(1);
    }
  });

  it("does not relaunch when the signature check or the install fails, and says so", async () => {
    const deps = { relaunch: vi.fn(), log: vi.fn() };
    showUpdateToast(update(async () => { throw new Error("signature verification failed <b>x</b>"); }), deps);
    click(".up-install");
    await settle();
    expect(deps.relaunch).not.toHaveBeenCalled();
    expect(toast()!.querySelector(".up-msg")!.textContent).toBe("Update failed");
    expect((toast()!.querySelector(".up-install") as HTMLButtonElement).disabled).toBe(false);
    expect(deps.log).toHaveBeenCalledTimes(1);
    const logged = deps.log.mock.calls[0][0] as string;
    expect(logged).toContain("signature verification failed");
    expect(logged).not.toContain("<b>x</b>");
  });

  it("can be tried again after a failure, and dismissed", async () => {
    let calls = 0;
    const deps = { relaunch: vi.fn(async () => {}), log: vi.fn() };
    showUpdateToast(update(async () => { if (++calls === 1) throw new Error("network"); }), deps);
    click(".up-install"); await settle();
    click(".up-install"); await settle();
    expect(calls).toBe(2);
    expect(deps.relaunch).toHaveBeenCalledTimes(1);
    click(".up-dismiss");
    expect(toast()).toBeNull();
  });
});

describe("an all-users install run by the backend", () => {
  const plugin = (run = vi.fn(async () => {})): UpdateLike => ({ version: "0.2.0", downloadAndInstall: run });

  it("uses the backend's install only when the backend says it owns this install", async () => {
    const pluginRun = vi.fn(async () => {});
    const install = vi.fn(async () => ({ status: "installed" as const }));
    const own = await withOwnInstall(plugin(pluginRun), { owns: async () => true, install });
    await own.downloadAndInstall(() => {});
    expect(install).toHaveBeenCalledTimes(1);
    expect(pluginRun).not.toHaveBeenCalled();
    for (const owns of [async () => false, async () => { throw new Error("no such command"); }]) {
      const p = plugin();
      expect(await withOwnInstall(p, { owns, install })).toBe(p);
    }
  });

  it("keeps the app running, relaunches nothing and says why when the elevation was cancelled", async () => {
    const message = "Update cancelled: administrator approval was not given. Copilot was not changed.";
    const deps = { relaunch: vi.fn(async () => {}), log: vi.fn() };
    const own = await withOwnInstall(plugin(), { owns: async () => true, install: async () => ({ status: "kept" as const, message }) });
    showUpdateToast(own, deps);
    click(".up-install"); await settle();
    expect(deps.relaunch).not.toHaveBeenCalled();
    expect(toast()!.querySelector(".up-msg")!.textContent).toBe(message);
    expect((toast()!.querySelector(".up-install") as HTMLButtonElement).disabled).toBe(false);
    expect(deps.log).not.toHaveBeenCalled();
  });

  it("does not relaunch after an install the installer already restarted", async () => {
    const deps = { relaunch: vi.fn(async () => {}), log: vi.fn() };
    const own = await withOwnInstall(plugin(), { owns: async () => true, install: async () => ({ status: "installed" as const }) });
    showUpdateToast(own, deps);
    click(".up-install"); await settle();
    expect(deps.relaunch).not.toHaveBeenCalled();
  });
});

describe("a .deb or .rpm install checked by the backend before the plugin installs it", () => {
  const plugin = (): UpdateLike => ({ version: "0.2.0", downloadAndInstall: async () => {} });
  const needs = "This update needs libwebkit2gtk-4.1.so.0()(64bit), which this computer does not have. Install it (sudo dnf install 'libwebkit2gtk-4.1.so.0()(64bit)') and update again. Copilot was not changed and is still running.";

  it("puts the missing dependency in the toast in full, relaunches nothing and leaves the button usable", async () => {
    const deps = { relaunch: vi.fn(async () => {}), log: vi.fn() };
    const own = await withOwnInstall(plugin(), { owns: async () => true, install: async () => ({ status: "kept" as const, message: needs }) });
    showUpdateToast(own, deps);
    click(".up-install"); await settle();
    expect(deps.relaunch).not.toHaveBeenCalled();
    expect(toast()!.querySelector(".up-msg")!.textContent).toBe(needs);
    expect((toast()!.querySelector(".up-install") as HTMLButtonElement).disabled).toBe(false);
    expect(deps.log).not.toHaveBeenCalled();
  });

  it("is a live region, so the refusal is announced and reaches the accessibility tree", () => {
    showUpdateToast(plugin(), { relaunch: vi.fn(async () => {}), log: vi.fn() });
    expect(toast()!.getAttribute("role")).toBe("status");
  });

  it("restarts the app after the plugin installed the package (the package manager does not)", async () => {
    const deps = { relaunch: vi.fn(async () => {}), log: vi.fn() };
    const own = await withOwnInstall(plugin(), { owns: async () => true, install: async () => ({ status: "relaunch" as const }) });
    showUpdateToast(own, deps);
    click(".up-install"); await settle();
    expect(deps.relaunch).toHaveBeenCalledTimes(1);
  });

  it("shows rpm's own reason when the install failed after the check", async () => {
    const deps = { relaunch: vi.fn(async () => {}), log: vi.fn() };
    const why = "rpm refused the update: error: x: does not verify: no signature (Failed to install package)";
    const own = await withOwnInstall(plugin(), { owns: async () => true, install: async () => { throw why; } });
    showUpdateToast(own, deps);
    click(".up-install"); await settle();
    expect(deps.relaunch).not.toHaveBeenCalled();
    expect(deps.log).toHaveBeenCalledTimes(1);
    expect(String(deps.log.mock.calls[0][0])).toContain("does not verify: no signature (Failed to install package)");
  });
});

describe("an MSI install is offered only an MSI", () => {
  const offer = (platforms: Record<string, unknown>): UpdateLike => ({ version: "0.2.0", rawJson: { platforms }, downloadAndInstall: async () => {} });
  const entry = { url: "https://example.invalid/x", signature: "sig" };

  it("refuses the bare windows entry (the NSIS installer) and accepts an MSI entry", () => {
    expect(fitsInstall(offer({ "windows-x86_64": entry }), "msi")).toBeNull();
    expect(fitsInstall(offer({ "windows-x86_64": entry, "windows-x86_64-nsis": entry }), "msi")).toBeNull();
    const withMsi = offer({ "windows-x86_64": entry, "windows-x86_64-msi": entry });
    expect(fitsInstall(withMsi, "msi")).toBe(withMsi);
    const arm = offer({ "windows-aarch64-msi": entry });
    expect(fitsInstall(arm, "msi")).toBe(arm);
  });

  it("leaves every other install, and no offer at all, alone", () => {
    const bare = offer({ "windows-x86_64": entry });
    for (const kind of ["all_users_nsis", "per_user_nsis", "unknown", ""]) expect(fitsInstall(bare, kind)).toBe(bare);
    expect(fitsInstall(null, "msi")).toBeNull();
    // a manifest the plugin could not describe is not an MSI entry
    expect(fitsInstall({ version: "0.2.0", downloadAndInstall: async () => {} }, "msi")).toBeNull();
  });
});
