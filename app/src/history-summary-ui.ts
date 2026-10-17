type Invoke=<T>(command:string,args?:Record<string,unknown>)=>Promise<T>;
type Status={enabled:boolean;provider:string;needs_approval:boolean;generation_paused:boolean;generation_enabled:boolean;
  history_generation:string;history_revision:number;summaries:number;last_status:null|{state:string;message?:string}};
type Summary={id:string;sha256:string;app:string;title:string;description:string;text:string;written:number;from_ms?:number;to_ms?:number;source_events:number;omitted_events:number;incomplete:boolean;untrusted:boolean};

export function wireHistorySummaries(root:Document,invoke:Invoke):()=>void{
  const get=<T extends HTMLElement>(id:string)=>root.getElementById(id) as T;
  const enabled=get<HTMLInputElement>("history-summary-enabled"),provider=get<HTMLSelectElement>("history-summary-provider");
  const consent=get<HTMLInputElement>("history-summary-consent"),save=get<HTMLButtonElement>("history-summary-save");
  const status=get("history-summary-status"),list=get("history-summary-list");
  let latest:Status|undefined,busy=false,dirty=false,stopped=false,epoch=0,listKey="",saveError:string|undefined;
  const controls=()=>{save.disabled=busy||!latest||!dirty||(enabled.checked&&!consent.checked);};
  const changed=()=>{dirty=true;consent.checked=false;controls();};
  enabled.onchange=changed;provider.onchange=changed;consent.onchange=controls;
  const render=(value:Status)=>{
    latest=value;
    if(!dirty){enabled.checked=value.enabled;provider.value=value.provider;consent.checked=false;}
    status.textContent=saveError??(value.needs_approval?"History permissions changed. Approve future sharing again to resume content summaries.":
      !value.enabled?"Native content is not shared automatically.":
      !value.generation_enabled?"Automatic summaries are disabled in Memory settings.":
      value.generation_paused?"Memory generation is paused. Saved summaries remain available.":
      value.last_status?.state==="retry"?`Waiting to retry: ${value.last_status.message??"The provider did not finish."}`:
      `${value.summaries} content summaries. New activity is summarized automatically; updates are spaced at least one minute apart.`);
    controls();
  };
  const renderList=(items:Summary[])=>{
    const next=JSON.stringify(items);if(next===listKey)return;listKey=next;
    const opened=new Set([...list.querySelectorAll<HTMLDetailsElement>("details[open]")].map(node=>node.dataset.summaryId));
    list.replaceChildren();
    if(!items.length){list.textContent="No automatic content summaries yet.";return;}
    for(const item of items){
      const card=root.createElement("details"),title=root.createElement("summary"),description=root.createElement("p"),source=root.createElement("p"),body=root.createElement("pre");
      card.dataset.summaryId=item.id;card.open=opened.has(item.id);title.textContent=item.title;description.textContent=item.description;
      const period=item.from_ms!==undefined&&item.to_ms!==undefined?`${new Date(item.from_ms).toLocaleString()} to ${new Date(item.to_ms).toLocaleString()}`:new Date(item.written*1000).toLocaleString();
      title.setAttribute('aria-label',`${item.title}, activity ${period}`);
      source.className="muted";source.textContent=`${item.app} · ${item.source_events} observations · Activity ${period} · Observed content`;
      if(item.incomplete)source.textContent+=` · Incomplete (${item.omitted_events} omitted)`;
      body.textContent=item.text;body.className="history-summary-text";
      const reveal=root.createElement("button"),remove=root.createElement("button"),message=root.createElement("p");
      const key={id:item.id,sha256:item.sha256};
      reveal.type=remove.type="button";reveal.className=remove.className="ghost";
      reveal.textContent="Reveal summary file";remove.textContent="Delete this summary and its activity";message.setAttribute("role","status");
      reveal.onclick=async()=>{reveal.disabled=true;try{await invoke("history_summary_reveal",{summary:key});message.textContent="Summary file revealed.";}catch(error){message.textContent=String(error);}finally{reveal.disabled=false;}};
      remove.onclick=()=>root.dispatchEvent(new CustomEvent("icu-delete-summary",{detail:{...key,title:item.title}}));
      card.append(title,description,source,body,reveal,remove,message);list.append(card);
    }
  };
  const load=async()=>{
    if(busy||stopped)return;const mark=epoch;
    try{
      const [value,items]=await Promise.all([invoke<Status>("history_summary_status"),invoke<Summary[]>("history_summary_list")]);
      if(mark!==epoch||stopped)return;render(value);renderList(items);
    }catch(error){if(mark===epoch&&!stopped)status.textContent=saveError??String(error);}
  };
  save.onclick=async()=>{
    if(!latest||busy||save.disabled)return;const mark=epoch;busy=true;saveError=undefined;controls();
    try{
      const value=await invoke<Status>("history_summary_configure",{request:{enabled:enabled.checked,provider:provider.value,approved:consent.checked,
        history_generation:latest.history_generation,history_revision:latest.history_revision}});
      if(mark!==epoch||stopped)return;dirty=false;render(value);
    }catch(error){if(mark===epoch&&!stopped){saveError=String(error);status.textContent=saveError;}}
    finally{busy=false;controls();void load();}
  };
  const clear=()=>{epoch++;latest=undefined;dirty=false;saveError=undefined;consent.checked=false;enabled.checked=false;listKey="";list.replaceChildren();status.textContent="Activity summaries cleared.";controls();void load();};
  root.addEventListener("icu-history-cleared",clear);root.addEventListener("icu-memory-cleared",clear);
  void load();const timer=setInterval(()=>void load(),2000);
  return()=>{stopped=true;epoch++;clearInterval(timer);root.removeEventListener("icu-history-cleared",clear);root.removeEventListener("icu-memory-cleared",clear);};
}
