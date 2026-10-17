// @vitest-environment jsdom
import { beforeEach, describe, expect, it, vi } from "vitest";
import { wireOnboarding } from "./onboarding";

function layout() {
  document.body.innerHTML = `
    <button id="model-key-button"></button>
    <div id="onboarding-overlay" hidden>
      <select id="onboard-provider">
        <option value="anthropic">Anthropic</option>
        <option value="openai">OpenAI</option>
      </select>
      <span id="onboard-key-label"></span>
      <input id="onboard-key" type="password" />
      <p id="onboard-status" hidden></p>
      <button id="onboard-later"></button>
      <button id="onboard-save"></button>
    </div>`;
}

const overlay = () => document.getElementById("onboarding-overlay") as HTMLElement;
const input = () => document.getElementById("onboard-key") as HTMLInputElement;
const save = () => document.getElementById("onboard-save") as HTMLButtonElement;

describe("wireOnboarding", () => {
  beforeEach(layout);

  it("shows the overlay on refresh when no model is configured", async () => {
    const invoke = vi.fn(async () => ({ needsOnboarding: true })) as never;
    await wireOnboarding(document, invoke).refresh();
    expect(overlay().hidden).toBe(false);
  });

  it("stays hidden on refresh when a model is already configured", async () => {
    const invoke = vi.fn(async () => ({ needsOnboarding: false })) as never;
    await wireOnboarding(document, invoke).refresh();
    expect(overlay().hidden).toBe(true);
  });

  it("saves the trimmed key to the chosen provider and closes once configured", async () => {
    const calls: Array<[string, unknown]> = [];
    const invoke = vi.fn(async (cmd: string, args?: unknown) => {
      calls.push([cmd, args]);
      return { needsOnboarding: false };
    }) as never;
    wireOnboarding(document, invoke).open();
    expect(overlay().hidden).toBe(false);
    input().value = "  sk-ant-xyz  ";
    save().click();
    await vi.waitFor(() =>
      expect(calls).toContainEqual(["set_model_key", { provider: "anthropic", key: "sk-ant-xyz" }]),
    );
    await vi.waitFor(() => expect(overlay().hidden).toBe(true));
  });

  it("refuses to save an empty key and never calls the backend", async () => {
    const invoke = vi.fn(async () => ({ needsOnboarding: true })) as never;
    wireOnboarding(document, invoke).open();
    input().value = "   ";
    save().click();
    await Promise.resolve();
    expect(invoke).not.toHaveBeenCalled();
    expect(document.getElementById("onboard-status")!.textContent).toMatch(/enter your api key/i);
  });

  it("updates the key label when the provider changes", () => {
    wireOnboarding(document, (async () => ({ needsOnboarding: true })) as never);
    const provider = document.getElementById("onboard-provider") as HTMLSelectElement;
    expect(document.getElementById("onboard-key-label")!.textContent).toMatch(/anthropic/i);
    provider.value = "openai";
    provider.dispatchEvent(new Event("change"));
    expect(document.getElementById("onboard-key-label")!.textContent).toMatch(/openai/i);
  });
});
