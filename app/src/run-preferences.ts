// Keep user choices through a webview reload without storing prompt text or
// extending the per-task permission to bring another window to the foreground.
const KEY = "icu.run-preferences.v1";
const checkboxes = ["run-use-memory", "run-remember", "run-use-screenshots"] as const;
const brains = new Set(["auto", "claude-code", "openai", "local"]);
const budgets = new Set(["18", "36", "72"]);

export function wireRunPreferences(root: Document, storage: () => Storage | undefined): void {
  const brain = root.getElementById("brain-select") as HTMLSelectElement | null;
  const budget = root.getElementById("task-budget") as HTMLSelectElement | null;
  const fields = checkboxes.map(id => root.getElementById(id) as HTMLInputElement | null);
  try {
    const raw = storage()?.getItem(KEY);
    const saved: unknown = raw ? JSON.parse(raw) : null;
    if (saved && typeof saved === "object" && !Array.isArray(saved)) {
      const data = saved as Record<string, unknown>;
      if (data.version === 1) {
        if (brain && typeof data.brain === "string" && brains.has(data.brain)) brain.value = data.brain;
        if (budget && typeof data.budget === "string" && budgets.has(data.budget)) budget.value = data.budget;
        for (let index = 0; index < fields.length; index++) {
          const field = fields[index], value = data[checkboxes[index]];
          if (field && typeof value === "boolean") field.checked = value;
        }
      }
    }
  } catch { /* Unavailable browser storage must not prevent task recovery. */ }
  const save = () => {
    const value: Record<string, unknown> = { version: 1, brain: brain?.value, budget: budget?.value };
    fields.forEach((field, index) => { if (field) value[checkboxes[index]] = field.checked; });
    try { storage()?.setItem(KEY, JSON.stringify(value)); } catch { /* Keep the current visible choices. */ }
  };
  for (const field of [brain, budget, ...fields]) field?.addEventListener("change", save);
}
