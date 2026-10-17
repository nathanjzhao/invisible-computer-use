// @vitest-environment jsdom
import { describe, it, expect, vi, afterEach } from "vitest";
import { readFileSync, readdirSync } from "node:fs";
import { pathToFileURL } from "node:url";
import { inlineConfirm, deleteAllMemories } from "./inline-confirm";

afterEach(() => { document.body.innerHTML = ""; });

function host() {
  document.body.innerHTML = `<div class="mem-foot"><button id="mem-delete-all">Delete all memories</button></div>`;
  return document.querySelector<HTMLElement>(".mem-foot")!;
}
const press = (sel: string) => document.querySelector<HTMLButtonElement>(sel)!.click();

describe("in-page confirmation", () => {
  it("Delete all asks first and deletes only after the confirm button", async () => {
    const del = vi.fn(async () => {});
    const done = deleteAllMemories(host(), del);
    expect(document.querySelector(".inline-confirm-text")!.textContent).toBe("Delete every stored memory? Settings are kept.");
    expect(del).not.toHaveBeenCalled();
    press(".inline-confirm-yes");
    expect(await done).toBe(true);
    expect(del).toHaveBeenCalledTimes(1);
    expect(document.querySelector(".inline-confirm")).toBeNull();
  });

  it("Cancel and Escape delete nothing", async () => {
    const del = vi.fn(async () => {});
    let done = deleteAllMemories(host(), del);
    press(".inline-confirm-no");
    expect(await done).toBe(false);
    done = deleteAllMemories(host(), del);
    document.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape" }));
    expect(await done).toBe(false);
    expect(del).not.toHaveBeenCalled();
    expect(document.querySelector(".inline-confirm")).toBeNull();
  });

  it("asking again cancels the first question and leaves one row", async () => {
    const h = host();
    const first = inlineConfirm(h, "one?", "Yes");
    const second = inlineConfirm(h, "two?", "Yes");
    expect(await first).toBe(false);
    expect(h.querySelectorAll(".inline-confirm").length).toBe(1);
    press(".inline-confirm-yes");
    expect(await second).toBe(true);
  });

  it("the message is text, not markup", async () => {
    const h = host();
    const p = inlineConfirm(h, `<img src=x>`, "OK");
    expect(h.querySelector("img")).toBeNull();
    press(".inline-confirm-no");
    await p;
  });
});

describe("the frontend source", () => {
  const dir = pathToFileURL(process.cwd() + "/src/");
  const files = readdirSync(dir).filter((f) => f.endsWith(".ts") && !f.endsWith(".test.ts"));
  it("calls no window.confirm, alert or prompt anywhere", () => {
    for (const f of files) {
      const text = readFileSync(new URL(f, dir), "utf8").replace(/\/\/.*$/gm, "").replace(/\/\*[\s\S]*?\*\//g, "");
      expect(text, f).not.toMatch(/(^|[^.\w])(window\.)?(confirm|alert|prompt)\(/m);
    }
  });
  const main = readFileSync(new URL("./main.ts", dir), "utf8");
  it("Delete all goes through the in-page question", () => {
    expect(main).toMatch(/deleteAllMemories\(\$\("#mem-delete-all"\)/);
  });
  it("the tile status gets the raw action text, since it is set with textContent", () => {
    expect(main).toMatch(/function tileStatus[\s\S]{0,500}el\.textContent = text/);
    expect(main).not.toMatch(/tileStatus\([^)]*escapeHtml\(/);
  });
  it("a reload redraws the open question from agent_status", () => {
    expect(main).toMatch(/pendingConfirm/);
    expect(main).toMatch(/askConfirmation\(state\.runId, pending\)/);
  });
  it("a reload puts the running task's window back in the feed so the next goal can name it", () => {
    expect(main).toMatch(/Watch it again[\s\S]{0,120}addWindow\(win\)/);
    expect(main).toMatch(/await loadWindows\(\)[\s\S]{0,400}activeAgent = \{ key: key\(win\)/);
  });
  it("the memory chip is refreshed when a task ends and after a restore, not only by the 20 s poll", () => {
    expect(main).toMatch(/function finishAgent[\s\S]{0,300}void refreshMemory\(\)/);
    expect(main).toMatch(/async function restoreRunningTask[\s\S]{0,2500}finally \{[\s\S]{0,200}void refreshMemory\(\)/);
  });
});
