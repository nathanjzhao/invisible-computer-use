const panels = [
  ["memory-pop", "memory-chip", "Memory"],
  ["audio-panel", "audio-button", "Record audio"],
  ["history-panel", "history-button", "Computer History"],
] as const;

type PanelId = typeof panels[number][0];

/** One visible utility panel, with focus moved out of the panel being hidden. */
export function showUtilityPanel(root: Document, selected: PanelId | null): void {
  const previous = panels.find(([id]) => {
    const panel = root.getElementById(id);
    return panel && !panel.hidden;
  });
  for (const [id, buttonId, label] of panels) {
    const panel = root.getElementById(id);
    const button = root.getElementById(buttonId);
    if (!panel) continue;
    const visible = id === selected;
    // Reattach an opening panel after making it visible. WebKit can retain an
    // earlier hidden panel outside its parent's accessibility child list.
    const parent = visible && panel.hidden ? panel.parentNode : null;
    if (parent) panel.remove();
    panel.hidden = !visible;
    panel.inert = !visible;
    panel.setAttribute("aria-hidden", String(!visible));
    panel.setAttribute("role", "region");
    panel.setAttribute("aria-label", label);
    panel.tabIndex = -1;
    button?.setAttribute("aria-controls", id);
    button?.setAttribute("aria-expanded", String(visible));
    if (parent) parent.appendChild(panel);
  }
  const focus = selected ? root.getElementById(selected)
    : previous ? root.getElementById(previous[1]) : null;
  focus?.focus({ preventScroll: true });
}

export function toggleUtilityPanel(root: Document, id: PanelId): boolean {
  const opening = Boolean(root.getElementById(id)?.hidden);
  showUtilityPanel(root, opening ? id : null);
  return opening;
}

export function wireUtilityPanelKeyboard(root: Document): void {
  for (const [id] of panels) {
    root.getElementById(id)?.addEventListener("keydown", event => {
      if (event.key !== "Escape" || event.defaultPrevented) return;
      event.preventDefault();
      event.stopPropagation();
      showUtilityPanel(root, null);
    });
  }
}
