// In-app model onboarding (ONBOARD-APIKEY): a first-run step that lets someone
// who downloaded the .app give it a model (an API key) without a terminal. The
// backend stores the key in the app's own data dir and uses it as a fallback to
// ANTHROPIC_API_KEY / OPENAI_API_KEY.

type Invoke = <T>(command: string, args?: Record<string, unknown>) => Promise<T>;

export type OnboardingStatus = {
  needsOnboarding: boolean;
  anthropic: boolean;
  openai: boolean;
  claudeCli: boolean;
  storedAnthropic: boolean;
  storedOpenai: boolean;
};

const KEY_LABEL: Record<string, string> = { anthropic: "Anthropic API key", openai: "OpenAI API key" };
const KEY_PLACEHOLDER: Record<string, string> = { anthropic: "sk-ant-…", openai: "sk-…" };

/** Wire the onboarding overlay. `refresh()` shows it when no model is configured;
 *  `open()` shows it on demand (the "Model key" button). */
export function wireOnboarding(root: Document, invoke: Invoke): { refresh: () => Promise<void>; open: () => void } {
  const overlay = root.getElementById("onboarding-overlay");
  const provider = root.getElementById("onboard-provider") as HTMLSelectElement | null;
  const input = root.getElementById("onboard-key") as HTMLInputElement | null;
  const keyLabel = root.getElementById("onboard-key-label");
  const statusEl = root.getElementById("onboard-status");
  const save = root.getElementById("onboard-save") as HTMLButtonElement | null;
  const later = root.getElementById("onboard-later");
  const openButton = root.getElementById("model-key-button");
  if (!overlay || !provider || !input || !save) return { refresh: async () => {}, open: () => {} };

  const setStatus = (text: string | null, kind: "" | "err" | "ok" = "") => {
    if (!statusEl) return;
    statusEl.hidden = !text;
    statusEl.textContent = text ?? "";
    statusEl.className = `onboard-status ${kind}`.trimEnd();
  };

  const show = (on: boolean) => { overlay.hidden = !on; if (on) input.focus(); };
  const open = () => { setStatus(null); show(true); };
  const close = () => show(false);

  const syncProvider = () => {
    if (keyLabel) keyLabel.textContent = KEY_LABEL[provider.value] ?? "API key";
    input.placeholder = KEY_PLACEHOLDER[provider.value] ?? "";
  };
  provider.addEventListener("change", syncProvider);
  syncProvider();

  const refresh = async () => {
    try {
      const status = await invoke<OnboardingStatus>("model_onboarding_status");
      if (status.needsOnboarding) open();
    } catch { /* status unavailable: never block the app on it */ }
  };

  save.addEventListener("click", async () => {
    const key = input.value.trim();
    if (!key) { setStatus("Enter your API key.", "err"); input.focus(); return; }
    save.disabled = true;
    setStatus("Saving…");
    try {
      const status = await invoke<OnboardingStatus>("set_model_key", { provider: provider.value, key });
      input.value = "";
      setStatus("Saved. The app will use this model.", "ok");
      if (!status.needsOnboarding) setTimeout(close, 700);
    } catch (e) {
      setStatus(`Could not save the key: ${e}`, "err");
    } finally {
      save.disabled = false;
    }
  });

  later?.addEventListener("click", close);
  openButton?.addEventListener("click", open);
  // Click the dimmed backdrop (not the card) to dismiss.
  overlay.addEventListener("click", (e) => { if (e.target === overlay) close(); });
  input.addEventListener("keydown", (e) => { if ((e as KeyboardEvent).key === "Enter") { e.preventDefault(); save.click(); } });

  return { refresh, open };
}
