// @vitest-environment jsdom
import {readFileSync} from "node:fs";
import {afterEach,beforeEach,expect,it,vi} from "vitest";
import {wireHistoryDeletion} from "./history-delete-ui";
const get=<T extends HTMLElement=HTMLElement>(id:string)=>document.getElementById(id) as T;
const tick=()=>new Promise(r=>setTimeout(r,0));
const preview={generation:"owned",revision:1,events:[{id:"1".repeat(48),application:"fixture.app",timestamp_unix_ms:1000}],summaries:[],derivatives:[]};
let stop=()=>{};
beforeEach(()=>{document.documentElement.innerHTML=readFileSync(`${process.cwd()}/index.html`,"utf8");});
afterEach(()=>{stop();vi.useRealTimers();});
it("binds confirmation to the reviewed identities and never re-queries on delete",async()=>{
  const invoke=vi.fn(async(command:string)=>command==="computer_history_delete_preview"?preview:1);
  stop=wireHistoryDeletion(document,invoke as never);get("history-delete-preview").click();await tick();
  expect(get<HTMLButtonElement>("history-delete-selection-apply").disabled).toBe(true);
  get<HTMLInputElement>("history-delete-selection-confirm").click();get("history-delete-selection-apply").click();await tick();
  expect(invoke).toHaveBeenCalledWith("computer_history_delete_reviewed",{preview,approved:true});
  expect(invoke.mock.calls.filter(c=>c[0]==="computer_history_delete_preview")).toHaveLength(1);
  expect(get("history-delete-review").hidden).toBe(true);
});
it("cancelled and invalidated previews cannot return as an approved selection",async()=>{
  let resolve!:(value:unknown)=>void;const invoke=vi.fn(()=>new Promise(r=>{resolve=r;}));
  stop=wireHistoryDeletion(document,invoke as never);get("history-delete-preview").click();
  get("history-delete-selection-cancel").click();resolve(preview);await tick();
  expect(get("history-delete-review").hidden).toBe(true);expect(get<HTMLButtonElement>("history-delete-selection-apply").disabled).toBe(true);
  expect(invoke).toHaveBeenCalledTimes(1);
});
it("changing a filter revokes confirmation and a stale response requires another review",async()=>{
  const invoke=vi.fn(async(command:string)=>{if(command==="computer_history_delete_preview")return preview;throw new Error("History changed");});
  stop=wireHistoryDeletion(document,invoke as never);get("history-delete-preview").click();await tick();
  get<HTMLInputElement>("history-delete-selection-confirm").click();get<HTMLInputElement>("history-delete-app").dispatchEvent(new Event("input"));
  expect(get<HTMLInputElement>("history-delete-selection-confirm").checked).toBe(false);
  get("history-delete-preview").click();await tick();get<HTMLInputElement>("history-delete-selection-confirm").click();get("history-delete-selection-apply").click();await tick();
  expect(get("history-delete-selection-status").textContent).toContain("History changed");
  expect(get("history-delete-review").hidden).toBe(true);expect(get<HTMLButtonElement>("history-delete-selection-apply").disabled).toBe(true);
});
it("reviews a retained summary even when its raw observations have expired",async()=>{
  const key={id:"summary-id",sha256:"hash",title:"Retained task"};
  const empty={...preview,events:[],summaries:[{id:key.id,sha256:key.sha256}]};
  const invoke=vi.fn(async(command:string)=>command==="computer_history_delete_preview"?empty:0);
  stop=wireHistoryDeletion(document,invoke as never);
  document.dispatchEvent(new CustomEvent("icu-delete-summary",{detail:key}));await tick();
  expect(get("history-delete-selection-description").textContent).toContain("already expired");
  expect(get<HTMLButtonElement>("history-delete-selection-apply").disabled).toBe(true);
  get<HTMLInputElement>("history-delete-selection-confirm").click();get("history-delete-selection-apply").click();await tick();
  expect(invoke).toHaveBeenCalledWith("computer_history_delete_reviewed",{preview:empty,approved:true});
});
it("does not offer cancellation after confirmed deletion has begun",async()=>{
  let finish!:(n:number)=>void;
  const invoke=vi.fn(async(command:string)=>command==="computer_history_delete_preview"?preview:new Promise<number>(r=>{finish=r;}));
  stop=wireHistoryDeletion(document,invoke as never);get("history-delete-preview").click();await tick();
  get<HTMLInputElement>("history-delete-selection-confirm").click();get("history-delete-selection-apply").click();
  expect(get<HTMLButtonElement>("history-delete-selection-cancel").disabled).toBe(true);
  get("history-delete-selection-cancel").click();expect(get("history-delete-selection-status").textContent).not.toContain("cancelled");
  finish(1);await tick();expect(get("history-delete-selection-status").textContent).toContain("Deleted 1");
});
it("can confirm imported memories when no raw files or automatic summaries remain",async()=>{
  const value={...preview,events:[],derivatives:[{kind:"run",id:"imported-id",sha256:"hash"}]};
  const invoke=vi.fn(async(command:string)=>command==="computer_history_delete_preview"?value:0);
  stop=wireHistoryDeletion(document,invoke as never);get("history-delete-preview").click();await tick();
  expect(get("history-delete-selection-description").textContent).toContain("1 imported recordings");
  get<HTMLInputElement>("history-delete-selection-confirm").click();get("history-delete-selection-apply").click();await tick();
  expect(invoke).toHaveBeenCalledWith("computer_history_delete_reviewed",{preview:value,approved:true});
});
it("an unannounced app change cannot approve the old deletion",async()=>{
  const invoke=vi.fn(async(command:string)=>command==="computer_history_delete_preview"?preview:1);
  stop=wireHistoryDeletion(document,invoke as never);get("history-delete-preview").click();await tick();
  get<HTMLInputElement>("history-delete-app").value="another.app";
  get<HTMLInputElement>("history-delete-selection-confirm").click();get("history-delete-selection-apply").click();await tick();
  expect(invoke).toHaveBeenCalledTimes(1);expect(get("history-delete-review").hidden).toBe(true);
});
it("an unannounced range change after approval cannot delete",async()=>{
  const invoke=vi.fn(async(command:string)=>command==="computer_history_delete_preview"?preview:1);
  stop=wireHistoryDeletion(document,invoke as never);get("history-delete-preview").click();await tick();
  get<HTMLInputElement>("history-delete-selection-confirm").click();get<HTMLSelectElement>("history-delete-range").value="all";
  get("history-delete-selection-apply").click();await tick();expect(invoke).toHaveBeenCalledTimes(1);
});
it("a native filter change hides a stale review before confirmation",async()=>{
  vi.useFakeTimers();stop=wireHistoryDeletion(document,(async()=>preview) as never);
  get("history-delete-preview").click();await vi.advanceTimersByTimeAsync(0);
  get<HTMLInputElement>("history-delete-app").value="another.app";await vi.advanceTimersByTimeAsync(250);
  expect(get("history-delete-review").hidden).toBe(true);
  expect(get("history-delete-selection-status").textContent).toContain("changed");
});
it("summary review hides unrelated editable time and app filters",async()=>{
  stop=wireHistoryDeletion(document,(async()=>({...preview,summaries:[{id:'saved',sha256:'hash'}]})) as never);
  document.dispatchEvent(new CustomEvent('icu-delete-summary',{detail:{id:'saved',sha256:'hash',title:'Saved task'}}));await tick();
  expect(get('history-delete-filter').hidden).toBe(true);
  get('history-delete-selection-cancel').click();expect(get('history-delete-filter').hidden).toBe(false);
});
it("a failed summary preview restores the general deletion controls",async()=>{
  stop=wireHistoryDeletion(document,(async()=>{throw new Error('Summary was deleted');}) as never);
  document.dispatchEvent(new CustomEvent('icu-delete-summary',{detail:{id:'saved',sha256:'hash',title:'Saved task'}}));await tick();
  expect(get('history-delete-filter').hidden).toBe(false);
  expect(get<HTMLButtonElement>('history-delete-preview').disabled).toBe(false);
});
it("choosing another summary supersedes an in-flight preview",async()=>{
  const pending:((v:unknown)=>void)[]=[];
  stop=wireHistoryDeletion(document,(()=>new Promise(r=>{pending.push(r);})) as never);
  for(const title of ['First task','Second task'])document.dispatchEvent(new CustomEvent('icu-delete-summary',{detail:{id:title,sha256:'hash',title}}));
  pending[1](preview);await tick();pending[0](preview);await tick();
  expect(get('history-delete-selection-description').textContent).toContain('Second task');
  expect(get('history-delete-selection-description').textContent).not.toContain('First task');
});
