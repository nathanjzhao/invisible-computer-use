// @vitest-environment jsdom
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { wireTrajectoryImport, type RecordingPreview } from "./trajectory-ui";
const preview: RecordingPreview = {recording_sha256:"a".repeat(64),manifest_sha256:"b".repeat(64),apps:{"test.app":3},steps:{"test.app":[{source_line:7,action:"Observed ui.click on AXButton \"Multiply\""}]},semantic_events:3,secure_events_omitted:1,has_values:true,memory_policy:"Values excluded."};
let dispose:(()=>void)|void;
function fixture(handler?: (command:string,args?:Record<string,unknown>)=>Promise<unknown>) {
 document.body.innerHTML=`<div id="root"><input id="trajectory-path" value="/recording"><input id="trajectory-values" type="checkbox"><select id="trajectory-app"></select><p id="trajectory-message"></p><button id="trajectory-review"></button><button id="trajectory-import" disabled></button><input id="trajectory-consent" type="checkbox"><div id="trajectory-options" hidden></div><p id="trajectory-preview-count"></p><ol id="trajectory-preview-steps"></ol><button id="trajectory-prev"></button><button id="trajectory-next"></button><input id="trajectory-project" value="test"><input id="trajectory-goal" value="Multiply"><select id="trajectory-provider"><option value="local">Local</option></select><select id="trajectory-actor"><option value="human">Human</option></select></div>`;
 const invoke=vi.fn(handler ?? (async (command:string)=>command==="trajectory_preview"?preview:{imported:true}));
 dispose=wireTrajectoryImport(document.querySelector("#root")!,invoke as never,async()=>{});
 return invoke;
}
const click=(id:string)=>document.querySelector<HTMLButtonElement>(`#${id}`)!.click();
const tick=async()=>{await new Promise(r=>setTimeout(r,0));};
beforeEach(()=>vi.restoreAllMocks());
afterEach(()=>{if(typeof dispose==='function')dispose();vi.useRealTimers();});
it("review does not import; explicit consent sends the reviewed identities",async()=>{
 const invoke=fixture();click("trajectory-review");await tick();
 expect(invoke).toHaveBeenCalledTimes(1);expect(document.querySelector<HTMLButtonElement>("#trajectory-import")!.disabled).toBe(true);
 const consent=document.querySelector<HTMLInputElement>("#trajectory-consent")!;consent.checked=true;consent.dispatchEvent(new Event("change"));click("trajectory-import");await tick();
 expect(invoke).toHaveBeenLastCalledWith("trajectory_import",{request:expect.objectContaining({approved:true,expected_sha256:preview.recording_sha256,expected_manifest_sha256:preview.manifest_sha256,app:"test.app",provider:"local",actor:"human"})});
});
it("changing the path invalidates consent and blocks stale imports",async()=>{
 const invoke=fixture();click("trajectory-review");await tick();
 const consent=document.querySelector<HTMLInputElement>("#trajectory-consent")!;consent.checked=true;consent.dispatchEvent(new Event("change"));
 const path=document.querySelector<HTMLInputElement>("#trajectory-path")!;path.value="/other";path.dispatchEvent(new Event("input"));click("trajectory-import");await tick();
 expect(invoke).toHaveBeenCalledTimes(1);expect(consent.checked).toBe(false);
});

it("renders exact reviewed steps as text and revokes consent when the provider changes",async()=>{
 const invoke=fixture();click("trajectory-review");await tick();
 expect(document.querySelector("#trajectory-preview-steps")!.textContent).toContain('Line 7: Observed ui.click');
 expect(document.querySelector("#trajectory-preview-count")!.textContent).toContain('3 eligible events will be imported');
 const consent=document.querySelector<HTMLInputElement>("#trajectory-consent")!;consent.checked=true;consent.dispatchEvent(new Event("change"));
 document.querySelector("#trajectory-provider")!.dispatchEvent(new Event("change"));click("trajectory-import");await tick();
 expect(consent.checked).toBe(false);expect(invoke).toHaveBeenCalledTimes(1);
});
it("does not interpret recording labels as markup",async()=>{
 const original=preview.steps["test.app"][0].action;
 preview.steps["test.app"][0].action='<img src=x onerror=alert(1)>';
 try{fixture();click("trajectory-review");await tick();expect(document.querySelector("#trajectory-preview-steps img")).toBeNull();expect(document.querySelector("#trajectory-preview-steps")!.textContent).toContain('<img');}
 finally{preview.steps["test.app"][0].action=original;}
});

it("pages through all chunks with reviewed hashes and renews consent",async()=>{
 const long={...preview,apps:{"test.app":81}};
 const invoke=fixture(async command=>command==="trajectory_preview"?long:{steps:[{source_line:81,action:"Last recorded event"}]});
 click("trajectory-review");await tick();expect(document.querySelector<HTMLButtonElement>("#trajectory-next")!.disabled).toBe(false);
 const consent=document.querySelector<HTMLInputElement>("#trajectory-consent")!;consent.checked=true;consent.dispatchEvent(new Event("change"));
 click("trajectory-next");await tick();
 expect(invoke).toHaveBeenLastCalledWith("trajectory_preview_page",{request:expect.objectContaining({app:"test.app",page:1,expected_sha256:preview.recording_sha256,expected_manifest_sha256:preview.manifest_sha256})});
 expect(consent.checked).toBe(false);expect(document.querySelector("#trajectory-preview-steps")!.textContent).toContain("Last recorded event");
 expect(document.querySelector<HTMLButtonElement>("#trajectory-prev")!.disabled).toBe(false);
});
it("discards an in-flight page after the recording path changes",async()=>{
 let resolve!:(v:unknown)=>void;
 fixture(async command=>command==="trajectory_preview"?{...preview,apps:{"test.app":81}}:new Promise(r=>{resolve=r;}));
 click("trajectory-review");await tick();click("trajectory-next");
 const path=document.querySelector<HTMLInputElement>("#trajectory-path")!;path.value="/other";path.dispatchEvent(new Event("input"));
 resolve({steps:[{source_line:70,action:"STALE_PAGE"}]});await tick();
 expect(document.querySelector("#trajectory-preview-steps")!.textContent).not.toContain("STALE_PAGE");
 expect(document.querySelector<HTMLElement>("#trajectory-options")!.hidden).toBe(true);expect(document.querySelector<HTMLButtonElement>("#trajectory-import")!.disabled).toBe(true);
});
it("invalidates review when hash-bound paging fails",async()=>{
 fixture(async command=>{if(command==="trajectory_preview")return {...preview,apps:{"test.app":81}};throw new Error("recording changed");});
 click("trajectory-review");await tick();click("trajectory-next");await tick();
 expect(document.querySelector("#trajectory-message")!.textContent).toContain("recording changed");
 expect(document.querySelector<HTMLElement>("#trajectory-options")!.hidden).toBe(true);
});
it("value sharing needs its own reviewed projection and preserves the byte boundary",async()=>{
 const valuePreview={...preview,includes_values:true,reviewed_bytes:1200,source_format:"computer-history-v1"};
 const invoke=fixture(async command=>command==='trajectory_preview'?valuePreview:{imported:true});
 const values=document.querySelector<HTMLInputElement>('#trajectory-values')!;values.checked=true;values.dispatchEvent(new Event('change'));click('trajectory-review');await tick();
 expect(invoke).toHaveBeenLastCalledWith('trajectory_preview',{directory:'/recording',includeValues:true});
 const consent=document.querySelector<HTMLInputElement>('#trajectory-consent')!;consent.checked=true;consent.dispatchEvent(new Event('change'));click('trajectory-import');await tick();
 expect(invoke).toHaveBeenLastCalledWith('trajectory_import',{request:expect.objectContaining({include_values:true,reviewed_bytes:1200})});
});
it("changing the value option revokes the previous review and consent",async()=>{
 const invoke=fixture();click('trajectory-review');await tick();const consent=document.querySelector<HTMLInputElement>('#trajectory-consent')!;consent.checked=true;consent.dispatchEvent(new Event('change'));
 const values=document.querySelector<HTMLInputElement>('#trajectory-values')!;values.checked=true;values.dispatchEvent(new Event('change'));click('trajectory-import');await tick();expect(invoke).toHaveBeenCalledTimes(1);expect(consent.checked).toBe(false);
});
it("history selection resets sharing and starts a local review",async()=>{
 const invoke=fixture();document.querySelector<HTMLInputElement>('#trajectory-values')!.checked=true;
 document.querySelector('#root')!.dispatchEvent(new CustomEvent('icu-review-history',{detail:{path:'/history/segment.jsonl'}}));await tick();
 expect(invoke).toHaveBeenLastCalledWith('trajectory_preview',{directory:'/history/segment.jsonl',includeValues:false});expect(document.querySelector<HTMLInputElement>('#trajectory-values')!.checked).toBe(false);
});
it("a pending review cannot restore text after History is cleared",async()=>{
 let resolve!:(v:unknown)=>void;
 fixture(async()=>new Promise(r=>{resolve=r;}));
 click('trajectory-review');
 document.dispatchEvent(new CustomEvent('icu-history-cleared'));
 resolve({...preview,steps:{'test.app':[{source_line:1,action:'DELETED_REVIEW_TEXT'}]}});await tick();
 expect(document.querySelector('#trajectory-preview-steps')!.textContent).not.toContain('DELETED_REVIEW_TEXT');
 expect(document.querySelector<HTMLElement>('#trajectory-options')!.hidden).toBe(true);
 expect(document.querySelector<HTMLButtonElement>('#trajectory-import')!.disabled).toBe(true);
 expect(document.querySelector('#trajectory-message')!.textContent).toContain('History cleared');
});
it("native selection without a DOM change event refreshes the visible app and revokes consent",async()=>{
 vi.useFakeTimers();
 const two={...preview,apps:{'test.app':3,'other.app':55},steps:{...preview.steps,'other.app':[{source_line:9,action:'OTHER APP OBSERVATION'}]}};
 fixture(async()=>two);click('trajectory-review');await vi.advanceTimersByTimeAsync(0);
 const consent=document.querySelector<HTMLInputElement>('#trajectory-consent')!;consent.click();
 document.querySelector<HTMLSelectElement>('#trajectory-app')!.value='other.app';
 await vi.advanceTimersByTimeAsync(250);
 expect(document.querySelector('#trajectory-preview-count')!.textContent).toContain('55 eligible events');
 expect(document.querySelector('#trajectory-preview-steps')!.textContent).toContain('OTHER APP OBSERVATION');
 expect(consent.checked).toBe(false);expect(document.querySelector<HTMLButtonElement>('#trajectory-import')!.disabled).toBe(true);
});
it("approval cannot race a native scope change before its next refresh",async()=>{
 const two={...preview,apps:{'test.app':3,'other.app':55},steps:{...preview.steps,'other.app':[{source_line:9,action:'OTHER APP OBSERVATION'}]}};
 const invoke=fixture(async()=>two);click('trajectory-review');await tick();
 document.querySelector<HTMLSelectElement>('#trajectory-app')!.value='other.app';
 click('trajectory-consent');click('trajectory-import');await tick();
 expect(invoke.mock.calls.some(c=>c[0]==='trajectory_import')).toBe(false);
 expect(document.querySelector<HTMLInputElement>('#trajectory-consent')!.checked).toBe(false);
 expect(document.querySelector('#trajectory-preview-steps')!.textContent).toContain('OTHER APP OBSERVATION');
});
it("an unannounced provider change cannot reuse an earlier approval",async()=>{
 const invoke=fixture();click('trajectory-review');await tick();click('trajectory-consent');
 const provider=document.querySelector<HTMLSelectElement>('#trajectory-provider')!;provider.add(new Option('Codex login','codex'));provider.value='codex';
 click('trajectory-import');await tick();
 expect(invoke.mock.calls.some(c=>c[0]==='trajectory_import')).toBe(false);
 expect(document.querySelector<HTMLInputElement>('#trajectory-consent')!.checked).toBe(false);
});
it("a late page cannot leave rows from the previously selected app",async()=>{
 let finish!:(v:unknown)=>void,calls=0;
 const two={...preview,apps:{'test.app':121,'other.app':55},steps:{...preview.steps,'other.app':[{source_line:9,action:'OTHER APP OBSERVATION'}]}};
 fixture(async command=>command==='trajectory_preview'?two:++calls===1?{steps:[{source_line:50,action:'PREVIOUS APP PAGE'}]}:new Promise(r=>{finish=r;}));
 click('trajectory-review');await tick();click('trajectory-next');await tick();click('trajectory-next');
 document.querySelector<HTMLSelectElement>('#trajectory-app')!.value='other.app';
 finish({steps:[{source_line:90,action:'STALE RESPONSE'}]});await tick();
 expect(document.querySelector('#trajectory-preview-steps')!.textContent).toContain('OTHER APP OBSERVATION');
 expect(document.querySelector('#trajectory-preview-steps')!.textContent).not.toMatch(/PREVIOUS APP PAGE|STALE RESPONSE/);
 expect(document.querySelector('#trajectory-preview-count')!.textContent).toContain('Showing chunk 1');
});
