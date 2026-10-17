// @vitest-environment jsdom
import { beforeEach, expect, it } from "vitest";
import { readFileSync } from "node:fs";
import { wireRunPreferences } from "./run-preferences";
const markup = readFileSync(`${process.cwd()}/index.html`, "utf8");
const field = <T extends HTMLInputElement | HTMLSelectElement>(id: string) => document.getElementById(id) as T;
function reload() { document.documentElement.innerHTML = markup; wireRunPreferences(document, () => sessionStorage); }
beforeEach(() => { sessionStorage.clear(); reload(); });

it("keeps the hosted provider and privacy choices through a real DOM remount", () => {
  field<HTMLSelectElement>("brain-select").value = "openai";
  field<HTMLSelectElement>("task-budget").value = "72";
  field<HTMLInputElement>("run-use-memory").checked = false;
  field<HTMLInputElement>("run-remember").checked = false;
  field<HTMLInputElement>("run-use-screenshots").checked = true;
  field<HTMLInputElement>("run-allow-foreground").checked = true;
  field("brain-select").dispatchEvent(new Event("change"));
  reload();
  expect(field("brain-select").value).toBe("openai");
  expect(field("task-budget").value).toBe("72");
  expect(field<HTMLInputElement>("run-use-memory").checked).toBe(false);
  expect(field<HTMLInputElement>("run-remember").checked).toBe(false);
  expect(field<HTMLInputElement>("run-use-screenshots").checked).toBe(true);
  expect(field<HTMLInputElement>("run-allow-foreground").checked).toBe(false);
  expect(sessionStorage.getItem("icu.run-preferences.v1")).not.toContain("foreground");
});

it("ignores malformed preferences and survives disabled browser storage", () => {
  sessionStorage.setItem("icu.run-preferences.v1", JSON.stringify({version: 1, brain: "unknown", budget: "9000", "run-remember": "false", "run-allow-foreground": true}));
  reload();
  expect(field("brain-select").value).toBe("auto");
  expect(field("task-budget").value).toBe("36");
  expect(field<HTMLInputElement>("run-remember").checked).toBe(true);
  expect(field<HTMLInputElement>("run-allow-foreground").checked).toBe(false);
  expect(() => wireRunPreferences(document, () => { throw new Error("storage denied"); })).not.toThrow();
  expect(() => field("brain-select").dispatchEvent(new Event("change"))).not.toThrow();
});
