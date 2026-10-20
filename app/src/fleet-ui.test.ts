// @vitest-environment jsdom
import { readFileSync } from "node:fs";
import { beforeEach, expect, it, vi } from "vitest";
import { wireFleet } from "./fleet-ui";
import { wireWorkflows } from "./workflow-ui";

const offer = {
  dispatchId: "dp-9b1d3c5e", title: "Export the weekly report",
  description: "Seen 12 times in 9 sessions over the last 30 days, usually in about 1m 24s.",
  issuerLabel: "ops@acme.example", issuerNote: "Run before standup.",
  stepsPreview: [{ place: "Zendesk", action: null }, { place: "Zendesk", action: "Export" }],
  appHint: "Google Chrome", deadlineUnixS: Math.floor(Date.now() / 1000) + 1000, state: "delivered",
};
// phase 2 with an offer (invite path: linked, no pairing code).
const offersStatus = { optedIn: true, linked: true, deviceLabel: "Test Mac", endpointConfigured: true, lastChecked: 1792343045, linkCode: null, tenantId: "org_acme", offers: [offer] };
// phase 2 waiting (linked, no offers).
const waitingStatus = { ...offersStatus, offers: [] };
// phase 0 Connect (nothing set up yet).
const offStatus = { optedIn: false, linked: false, deviceLabel: null, endpointConfigured: false, lastChecked: null, linkCode: null, tenantId: null, offers: [] };
// phase 1 Pairing (manual/Advanced device-code path only).
const pairingStatus = { optedIn: true, linked: false, deviceLabel: null, endpointConfigured: true, lastChecked: null, linkCode: "WXYZ-7Q2K", tenantId: null, offers: [] };
const windows = { windows: [{ pid: 10, window_id: 1, app_name: "Finder", title: "Files" }, { pid: 20, window_id: 2, app_name: "Google Chrome", title: "Zendesk" }] };
const generated = { request_id: "wf-staged", proposal_sha256: "a".repeat(64), plan: {
  title: "Export the weekly report", description: "Seen 12 times.", parameters: [],
  steps: [{ description: "In Zendesk: work in this view", evidence_lines: [1] }, { description: 'In Zendesk: click "Export"', evidence_lines: [2] }],
  verification: "A new tab appears.",
} };

const get = <T extends HTMLElement = HTMLElement>(id: string) => document.getElementById(id) as T;
const q = (sel: string) => get("fleet-body").querySelector(sel) as HTMLElement | null;
const button = (text: string) => [...get("fleet-body").querySelectorAll("button")].find(b => b.textContent === text) as HTMLButtonElement | undefined;
const tick = () => new Promise(r => setTimeout(r, 0));
const settle = async () => { await tick(); await tick(); };
beforeEach(() => { document.documentElement.innerHTML = readFileSync(`${process.cwd()}/index.html`, "utf8"); });

type Handler = (command: string, args?: Record<string, unknown>) => Promise<unknown>;
function setup(handler?: Handler) {
  const invoke = vi.fn(handler ?? (async command =>
    command === "fleet_status" ? offersStatus
      : command === "cua_list_windows" ? windows
      : command === "fleet_stage" ? generated
      : command === "fleet_enroll" ? offersStatus
      : command === "fleet_set_exec_optin" ? waitingStatus
      : command === "fleet_decline" ? waitingStatus
      : command === "workflow_list" ? []
      : offersStatus));
  const api = wireFleet(document, invoke as never);
  return { invoke, api };
}

it("phase 0: shows the Connect screen with invite input, recovery link and an Advanced manual path", async () => {
  setup(async () => offStatus);
  await settle();
  expect(get("fleet-chip").hidden).toBe(true);
  expect(q(".org-heading")?.textContent).toBe("Connect to your organization");
  expect(get("fleet-invite")).toBeTruthy();
  expect(button("Connect")).toBeTruthy();
  expect(q(".recovery")?.textContent).toContain("Ask your administrator to send one");
  // Manual server URL lives ONLY under Advanced, not the default path.
  const advanced = q("details.org-advanced");
  expect(advanced).toBeTruthy();
  expect(advanced!.contains(get("fleet-endpoint"))).toBe(true);
});

it("phase 0: Connect sends the pasted invite through fleet_enroll", async () => {
  const { invoke } = setup(async command => command === "fleet_status" ? offStatus : offersStatus);
  await settle();
  get<HTMLInputElement>("fleet-invite").value = "icu://enroll?invite=inv_abc&h=fleet.acme.example";
  button("Connect")!.click();
  await settle();
  expect(invoke).toHaveBeenCalledWith("fleet_enroll", { input: "icu://enroll?invite=inv_abc&h=fleet.acme.example" });
});

it("phase 0 Advanced: the manual server URL drives the classic opt-in path", async () => {
  const { invoke } = setup(async command => command === "fleet_status" ? offStatus : pairingStatus);
  await settle();
  get<HTMLInputElement>("fleet-endpoint").value = "https://fleet.acme.example";
  button("Connect to this server")!.click();
  await settle();
  expect(invoke).toHaveBeenCalledWith("fleet_set_exec_optin", { enabled: true, endpoint: "https://fleet.acme.example" });
});

it("phase 1: the manual pairing path shows a segmented code and the share guard, never on invite", async () => {
  setup(async () => pairingStatus);
  await settle();
  expect(get("fleet-chip").hidden).toBe(true);
  expect(q(".pair-code-card")).toBeTruthy();
  // WXYZ-7Q2K -> 8 code boxes, split by a dash.
  expect(get("fleet-body").querySelectorAll(".code-box").length).toBe(8);
  expect(q(".code-dash")).toBeTruthy();
  expect(q(".code-guard")?.textContent).toContain("Only share this with your administrator");
});

it("phase 2 waiting: reassures and offers a disconnect that opts out", async () => {
  const { invoke } = setup(async () => waitingStatus);
  await settle();
  expect(q(".org-guarantee")?.querySelector(".org-heading")?.textContent).toBe("Connected to your organization");
  expect(q(".pair-code-card")).toBeNull(); // invite path never shows a code
  button("Disconnect")!.click();
  await settle();
  expect(invoke).toHaveBeenCalledWith("fleet_set_exec_optin", { enabled: false });
});

it("phase 2: shows the offer passively and never runs anything", async () => {
  const { invoke } = setup();
  await settle();
  expect(get("fleet-chip").hidden).toBe(false);
  expect(get("fleet-chip-text").textContent).toBe("1 routine offered");
  const card = q(".offer-card")!;
  expect(card.querySelector(".offer-title")?.textContent).toBe("Export the weekly report");
  expect(card.querySelector(".offer-issuer")?.textContent).toContain("ops@acme.example");
  // issuerNote is display only.
  expect(card.querySelector(".offer-issuer")?.textContent).toContain("Run before standup.");
  // No run, and nothing staged merely by showing the offer.
  expect(invoke.mock.calls.some(([c]) => c === "agent_run")).toBe(false);
  expect(invoke.mock.calls.some(([c]) => c === "fleet_stage")).toBe(false);
});

it("phase 2: the window picker lists the matching app first and stages on Review and Send", async () => {
  const { invoke } = setup();
  await settle();
  const select = q("select.fleet-window") as HTMLSelectElement;
  expect(select.options[0].dataset.appName).toBe("Google Chrome");
  expect(select.options[0].value).toBe("20");
  button("Review and Send")!.click();
  await settle();
  expect(invoke).toHaveBeenCalledWith("fleet_stage", { dispatchId: "dp-9b1d3c5e", pid: 20, appName: "Google Chrome" });
  expect(invoke.mock.calls.some(([c]) => c === "agent_run")).toBe(false);
});

it("phase 2: Decline calls fleet_decline and never stages", async () => {
  const { invoke } = setup();
  await settle();
  button("Decline")!.click();
  await settle();
  expect(invoke).toHaveBeenCalledWith("fleet_decline", { dispatchId: "dp-9b1d3c5e" });
  expect(invoke.mock.calls.some(([c]) => c === "fleet_stage")).toBe(false);
});

it("staging bridges into the existing review form with the organization consent copy", async () => {
  const { invoke } = setup();
  wireWorkflows(get("trajectory-tools"), invoke as never, () => {});
  await settle();
  button("Review and Send")!.click();
  await settle();
  expect(get("workflow-draft").hidden).toBe(false);
  expect(get<HTMLInputElement>("workflow-draft-title").value).toBe("Export the weekly report");
  expect((get("trajectory-tools") as HTMLDetailsElement).open).toBe(true);
  const consentLabel = get<HTMLInputElement>("workflow-save-consent").parentElement!;
  expect(consentLabel.textContent).toContain("I reviewed this routine from ops@acme.example");
  // Save is gated on consent; nothing has been saved or run.
  expect(get<HTMLButtonElement>("workflow-save").disabled).toBe(true);
  expect(invoke.mock.calls.some(([c]) => c === "workflow_save")).toBe(false);
  expect(invoke.mock.calls.some(([c]) => c === "agent_run")).toBe(false);
});

it("a failed deep-link enrollment surfaces its message, then refreshes", async () => {
  const { invoke, api } = setup(async () => offStatus);
  await settle();
  api.onEvent({ kind: "enroll_error", data: { message: "That invite has expired. Ask your administrator to send a new one." } });
  await settle();
  expect(get("fleet-status").textContent).toContain("That invite has expired");
  expect(invoke.mock.calls.some(([c]) => c === "fleet_status")).toBe(true);
});

it("refresh re-reads status (used by the fleet event)", async () => {
  const { invoke, api } = setup();
  await settle();
  const before = invoke.mock.calls.filter(([c]) => c === "fleet_status").length;
  await api.refresh();
  const after = invoke.mock.calls.filter(([c]) => c === "fleet_status").length;
  expect(after).toBe(before + 1);
});
