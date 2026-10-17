import {readSettings, settingsPatch, type MemorySettings} from "./memory-ui";

/** Keep the saved baseline paired with the values actually rendered in the form. */
export function memorySettingsForm(root: Document) {
  const input = (id: string) => root.getElementById(id) as HTMLInputElement;
  let rendered: MemorySettings | null = null;
  return {
    refresh(settings: MemorySettings, editing: boolean) {
      if (editing && rendered) return;
      rendered = {...settings};
      input("mem-chat").checked = settings.chatMemories;
      input("mem-sky").checked = settings.skysight;
      input("mem-retention").value = String(settings.retentionDays);
    },
    patch(): Partial<MemorySettings> | string | null {
      if (!rendered) return null;
      const next = readSettings(rendered, {
        chat: input("mem-chat").checked,
        skysight: input("mem-sky").checked,
        retention: input("mem-retention").value,
      });
      return typeof next === "string" ? next : settingsPatch(rendered, next);
    },
  };
}
