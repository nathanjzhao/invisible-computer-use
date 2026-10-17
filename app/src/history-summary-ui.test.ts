// @vitest-environment jsdom
import {readFileSync} from "node:fs";
import {afterEach,beforeEach,expect,it,vi} from "vitest";
import {wireHistorySummaries} from "./history-summary-ui";
const status={enabled:false,provider:"codex",needs_approval:false,generation_paused:false,generation_enabled:true,history_generation:"g",history_revision:2,summaries:0,last_status:null};
const summary={id:"id",app:"fixture.app",title:"Release check",description:"An observed release check.",text:"Olive icon, four reviewers.",written:1,source_events:5,omitted_events:0,incomplete:false,untrusted:true};
const get=<T extends HTMLElement=HTMLElement>(id:string)=>document.getElementById(id) as T;
const tick=()=>new Promise(r=>setTimeout(r,0));
let stop=()=>{};
beforeEach(()=>{document.documentElement.innerHTML=readFileSync(`${process.cwd()}/index.html`,"utf8");});
afterEach(()=>{stop();vi.useRealTimers();});

it("requires fresh provider-specific consent before enabling sharing",async()=>{
  const invoke=vi.fn(async(command:string,args?:Record<string,unknown>)=>command==="history_summary_list"?[]:command==="history_summary_configure"?{...status,enabled:true}:status);
  stop=wireHistorySummaries(document,invoke as never);await tick();
  get<HTMLInputElement>("history-summary-enabled").click();expect(get<HTMLButtonElement>("history-summary-save").disabled).toBe(true);
  get<HTMLInputElement>("history-summary-consent").click();expect(get<HTMLButtonElement>("history-summary-save").disabled).toBe(false);
  const provider=get<HTMLSelectElement>("history-summary-provider");provider.value="anthropic";provider.dispatchEvent(new Event("change"));
  expect(get<HTMLInputElement>("history-summary-consent").checked).toBe(false);expect(get<HTMLButtonElement>("history-summary-save").disabled).toBe(true);
  get<HTMLInputElement>("history-summary-consent").click();get("history-summary-save").click();await tick();
  expect(invoke).toHaveBeenCalledWith("history_summary_configure",{request:{enabled:true,provider:"anthropic",approved:true,history_generation:"g",history_revision:2}});
});

for(const event of ["icu-history-cleared","icu-memory-cleared"]){
  it(`discards a pending summary response after ${event}`,async()=>{
    let oldStatus!:(value:unknown)=>void,oldList!:(value:unknown)=>void;let statuses=0,lists=0;
    const invoke=vi.fn((command:string)=>command==="history_summary_status"?
      (++statuses===1?new Promise(resolve=>{oldStatus=resolve;}):Promise.resolve(status)):
      (++lists===1?new Promise(resolve=>{oldList=resolve;}):Promise.resolve([])));
    stop=wireHistorySummaries(document,invoke as never);
    document.dispatchEvent(new CustomEvent(event));await tick();
    oldStatus({...status,enabled:true});oldList([{...summary,text:"Deleted factual content"}]);await tick();
    expect(get("history-summary-list").textContent).not.toContain("Deleted factual content");
    expect(get<HTMLInputElement>("history-summary-enabled").checked).toBe(false);
  });
}

it("renders observed summary text without executing markup",async()=>{
  stop=wireHistorySummaries(document,(async(command:string)=>command==="history_summary_status"?status:[{...summary,title:"<img src=x>",text:"<script>unsafe()</script>"}]) as never);
  await tick();expect(get("history-summary-list").querySelector("img,script")).toBeNull();
  expect(get("history-summary-list").textContent).toContain("<script>unsafe()</script>");
});
it("distinguishes equal summary titles by activity period and deletes the chosen identity",async()=>{
  const first={...summary,id:'first',sha256:'one',from_ms:1000,to_ms:601000};
  const second={...summary,id:'second',sha256:'two',from_ms:601000,to_ms:1201000};
  const invoke=vi.fn(async(command:string)=>command==='history_summary_status'?status:[first,second]);
  const selections:unknown[]=[];const selected=(e:Event)=>selections.push((e as CustomEvent).detail);
  document.addEventListener('icu-delete-summary',selected);
  try {
    stop=wireHistorySummaries(document,invoke as never);await tick();
    const cards=[...get('history-summary-list').querySelectorAll('details')];
    expect(cards[0].querySelector('summary')!.getAttribute('aria-label')).not.toBe(cards[1].querySelector('summary')!.getAttribute('aria-label'));
    expect(cards[0].textContent).toContain('Activity');
    cards[1].querySelectorAll('button')[1].click();
    expect(selections).toEqual([{id:'second',sha256:'two',title:summary.title}]);
  }finally{document.removeEventListener('icu-delete-summary',selected);}
});

it("keeps a sharing failure visible through status polling until retry succeeds",async()=>{
  vi.useFakeTimers();let attempts=0;
  const invoke=vi.fn(async(command:string)=>{
    if(command==="history_summary_list")return [];
    if(command==="history_summary_configure"){
      if(++attempts===1)throw new Error("History storage is temporarily unavailable");
      return {...status,enabled:true};
    }
    return attempts>1?{...status,enabled:true}:status;
  });
  stop=wireHistorySummaries(document,invoke as never);await vi.advanceTimersByTimeAsync(0);
  get<HTMLInputElement>("history-summary-enabled").click();get<HTMLInputElement>("history-summary-consent").click();
  get("history-summary-save").click();await vi.advanceTimersByTimeAsync(4000);
  expect(get("history-summary-status").textContent).toContain("History storage is temporarily unavailable");
  expect(get<HTMLButtonElement>("history-summary-save").disabled).toBe(false);
  get("history-summary-save").click();await vi.advanceTimersByTimeAsync(0);
  expect(get("history-summary-status").textContent).toContain("0 content summaries");
});
