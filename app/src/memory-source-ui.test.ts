// @vitest-environment jsdom
import { readFileSync } from "node:fs";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { wireMemorySources } from "./memory-source-ui";
import { memoryLine } from "./memory-ui";
const escape=(s:string)=>s.replace(/&/g,"&amp;").replace(/</g,"&lt;").replace(/>/g,"&gt;").replace(/"/g,"&quot;");
const key={kind:"session",id:"s-one",sha256:"a".repeat(64)};
const source={key,title:"Original task",app:"Editor",at:1700000000};
const tick=()=>new Promise(resolve=>setTimeout(resolve,0));
let stop=()=>{};
beforeEach(()=>{
  document.documentElement.innerHTML=readFileSync(`${process.cwd()}/index.html`,"utf8");
  const style=document.createElement("style");style.textContent=readFileSync(`${process.cwd()}/src/styles.css`,"utf8");document.head.append(style);
  const row=document.createElement("div");row.id="test-recall";
  row.innerHTML=memoryLine({used:[{kind:"app_fact",text:"Observed title",evidence:[key.id]}],sources:[source]},escape);
  document.body.append(row);
});
afterEach(()=>{stop();vi.useRealTimers();});
const open=()=>document.querySelector<HTMLButtonElement>("[data-memory-source]")!.click();

it("maps a note to its source and renders the exact reviewed body as inert text",async()=>{
  const invoke=vi.fn(async()=>({source:{...source,title:"<img src=x>"},text:"<script>injected()</script>"}));
  stop=wireMemorySources(document,invoke);open();await tick();
  expect(invoke).toHaveBeenCalledWith("memory_source",{key});
  expect(document.getElementById("test-recall")!.textContent).toContain("[1]");
  expect(document.getElementById("memory-source")!.querySelector("img,script")).toBeNull();
  expect(document.getElementById("memory-source-text")!.textContent).toBe("<script>injected()</script>");
  expect(document.activeElement?.id).toBe("memory-source-title");
});

it("preserves exact observed-field bindings when opening a source",async()=>{
  const selected={...key,kind:"history",id:"b".repeat(64)+".json",field_hashes:["c".repeat(64)]};
  document.querySelector<HTMLButtonElement>("[data-memory-source]")!.dataset.memorySource=JSON.stringify(selected);
  const invoke=vi.fn(async()=>({source:{...source,key:selected},text:"Earlier observed field"}));
  stop=wireMemorySources(document,invoke);open();await tick();
  expect(invoke).toHaveBeenCalledWith("memory_source",{key:selected});
  expect(document.getElementById("memory-source-text")!.textContent).toBe("Earlier observed field");
});

for(const event of ["icu-memory-cleared","icu-history-cleared"]){
  it(`cannot restore deleted source text from an older response after ${event}`,async()=>{
    let complete!:(value:unknown)=>void;
    stop=wireMemorySources(document,()=>new Promise(resolve=>{complete=resolve;}));open();
    document.dispatchEvent(new CustomEvent(event));complete({source,text:"Deleted content"});await tick();
    expect(document.getElementById("memory-source-text")!.textContent).toBe("");
    expect(document.getElementById("memory-source")!.hidden).toBe(true);
    expect(getComputedStyle(document.getElementById("memory-source")!).display).toBe("none");
  });
}

it("removes a source another app instance deleted while it is open",async()=>{
  vi.useFakeTimers();let removed=false;
  stop=wireMemorySources(document,async()=>{if(removed)throw new Error("Source removed");return {source,text:"Earlier source"};});
  open();await vi.advanceTimersByTimeAsync(1);expect(document.getElementById("memory-source-text")!.textContent).toBe("Earlier source");
  removed=true;await vi.advanceTimersByTimeAsync(20_000);
  expect(document.getElementById("memory-source-text")!.textContent).toBe("");
  expect(document.getElementById("memory-source-title")!.textContent).toBe("Memory source unavailable");
});

it("does not attach an unrelated source to a note with missing evidence",()=>{
  const html=memoryLine({used:[{kind:"app_fact",text:"Ungrounded",evidence:["s-other"]}],sources:[source]},escape);
  const el=document.createElement("div");el.innerHTML=html;
  expect(el.querySelector("li")!.textContent).toContain("source unavailable");
  expect(el.querySelector("li")!.textContent).not.toContain("[1]");
});
