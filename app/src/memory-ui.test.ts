import { describe, it, expect } from "vitest";
import { memoryStatusSuffix, confirmHtml, confirmedText, memoryChip, memoryLine, readSettings, settingsPatch, memoryListHtml, providerNote, runMemoryOptions, type MemorySettings, type MemoryStatus } from "./memory-ui";

const esc = (s: string) => s.replace(/&/g, "&amp;").replace(/</g, "&lt;").replace(/>/g, "&gt;").replace(/"/g, "&quot;");
const settings: MemorySettings = { chatMemories: true, skysight: false, paused: false, retentionDays: 7, idleMinutes: 30, provider: "", extractModel: null, consolidationModel: null };
const status = (state: MemoryStatus["state"], extra: Partial<MemoryStatus> = {}): MemoryStatus => ({ state, settings, ratePausedUntil: null, lastError: null, ...extra });

describe("memory chip", () => {
  it("shows every state, the pause plainly", () => {
    expect(memoryChip(status("on"))).toEqual({ text: "Memory on", tone: "ok" });
    expect(memoryChip(status("on", { settings: { ...settings, skysight: true } })).text).toBe("Memory on · Activity summaries");
    expect(memoryChip(status("paused"))).toEqual({ text: "Memory paused", tone: "warn" });
    expect(memoryChip(status("storage_unavailable"))).toEqual({ text: "Memory storage unavailable", tone: "warn" });
    expect(memoryChip(status("off")).tone).toBe("off");
    expect(memoryChip(status("rate_limited", { ratePausedUntil: 1000 + 600 }), 1000).text).toBe("Memory waiting on rate limit (10 min)");
  });
});

describe("memory event line", () => {
  it("lists what the run was given, escaped, with observed notes marked", () => {
    const line = memoryLine({ state: "used", used: [{ kind: "preference", text: "The user wants <b>caps</b>.", untrusted: true }] }, esc);
    expect(line).toContain("1 note given to the model as background");
    expect(line).toContain("preference · observed: The user wants &lt;b&gt;caps&lt;/b&gt;.");
    expect(line).not.toContain("<b>caps");
  });
  it("says when the run will ask before consequential steps, and only then", () => {
    const used = [{ kind: "app_fact", text: "x", untrusted: true }];
    expect(memoryLine({ state: "used", tainted: true, used }, esc)).toContain("asks you before it sends, pays, deletes, submits or publishes");
    expect(memoryLine({ state: "used", tainted: false, used }, esc)).not.toContain("asks you");
  });
  it("asks plainly, escaped, with the request id on both buttons", () => {
    const html = confirmHtml({ id: "c1", action: "click \"Send <now>\"", why: "its target \"Send\" sends", app: "Mail & Co" }, esc);
    expect(html).toContain("The task wants to <b>click &quot;Send &lt;now&gt;&quot;</b> in Mail &amp; Co.");
    expect(html).toContain("not from you, so it waits for your answer");
    expect(html.match(/data-id="c1"/g)?.length).toBe(2);
    expect(html).toContain("Allow this step");
    expect(html).toContain("Don't allow");
    expect(html).not.toMatch(/—/);
  });
  it("says what happened to the step", () => {
    expect(confirmedText("approved")).toContain("runs");
    expect(confirmedText("declined")).toContain("did not run");
    expect(confirmedText("gone")).toContain("no longer waiting");
    expect(confirmedText("timeout")).toContain("No answer in time");
    expect(confirmedText("stopped")).toContain("stopped");
  });
  it("says why nothing was used", () => {
    expect(memoryLine({ state: "off for this run", used: [] }, esc)).toContain("none used (off for this run)");
  });
});

describe("memory settings and per-run switches", () => {
  it("refuses a retention the backend would refuse", () => {
    for (const bad of ["0", "3651", "1.5", "", "seven"]) expect(typeof readSettings(settings, { chat: true, skysight: false, retention: bad })).toBe("string");
    expect(readSettings(settings, { chat: false, skysight: true, retention: " 14 " })).toEqual({ ...settings, chatMemories: false, skysight: true, retentionDays: 14 });
  });
  it("saves only the fields the form changed", () => {
    const next = readSettings(settings, { chat: true, skysight: false, retention: "20" }) as MemorySettings;
    expect(settingsPatch(settings, next)).toEqual({ retentionDays: 20 });
    expect(settingsPatch(settings, { ...settings })).toEqual({});
  });
  it("passes the per-run switches through", () => {
    expect(runMemoryOptions(false, true)).toEqual({ useMemory: false, remember: true });
  });
});

describe("memory list", () => {
  it("keeps pending run identifiers separate from bulk and generated-memory selectors",()=>{
    const html=memoryListHtml({entries:[],summaries:[],pending:[{runId:"all",app:"Fixture",goal:"One pending task",ended:1}],skysight:{summaries:0,profile:[]}},esc);
    expect(html).toContain('data-del="run:all"');expect(html).not.toContain('data-del="all"');
  });
  it("gives every stored memory a delete control and escapes ids and text", () => {
    const html = memoryListHtml({
      entries: [{ id: "e-1", scope: "Pages", kind: "app_fact", text: "<script>x</script>", untrusted: true, evidence: ["s-1"] }],
      summaries: [{ id: "s-1", app_name: "Pages", title: "Titles", asked: "The user asked.", ended: 1, untrusted: true, notes: [] }],
      pending: [{ runId: "run-\"x", app: "Pages", goal: "set title", ended: 1 }],
      skysight: { summaries: 2, profile: [] },
    }, esc);
    expect(html.match(/data-del=/g)?.length).toBe(4);
    expect(html).toContain('data-del="run:run-&quot;x"');
    expect(html).toContain("app fact · observed");
    expect(html).not.toContain("<script>");
    expect(memoryListHtml({ entries: [], summaries: [], pending: [], skysight: { summaries: 0, profile: [] } }, esc)).toContain("No memories yet.");
  });
});

describe("provider note", () => {
  it("says where memory passes run, and names the provider setting when it is set", () => {
    expect(providerNote(settings)).toContain("A task only the local model answered is summarized on the local model.");
    expect(providerNote(settings)).toContain("whichever provider runs them");
    expect(providerNote(settings)).not.toContain("never sent to a hosted one");
    const set = providerNote({ ...settings, provider: "claude-code" });
    expect(set).toContain("Every memory pass runs on claude-code");
    expect(set).not.toContain("summarized on the local model");
  });
  it("matches the text the page starts with", async () => {
    const html = (await import("node:fs")).readFileSync(new URL("../index.html", import.meta.url), "utf8");
    expect(html).toContain(`<p id="mem-provider-note" class="muted">${providerNote(settings)}</p>`);
  });
});

describe("memory status suffix", () => {
  it("shows an error first, then a note, else nothing", () => {
    expect(memoryStatusSuffix({ lastError: "boom", lastNote: "n" })).toBe(" · last error: boom");
    expect(memoryStatusSuffix({ lastError: null, lastNote: "a replacement was not applied" })).toBe(" · note: a replacement was not applied");
    expect(memoryStatusSuffix({ lastError: null })).toBe("");
    expect(memoryStatusSuffix({ lastError: null, lastNote: null })).toBe("");
  });
});
