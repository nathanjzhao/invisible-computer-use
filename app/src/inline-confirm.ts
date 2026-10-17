// A confirmation drawn in the page. window.confirm, alert and prompt do nothing
// in the macOS webview (wry implements none of the JavaScript dialog panels, so
// WebKit answers false and shows nothing), so every question the app asks the
// user is asked here, with buttons.

let open: { row: HTMLElement; settle: (answer: boolean) => void } | null = null;

/**
 * Ask `message` in a row appended to `host`, with a confirm button labelled
 * `confirmLabel` and a Cancel button. Resolves true on confirm, false on Cancel
 * or Escape. Asking again while a row is open cancels the first one.
 */
export function inlineConfirm(host: HTMLElement, message: string, confirmLabel: string): Promise<boolean> {
  open?.settle(false);
  const doc = host.ownerDocument;
  const row = doc.createElement("div");
  row.className = "inline-confirm";
  row.setAttribute("role", "alertdialog");
  const text = doc.createElement("span");
  text.className = "inline-confirm-text";
  text.textContent = message;
  const yes = doc.createElement("button");
  yes.type = "button";
  yes.className = "ghost accent inline-confirm-yes";
  yes.textContent = confirmLabel;
  const no = doc.createElement("button");
  no.type = "button";
  no.className = "ghost inline-confirm-no";
  no.textContent = "Cancel";
  row.append(text, yes, no);
  host.append(row);
  no.focus();
  return new Promise<boolean>((resolve) => {
    const onKey = (e: KeyboardEvent) => { if (e.key === "Escape") settle(false); };
    const settle = (answer: boolean) => {
      if (open?.row !== row) return;
      open = null;
      doc.removeEventListener("keydown", onKey);
      row.remove();
      resolve(answer);
    };
    open = { row, settle };
    doc.addEventListener("keydown", onKey);
    yes.addEventListener("click", () => settle(true));
    no.addEventListener("click", () => settle(false));
  });
}

/** The "Delete all memories" button: ask first, delete only on a yes. True when it deleted. */
export async function deleteAllMemories(host: HTMLElement, del: () => Promise<unknown>): Promise<boolean> {
  if (!(await inlineConfirm(host, "Delete every stored memory? Settings are kept.", "Delete all"))) return false;
  await del();
  return true;
}
