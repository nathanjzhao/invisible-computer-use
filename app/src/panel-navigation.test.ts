// @vitest-environment jsdom
import { beforeEach, expect, it } from "vitest";
import { readFileSync } from "node:fs";
import { showUtilityPanel, toggleUtilityPanel, wireUtilityPanelKeyboard } from "./panel-navigation";

beforeEach(() => {
  document.documentElement.innerHTML = readFileSync(`${process.cwd()}/index.html`, "utf8");
});
const el = (id: string) => document.getElementById(id)!;

it("returns from a focused review to History without hidden focus or overlapping panels", () => {
  wireUtilityPanelKeyboard(document);
  for (let cycle = 0; cycle < 10; cycle++) {
    showUtilityPanel(document, "memory-pop");
    el("trajectory-path").focus();
    expect(toggleUtilityPanel(document, "history-panel")).toBe(true);
    expect(document.activeElement).toBe(el("history-panel"));
    expect(el("memory-pop").inert).toBe(true);
    expect(el("memory-pop").hidden).toBe(true);
    expect(el("memory-chip").getAttribute("aria-expanded")).toBe("false");
    expect(el("history-button").getAttribute("aria-expanded")).toBe("true");
    expect(el("history-panel").getAttribute("aria-hidden")).toBe("false");
    expect(el("history-button").getAttribute("aria-controls")).toBe("history-panel");
    el("history-panel").dispatchEvent(new KeyboardEvent("keydown", { key: "Escape", bubbles: true }));
    expect(el("history-panel").hidden).toBe(true);
    expect(document.activeElement).toBe(el("history-button"));
  }
});

it("opening Audio closes Memory and History without changing consent or recording settings", () => {
  const consent = el("history-consent") as HTMLInputElement;
  consent.checked = true;
  showUtilityPanel(document, "history-panel");
  showUtilityPanel(document, "audio-panel");
  expect(el("history-panel").hidden).toBe(true);
  expect(el("memory-pop").hidden).toBe(true);
  expect(el("audio-panel").hidden).toBe(false);
  expect(document.activeElement).toBe(el("audio-panel"));
  expect(consent.checked).toBe(true);
});
