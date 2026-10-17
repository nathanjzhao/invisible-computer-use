// A real editable surface lets the webview's input method finish composition
// before any text is sent to the remote window. Its lifetime belongs to one
// explicit control gesture, so a late commit cannot follow a changed target.
export class ManualTextInput {
  readonly element = document.createElement("textarea");
  private alive = true;
  private composing = false;
  private endTimer: number | undefined;

  constructor(parent: HTMLElement, label: string, private owns: () => boolean,
              private commit: (text: string) => void) {
    const input = this.element;
    input.className = "manual-text-input";
    input.setAttribute("aria-label", label);
    input.tabIndex = -1;
    input.autocomplete = "off";
    input.setAttribute("autocapitalize", "off");
    input.spellcheck = false;
    input.addEventListener("compositionstart", () => { this.composing = true; });
    input.addEventListener("compositionend", () => {
      this.composing = false;
      // Engines differ on whether the final input precedes compositionend.
      // The input listener may flush first; clearing the buffer makes this
      // deferred fallback harmless when the commit was already delivered.
      this.endTimer = window.setTimeout(() => this.flush(), 0);
    });
    input.addEventListener("input", (event) => {
      if (!(event as InputEvent).isComposing) this.flush();
    });
    input.addEventListener("paste", (event) => {
      if (!this.focused || !this.owns() || this.composing || !event.clipboardData) return;
      event.preventDefault();
      this.flush();
      const text = event.clipboardData.getData("text/plain");
      if (text) this.commit(text);
    });
    input.addEventListener("blur", () => this.reset());
    parent.appendChild(input);
    input.focus({ preventScroll: true });
  }

  get focused(): boolean { return this.alive && document.activeElement === this.element; }
  get isComposing(): boolean { return this.composing; }

  private flush(): void {
    if (this.composing) return;
    const text = this.element.value;
    this.element.value = "";
    if (text && this.focused && this.owns()) this.commit(text);
  }

  private reset(): void {
    if (this.endTimer !== undefined) window.clearTimeout(this.endTimer);
    this.endTimer = undefined;
    this.composing = false;
    this.element.value = "";
  }

  dispose(): void {
    this.alive = false;
    this.reset();
    this.element.remove();
  }
}
