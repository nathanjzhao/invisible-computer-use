// Memory controls: the topbar state chip, the settings panel with its list of
// stored memories, the per-run switches, and the log line for what a run used.
// The backend owns every rule; this only shows state and sends choices.

export type MemorySettings = {
  chatMemories: boolean;
  skysight: boolean;
  paused: boolean;
  retentionDays: number;
  idleMinutes: number;
  provider: string;
  extractModel: string | null;
  consolidationModel: string | null;
};

export type MemoryStatus = {
  state: "on" | "off" | "paused" | "rate_limited" | "waiting" | "storage_unavailable";
  settings: MemorySettings;
  ratePausedUntil: number | null;
  lastError: string | null;
  lastNote?: string | null;
};

/** The status line under the Memory chip: the error if there is one, else a short note. */
export function memoryStatusSuffix(s: { lastError: string | null; lastNote?: string | null }): string {
  if (s.lastError) return ` · last error: ${s.lastError}`;
  if (s.lastNote) return ` · note: ${s.lastNote}`;
  return "";
}

export type MemoryEntry = { id: string; scope: string; kind: string; text: string; untrusted: boolean; evidence: string[] };
export type RecordingOrigin = { project: string; actor: string; recording_sha256: string };
export type ChatSummary = { id: string; app_name: string; title: string; asked: string; ended: number; untrusted: boolean; notes: { kind: string; text: string }[]; recordings?: RecordingOrigin[] };
export type MemoryList = {
  entries: MemoryEntry[];
  summaries: ChatSummary[];
  pending: { runId: string; app: string; goal: string; ended: number; recording?: RecordingOrigin | null }[];
  skysight: { summaries: number; profile: string[] };
};

/// The chip text and its tone. A paused or rate-limited state says so plainly.
export function memoryChip(s: MemoryStatus, now = Date.now() / 1000): { text: string; tone: "ok" | "warn" | "off" } {
  switch (s.state) {
    case "off": return { text: "Memory off", tone: "off" };
    case "paused": return { text: "Memory paused", tone: "warn" };
    case "storage_unavailable": return { text: "Memory storage unavailable", tone: "warn" };
    case "rate_limited": {
      const mins = s.ratePausedUntil ? Math.max(1, Math.ceil((s.ratePausedUntil - now) / 60)) : null;
      return { text: mins ? `Memory waiting on rate limit (${mins} min)` : "Memory waiting on rate limit", tone: "warn" };
    }
    case "waiting": return { text: "Memory on (waits for the task)", tone: "ok" };
    default: return { text: s.settings.skysight ? "Memory on · Activity summaries" : "Memory on", tone: "ok" };
  }
}

/// Where memory passes run, as the panel states it. The backend sends a task's
/// memory to the provider that answered it (the hosted one when a fallback
/// answered part of it, the local model when only it answered), unless the
/// `provider` setting names one provider for every pass. Stored memories go to
/// later tasks on the same app whatever provider runs them.
export function providerNote(s: MemorySettings): string {
  const later = "Stored memories are given to later tasks on the same app, whichever provider runs them.";
  if (!s.provider) {
    return "Memories from a task are written by the model provider that answered it, or by the hosted one when a fallback answered part of it. "
      + "A task only the local model answered is summarized on the local model. " + later;
  }
  return `Every memory pass runs on ${s.provider} (the provider setting), whichever provider ran the task. ` + later;
}

/// The per-run switches as `agent_run` takes them.
export function runMemoryOptions(useMemory: boolean, remember: boolean): { useMemory: boolean; remember: boolean } {
  return { useMemory, remember };
}

/// The action-log line for a run's `memory` event.
export function memoryLine(d: { state?: string; tainted?: boolean; used?: { kind: string; text: string; untrusted?: boolean; evidence?:string[] }[]; sources?: { key: {kind:string;id:string;sha256:string;field_hashes?:string[]};title:string;app:string;at:number }[] }, escape: (s: string) => string): string {
  const used = d.used ?? [];
  if (!used.length) return `<span class="muted"><b>memory</b> none used (${escape(d.state || "nothing relevant stored")})</span>`;
  const items = used.map(u => {
    const refs=(d.sources??[]).flatMap((s,i)=>u.evidence?.includes(s.key.id)?[i+1]:[]);
    const citation=refs.length?` <span aria-label="Source ${refs.join(", ")}">[${refs.join(", ")}]</span>`:u.evidence?" <span class=\"muted\">[source unavailable]</span>":"";
    return `<li>${escape(u.kind.replace("_", " "))}${u.untrusted ? " · observed" : ""}: ${escape(u.text)}${citation}</li>`;
  }).join("");
  const asks = d.tainted ? `<div class="mem-asks">Some notes came from what apps showed, not from you, so this task asks you before it sends, pays, deletes, submits or publishes anything.</div>` : "";
  const sources=(d.sources??[]).map((s,i)=>`<button type="button" class="ghost" data-memory-source="${escape(JSON.stringify(s.key))}">Review source ${i+1}: ${escape(s.title)}</button>`).join("");
  return `<b>memory</b> ${used.length} note${used.length === 1 ? "" : "s"} given to the model as background<ul class="mem-used">${items}</ul>${sources}${asks}`;
}

/// A step waiting for the user's answer, as the backend's `confirm` event says it.
export type ConfirmRequest = { id: string; action: string; why: string; app: string };

/// The question shown in the chat for a `confirm` event. Plain words; the
/// buttons carry the request id.
export function confirmHtml(d: ConfirmRequest, escape: (s: string) => string): string {
  return `<div class="confirm-text">The task wants to <b>${escape(d.action)}</b> in ${escape(d.app)}. `
    + `This looks like it sends, pays, deletes, submits or publishes something (${escape(d.why)}). `
    + `It was given memory notes that came from what apps showed, not from you, so it waits for your answer.</div>`
    + `<div class="confirm-actions"><button type="button" class="ghost accent confirm-allow" data-id="${escape(d.id)}">Allow this step</button> `
    + `<button type="button" class="ghost confirm-deny" data-id="${escape(d.id)}">Don't allow</button></div>`;
}

/// What the chat says once a confirmation is settled.
export function confirmedText(answer: string): string {
  switch (answer) {
    case "approved": return "Allowed. The step runs.";
    case "declined": return "Not allowed. The step did not run, and the task was told.";
    case "timeout": return "No answer in time. The step did not run, and the task was told.";
    case "gone": return "The task was no longer waiting for this answer (it timed out or stopped). The step did not run.";
    default: return "The task stopped before the step ran.";
  }
}

/// Settings from the panel's inputs, checked the way the backend checks them.
/// The fields of `next` that differ from `base`: what a save sends, so a field
/// another app instance changed since the panel loaded is not written back.
export function settingsPatch(base: MemorySettings, next: MemorySettings): Partial<MemorySettings> {
  const patch: Partial<MemorySettings> = {};
  for (const k of Object.keys(next) as (keyof MemorySettings)[]) {
    if (JSON.stringify(next[k]) !== JSON.stringify(base[k])) (patch as Record<string, unknown>)[k] = next[k];
  }
  return patch;
}

export function readSettings(base: MemorySettings, form: { chat: boolean; skysight: boolean; retention: string }): MemorySettings | string {
  const days = Number(form.retention.trim());
  if (!Number.isInteger(days) || days < 1 || days > 3650) return "Retention is a whole number of days from 1 to 3650.";
  return { ...base, chatMemories: form.chat, skysight: form.skysight, retentionDays: days };
}

/// The list of stored memories, each with its delete control (data-del holds the id).
export function memoryListHtml(list: MemoryList, escape: (s: string) => string): string {
  const del = (id: string) => `<button class="ghost mem-del" data-del="${escape(id)}" title="Delete">Delete</button>`;
  const origin = (r: RecordingOrigin) => `<div class="muted">Recording · ${escape(r.actor.replace(/_/g, " "))} · project ${escape(r.project)}</div>`;
  const rows: string[] = [];
  if (list.entries.length) {
    rows.push(`<div class="mem-h">Durable memories</div>`);
    for (const e of list.entries) {
      rows.push(`<div class="mem-row"><div><span class="mem-scope">${escape(e.scope)}</span> <span class="muted">${escape(e.kind.replace("_", " "))}${e.untrusted ? " · observed" : ""}</span><div>${escape(e.text)}</div></div>${del(e.id)}</div>`);
    }
  }
  if (list.summaries.length) {
    rows.push(`<div class="mem-h">Session memories</div>`);
    for (const s of list.summaries) {
      rows.push(`<div class="mem-row"><div><span class="mem-scope">${escape(s.app_name)}</span> ${escape(s.title)}<div class="muted">${escape(s.asked)}</div>${(s.recordings ?? []).map(origin).join("")}</div>${del(s.id)}</div>`);
    }
  }
  if (list.pending.length) {
    rows.push(`<div class="mem-h">Runs waiting to be summarized</div>`);
    for (const p of list.pending) rows.push(`<div class="mem-row"><div><span class="mem-scope">${escape(p.app)}</span> ${escape(p.goal)}${p.recording ? origin(p.recording) : ""}</div>${del(`run:${p.runId}`)}</div>`);
  }
  if (list.skysight.summaries || list.skysight.profile.length) {
    rows.push(`<div class="mem-h">Activity summaries</div><div class="mem-row"><div>${list.skysight.summaries} activity summaries, ${list.skysight.profile.length} profile lines</div>${del("skysight")}</div>`);
  }
  return rows.length ? rows.join("") : `<div class="pick-empty">No memories yet.</div>`;
}
