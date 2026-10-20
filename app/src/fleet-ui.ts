// The "Routines from your organization" card and header chip, invite-first (UI-SPEC.md).
// Three phases: Connect (paste or open an invite link), Pairing (manual/Advanced device-code
// path only), and Connected (waiting, or one/more offers). Offers are shown passively: nothing
// runs from here. Review and Send opens a window picker and stages the routine into the existing
// workflow draft form (icu-workflow-staged on #trajectory-tools); Save, Choose and Send stay the
// only path to running it. The actor token and endpoint live in Rust and never reach this layer.
type OfferView = {
  dispatchId: string; title: string; description: string;
  issuerLabel: string; issuerNote: string | null;
  stepsPreview: { place: string; action: string | null }[];
  appHint: string; deadlineUnixS: number; state: string;
};
type FleetStatus = {
  optedIn: boolean; linked: boolean; deviceLabel: string | null;
  endpointConfigured: boolean; lastChecked: number | null; linkCode: string | null;
  tenantId?: string | null; offers: OfferView[];
};
type Win = { pid: number; window_id: number; app_name: string; title?: string };
type Invoke = <T>(command: string, args?: Record<string, unknown>) => Promise<T>;
type FleetEvent = { kind?: string; data?: { message?: string } };

export function wireFleet(doc: Document, invoke: Invoke) {
  const get = <T extends HTMLElement>(id: string) => doc.getElementById(id) as T;
  const chip = get<HTMLButtonElement>("fleet-chip"), chipText = get("fleet-chip-text");
  const bodyEl = get("fleet-body"), statusLine = get("fleet-status");
  let busy = false;

  const setStatus = (text: string) => { statusLine.textContent = text; };
  const el = (tag: string, className: string, text?: string) => {
    const node = doc.createElement(tag); node.className = className;
    if (text !== undefined) node.textContent = text; return node;
  };

  // phase 0 Connect (!optedIn || !endpointConfigured), phase 1 Pairing (!linked, manual path),
  // phase 2 Connected. The invite path jumps 0 -> 2 directly (redeem links; no device code).
  const phase = (s: FleetStatus) => (!s.optedIn || !s.endpointConfigured) ? 0 : (!s.linked ? 1 : 2);

  const dueIn = (unixS: number) => {
    const secs = unixS - Math.floor(Date.now() / 1000);
    if (secs <= 0) return "Expired";
    const mins = Math.round(secs / 60);
    if (mins < 60) return `Due in ${mins} min`;
    const hrs = Math.round(mins / 60);
    if (hrs < 48) return `Due in ${hrs} h`;
    return `Due in ${Math.round(hrs / 24)} d`;
  };
  const lastCheckedText = (at: number | null) =>
    at ? `Last checked ${new Date(at * 1000).toLocaleTimeString()}.` : "Connected.";

  // Subtle Connect -> Approve -> Receive wayfinding (not a progress bar the user drives).
  const stepper = (active: number) => {
    const steps = el("div", "org-steps");
    ["Connect", "Approve", "Receive"].forEach((name, i) => {
      const step = el("span", `org-step${i === active ? " active" : ""}${i < active ? " done" : ""}`, name);
      steps.append(step);
    });
    return steps;
  };

  const render = async (s: FleetStatus) => {
    const count = s.offers.length;
    chip.hidden = count === 0;
    chipText.textContent = count === 1 ? "1 routine offered" : `${count} routines offered`;
    bodyEl.replaceChildren();
    const p = phase(s);
    if (p === 0) renderConnect();
    else if (p === 1) renderPairing(s);
    else await renderConnected(s);
  };

  // --- phase 0: Connect ---
  const renderConnect = () => {
    bodyEl.append(stepper(0));
    bodyEl.append(el("h3", "org-heading", "Connect to your organization"));
    bodyEl.append(el("p", "muted", "Your administrator sends you an invite link. Opening it connects this Mac so they can send you routines to review."));

    const row = el("div", "invite-row");
    const input = doc.createElement("input");
    input.id = "fleet-invite"; input.type = "text"; input.spellcheck = false;
    input.autocapitalize = "off"; input.setAttribute("autocorrect", "off");
    input.placeholder = "Paste your invite link or code"; input.setAttribute("aria-label", "Invite link or code");
    const connect = el("button", "btn-primary", "Connect") as HTMLButtonElement;
    connect.type = "button";
    const submit = () => void enroll(input.value);
    connect.onclick = submit;
    input.addEventListener("keydown", e => { if (e.key === "Enter") { e.preventDefault(); submit(); } });
    row.append(input, connect);
    bodyEl.append(row);

    bodyEl.append(el("p", "recovery muted", "Don't have an invite? Ask your administrator to send one."));

    const adv = doc.createElement("details"); adv.className = "org-advanced";
    adv.append(el("summary", "", "Advanced: connect to a server manually."));
    const label = el("label", "");
    label.append(doc.createTextNode("Organization server "));
    const manual = doc.createElement("input");
    manual.id = "fleet-endpoint"; manual.type = "text"; manual.spellcheck = false;
    manual.autocapitalize = "off"; manual.setAttribute("autocorrect", "off"); manual.placeholder = "https://...";
    label.append(manual);
    const validate = el("p", "validate muted", "Enter your organization's https server address.");
    manual.addEventListener("input", () => {
      const ok = /^https:\/\/\S+/i.test(manual.value.trim());
      validate.textContent = manual.value.trim() === "" ? "Enter your organization's https server address."
        : ok ? "Looks good. This uses the classic approval code path." : "The address must start with https://";
      validate.classList.toggle("ok", ok);
    });
    const manualBtn = el("button", "ghost", "Connect to this server") as HTMLButtonElement;
    manualBtn.type = "button";
    manualBtn.onclick = () => void manualConnect(manual.value);
    adv.append(label, validate, manualBtn);
    bodyEl.append(adv);
    setStatus("Open your invite link, or paste it above to connect.");
  };

  // --- phase 1: Pairing (manual/Advanced path only; invite never shows a code) ---
  const renderPairing = (s: FleetStatus) => {
    bodyEl.append(stepper(1));
    if (s.linkCode) {
      const card = el("div", "pair-code-card");
      const boxes = el("div", "code-boxes");
      s.linkCode.split("-").forEach((group, gi) => {
        if (gi > 0) boxes.append(el("span", "code-dash", "–"));
        const g = el("div", "code-group");
        for (const ch of group) g.append(el("span", "code-box", ch));
        boxes.append(g);
      });
      card.append(boxes);
      const actions = el("div", "code-actions");
      const copy = el("button", "ghost", "Copy code") as HTMLButtonElement;
      copy.type = "button";
      copy.onclick = () => { void navigator.clipboard?.writeText(s.linkCode ?? ""); copy.textContent = "Copied"; };
      actions.append(copy);
      card.append(actions);
      const guard = el("p", "code-guard");
      guard.append(el("span", "dot warn"), doc.createTextNode(" Only share this with your administrator."));
      card.append(guard);
      bodyEl.append(card);
    }
    const waiting = el("p", "org-waiting");
    waiting.append(el("span", "dot ok livedot"), doc.createTextNode(" Waiting for your administrator to approve this Mac..."));
    bodyEl.append(waiting);
    setStatus("Give your administrator the code above to approve this computer.");
  };

  // --- phase 2: Connected (waiting, or offers) ---
  const renderConnected = async (s: FleetStatus) => {
    bodyEl.append(stepper(2));
    if (s.offers.length === 0) {
      const card = el("div", "org-guarantee");
      card.append(el("h3", "org-heading", "Connected to your organization"));
      card.append(el("p", "muted", "Your administrator can send you routines here. Nothing runs on this Mac until you review it and choose Send. You can disconnect any time."));
      const disconnect = el("button", "ghost", "Disconnect") as HTMLButtonElement;
      disconnect.type = "button";
      disconnect.onclick = () => void setOptin(false);
      card.append(disconnect);
      bodyEl.append(card);
      setStatus(lastCheckedText(s.lastChecked));
      return;
    }
    // Window list is fetched once and shared across cards for the inline picker.
    let windows: Win[] = [];
    try { windows = (await invoke<{ windows: Win[] }>("cua_list_windows")).windows ?? []; }
    catch { windows = []; }
    for (const offer of s.offers) bodyEl.append(offerCard(offer, windows));
    setStatus(`${s.offers.length === 1 ? "1 routine" : `${s.offers.length} routines`} offered. ${lastCheckedText(s.lastChecked)}`);
  };

  const offerCard = (offer: OfferView, windows: Win[]) => {
    const card = el("div", "offer-card"); card.dataset.dispatchId = offer.dispatchId;
    card.append(el("h3", "offer-title", offer.title));
    const issuer = el("p", "offer-issuer muted", `From ${offer.issuerLabel}`);
    // issuerNote is display-only (spec 3.1); it is never copied into the plan.
    if (offer.issuerNote) issuer.append(doc.createTextNode(` ${offer.issuerNote}`));
    card.append(issuer);

    const steps = el("ol", "offer-steps");
    for (const step of offer.stepsPreview) {
      const prow = el("li", "pick-row");
      prow.append(el("span", "pick-glyph", (step.place[0] ?? "?").toUpperCase()));
      const txt = el("span", "pick-text");
      txt.append(el("span", "pick-name", step.place));
      txt.append(el("span", "pick-sub", step.action ?? "work in this view"));
      prow.append(txt);
      steps.append(prow);
    }
    card.append(steps);

    // Window picker (the app-name match is listed first).
    const hint = offer.appHint.toLowerCase();
    const ordered = [...windows].filter(w => w.pid > 0)
      .sort((a, b) => Number(b.app_name.toLowerCase() === hint) - Number(a.app_name.toLowerCase() === hint));
    const winPick = el("div", "win-pick");
    const select = doc.createElement("select"); select.className = "fleet-window";
    select.setAttribute("aria-label", `Window for ${offer.title}`);
    if (!ordered.length) {
      const opt = doc.createElement("option");
      opt.textContent = `Open ${offer.appHint} and refresh`; opt.disabled = true; select.append(opt);
    }
    for (const w of ordered) {
      const opt = doc.createElement("option"); opt.value = `${w.pid}`; opt.dataset.appName = w.app_name;
      opt.textContent = `${w.app_name}${w.title ? ` — ${w.title}` : ""}`;
      select.append(opt);
    }
    winPick.append(el("span", "pick-sub muted", `Run in the ${offer.appHint} window`), select);
    card.append(winPick);

    if (offer.deadlineUnixS) card.append(el("p", "offer-deadline muted", dueIn(offer.deadlineUnixS)));

    const actions = el("div", "offer-actions");
    const review = el("button", "btn-primary", "Review and Send") as HTMLButtonElement;
    review.type = "button"; review.disabled = !ordered.length;
    review.onclick = () => void stageOffer(offer, select, card);
    const decline = el("button", "ghost", "Decline") as HTMLButtonElement;
    decline.type = "button";
    decline.onclick = () => void declineOffer(offer.dispatchId);
    actions.append(review, decline);
    card.append(actions);
    return card;
  };

  // --- actions ---
  const enroll = async (raw: string) => {
    const input = raw.trim();
    if (!input) { setStatus("Paste the invite link or code your administrator sent you."); return; }
    if (busy) return; busy = true;
    setStatus("Connecting to your organization...");
    try { await render(await invoke<FleetStatus>("fleet_enroll", { input })); }
    catch (error) { setStatus(String(error)); await refresh(); }
    finally { busy = false; }
  };

  const manualConnect = async (raw: string) => {
    const endpoint = raw.trim();
    if (!/^https:\/\/\S+/i.test(endpoint)) { setStatus("Enter an https address for your organization server."); return; }
    if (busy) return; busy = true;
    try { await render(await invoke<FleetStatus>("fleet_set_exec_optin", { enabled: true, endpoint })); }
    catch (error) { setStatus(String(error)); await refresh(); }
    finally { busy = false; }
  };

  const setOptin = async (enabled: boolean) => {
    if (busy) return; busy = true;
    try { await render(await invoke<FleetStatus>("fleet_set_exec_optin", { enabled })); }
    catch (error) { setStatus(String(error)); await refresh(); }
    finally { busy = false; }
  };

  const stageOffer = async (offer: OfferView, select: HTMLSelectElement, card: HTMLElement) => {
    const option = select.selectedOptions[0];
    const pid = Number(option?.value);
    const appName = option?.dataset.appName ?? offer.appHint;
    if (!pid) { setStatus("Pick a window to review this routine in."); return; }
    try {
      const generated = await invoke("fleet_stage", { dispatchId: offer.dispatchId, pid, appName });
      doc.getElementById("trajectory-tools")?.dispatchEvent(
        new CustomEvent("icu-workflow-staged", { detail: { generated, issuerLabel: offer.issuerLabel } }));
      const note = el("p", "staged-note", "Review the routine below, then Save. Nothing runs until you press Send.");
      card.querySelector(".staged-note")?.remove();
      card.append(note);
      setStatus("Review the routine below, then Save. Nothing runs until you press Send.");
    } catch (error) { setStatus(String(error)); await refresh(); }
  };

  const declineOffer = async (dispatchId: string) => {
    try { await render(await invoke<FleetStatus>("fleet_decline", { dispatchId })); }
    catch (error) { setStatus(String(error)); await refresh(); }
  };

  const refresh = async () => {
    try { await render(await invoke<FleetStatus>("fleet_status")); }
    catch (error) { setStatus(String(error)); }
  };

  // The "fleet" event carries the deep-link enrollment outcome: a redeem that failed on a cold
  // start has no caller to show it. Refresh first (re-renders the current phase and its default
  // status), THEN set the error so it is not immediately clobbered by that re-render.
  const onEvent = async (payload?: FleetEvent) => {
    await refresh();
    if (payload?.kind === "enroll_error" && payload.data?.message) setStatus(payload.data.message);
  };

  void refresh();
  return { refresh, onEvent };
}
