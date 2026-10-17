// @vitest-environment jsdom
import {readFileSync} from "node:fs";
import {beforeEach,expect,it,vi} from "vitest";
import {wireWorkflows,type WorkflowPlan,type ListedWorkflow} from "./workflow-ui";
const plan:WorkflowPlan={title:"Export note",description:"Export in the fixture",parameters:[{key:"project",label:"Project",description:"Project name"}],steps:[{description:"Set Project",evidence_lines:[2]},{description:"Click Export note",evidence_lines:[3]}],verification:"Exported filename is visible"};
const entry:ListedWorkflow={workflow:{id:"wf-saved",app_key:"dev.test.fixture",app_name:"Fixture",plan},sha256:"a".repeat(64)};
const get=<T extends HTMLElement=HTMLElement>(id:string)=>document.getElementById(id) as T;
const tick=()=>new Promise(r=>setTimeout(r,0));
beforeEach(()=>{document.documentElement.innerHTML=readFileSync(`${process.cwd()}/index.html`,"utf8");});
function setup(handler?:(command:string,args?:Record<string,unknown>)=>Promise<unknown>){
 const use=vi.fn();
 const invoke=vi.fn(handler??(async command=>command==="workflow_list"?[]:command==="workflow_generate"?{request_id:"wf-draft",proposal_sha256:"b".repeat(64),plan}:entry));
 wireWorkflows(get("trajectory-tools"),invoke as never,use);return {invoke,use};
}
function source(){get("trajectory-tools").dispatchEvent(new CustomEvent("icu-workflow-source",{detail:{approved:true,provider:"codex"}}));}
it("drafts without running or saving and requires fresh edited-plan consent",async()=>{
 const {invoke,use}=setup();source();await tick();expect(get("workflow-draft").hidden).toBe(false);
 expect(get<HTMLInputElement>("workflow-draft-title").value).toBe("Export note");expect(use).not.toHaveBeenCalled();
 expect(invoke.mock.calls.some(([c])=>c==="workflow_save")).toBe(false);expect(get<HTMLButtonElement>("workflow-save").disabled).toBe(true);
 const consent=get<HTMLInputElement>("workflow-save-consent");consent.checked=true;consent.dispatchEvent(new Event("change"));
 get<HTMLInputElement>("workflow-draft-title").value="My export";get("workflow-draft-title").dispatchEvent(new Event("input",{bubbles:true}));expect(consent.checked).toBe(false);
 consent.checked=true;consent.dispatchEvent(new Event("change"));get("workflow-save").click();await tick();
 expect(invoke).toHaveBeenCalledWith("workflow_save",{request:expect.objectContaining({approved:true,plan:expect.objectContaining({title:"My export"})})});
});
it("the native checkbox input event does not revoke its own approval",async()=>{
 setup();source();await tick();
 const consent=get<HTMLInputElement>("workflow-save-consent");
 consent.click();
 expect(consent.checked).toBe(true);expect(get<HTMLButtonElement>("workflow-save").disabled).toBe(false);
});
it("cancelling while the model runs never restores its late draft",async()=>{
 let resolve!:(v:unknown)=>void;
 const {invoke}=setup(async command=>command==="workflow_list"?[]:command==="workflow_generate"?new Promise(r=>{resolve=r;}):true);
 source();get("workflow-cancel").click();resolve({request_id:"wf-late",proposal_sha256:"x",plan});await tick();
 expect(get("workflow-draft").hidden).toBe(true);expect(get<HTMLInputElement>("workflow-draft-title").value).toBe("");
 expect(invoke.mock.calls.some(([c])=>c==="workflow_cancel")).toBe(true);expect(invoke.mock.calls.some(([c])=>c==="workflow_save")).toBe(false);
});
it("deleting History invalidates an outstanding draft",async()=>{
 setup();source();await tick();document.dispatchEvent(new CustomEvent("icu-history-cleared"));
 expect(get("workflow-draft").hidden).toBe(true);expect(get<HTMLButtonElement>("workflow-save").disabled).toBe(true);
});
it("saved workflows require all inputs and only prepare a user-started task",async()=>{
 const {use}=setup(async()=>[entry]);await tick();
 const choose=[...get("workflow-list").querySelectorAll("button")].find(b=>b.textContent==="Choose Export note")!;choose.click();
 get("workflow-use").click();expect(use).not.toHaveBeenCalled();
 get("workflow-input-fields").querySelector<HTMLTextAreaElement>("textarea")!.value="Juniper";get("workflow-use").click();
 expect(use).toHaveBeenCalledWith(entry,{id:"wf-saved",expected_sha256:entry.sha256,values:{project:"Juniper"},approved:true});
});
it("deleted or changed saved workflows cannot remain selected",async()=>{
 let rows=[entry];const {use}=setup(async()=>rows);await tick();get("workflow-list").querySelector<HTMLButtonElement>("button")!.click();
 rows=[];get("workflow-refresh").click();await tick();get("workflow-use").click();expect(use).not.toHaveBeenCalled();expect(get("workflow-inputs").hidden).toBe(true);
});
for(const event of ['icu-history-cleared','icu-memory-cleared']){
 it(`${event} clears selected workflow data and refreshes the saved list`,async()=>{
  let rows=[entry];const {use}=setup(async()=>rows);await tick();
  get('workflow-list').querySelector<HTMLButtonElement>('button')!.click();
  get('workflow-input-fields').querySelector<HTMLTextAreaElement>('textarea')!.value='REMOVED INPUT';
  rows=[];document.dispatchEvent(new CustomEvent(event));
  expect(get('workflow-inputs').hidden).toBe(true);expect(get('workflow-input-fields').children).toHaveLength(0);
  expect(get('workflow-selected-title').textContent).toBe('');
  await tick();expect(get('workflow-list').textContent).not.toContain('Export note');
  get('workflow-use').click();expect(use).not.toHaveBeenCalled();
 });
}
it('a pending list response cannot restore a workflow after History deletion',async()=>{
 let old!:(v:unknown)=>void,calls=0;
 setup(async command=>command==='workflow_list'&&++calls===1?new Promise(r=>{old=r;}):[]);
 document.dispatchEvent(new CustomEvent('icu-history-cleared'));await tick();
 old([entry]);await tick();expect(get('workflow-list').textContent).not.toContain('Export note');
});
