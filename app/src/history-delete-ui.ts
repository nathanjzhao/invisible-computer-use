type Invoke = <T>(command:string,args?:Record<string,unknown>)=>Promise<T>;
type EventIdentity = {id:string;application:string;timestamp_unix_ms:number};
type Preview = {generation:string;revision:number;events:EventIdentity[];summaries:{id:string;sha256:string}[];derivatives:{kind:string;id:string;sha256:string}[]};
type SummaryKey = {id:string;sha256:string;title:string};

export function wireHistoryDeletion(root:Document,invoke:Invoke):()=>void {
  const get=<T extends HTMLElement>(id:string)=>root.getElementById(id) as T;
  const range=get<HTMLSelectElement>("history-delete-range"),app=get<HTMLInputElement>("history-delete-app");
  const start=get<HTMLButtonElement>("history-delete-preview"),review=get("history-delete-review");
  const approve=get<HTMLInputElement>("history-delete-selection-confirm"),apply=get<HTMLButtonElement>("history-delete-selection-apply");
  const cancel=get<HTMLButtonElement>("history-delete-selection-cancel"),status=get("history-delete-selection-status"),description=get("history-delete-selection-description");
  const filterBox=get("history-delete-filter");
  let preview:Preview|undefined,busy=false,applying=false,epoch=0,stopped=false;
  let reviewedFilter:string|null=null,summaryMode=false;
  const filterFingerprint=()=>JSON.stringify([range.value,app.value.trim()]);
  const controls=()=>{start.disabled=busy;apply.disabled=busy||!preview||(!preview.events.length&&!preview.summaries.length&&!preview.derivatives.length)||!approve.checked;range.disabled=busy;app.disabled=busy;approve.disabled=busy;cancel.disabled=applying;};
  const reset=()=>{epoch++;if(!applying)busy=false;preview=undefined;reviewedFilter=null;summaryMode=false;filterBox.hidden=false;approve.checked=false;review.hidden=true;description.textContent="";controls();};
  const changedFilter=()=>{
    if(preview&&!summaryMode&&reviewedFilter!==filterFingerprint()){
      reset();status.textContent="The selected time range or app changed. Review the deletion again.";return true;
    }
    return false;
  };
  range.onchange=reset;range.oninput=reset;app.oninput=reset;app.onchange=reset;
  approve.onchange=()=>{if(!changedFilter())controls();};
  cancel.onclick=()=>{if(applying)return;reset();status.textContent="Deletion cancelled. No activity was removed.";};
  const prepare=async(selected?:SummaryKey)=>{
    if(applying)return;reset();const mark=epoch,filterMark=filterFingerprint();summaryMode=Boolean(selected);filterBox.hidden=summaryMode;
    busy=true;controls();status.textContent="Reviewing selected activity…";
    try {
      const end=Date.now(),minutes=Number(range.value);
      const filter={application:app.value.trim()||null,from_ms:range.value==="all"?null:end-minutes*60_000,to_ms:range.value==="all"?null:end,event_ids:[]};
      const value=await invoke<Preview>("computer_history_delete_preview",{filter,summary:selected?{id:selected.id,sha256:selected.sha256}:null});
      if(stopped||mark!==epoch)return;
      if(!selected&&filterMark!==filterFingerprint()){reset();status.textContent="The selection changed while loading. Review the deletion again.";return;}
      if(!value.events.length&&!value.summaries.length&&!value.derivatives.length){reset();status.textContent="No recorded observations or memories match this selection.";return;}
      preview=value;reviewedFilter=filterMark;review.hidden=false;
      const times=value.events.map(e=>e.timestamp_unix_ms),apps=new Set(value.events.map(e=>e.application));
      // Iterate rather than spread a large reviewed selection into Math.min.
      const first=times.reduce((a,b)=>Math.min(a,b),Infinity),last=times.reduce((a,b)=>Math.max(a,b),0);
      description.textContent=(selected?`Summary “${selected.title}” and summaries derived from it. `:`${value.summaries.length} overlapping automatic summaries. `)+
        `${value.derivatives.length} imported recordings, memories or saved workflows. `+
        (times.length?`${value.events.length} observations from ${apps.size} apps, ${new Date(first).toLocaleString()} to ${new Date(last).toLocaleString()}. Later activity is excluded from this reviewed selection.`:
        "The original raw observations have already expired. The saved summary and linked memories will still be deleted.");
      status.textContent="Review the selection before confirming.";
    }catch(error){if(!stopped&&mark===epoch){reset();status.textContent=String(error);}}
    finally{if(mark===epoch){busy=false;controls();}}
  };
  start.onclick=()=>void prepare();
  const summarySelected=(event:Event)=>{
    const key=(event as CustomEvent<SummaryKey>).detail;
    if(!key||typeof key.id!=="string"||typeof key.sha256!=="string"||typeof key.title!=="string")return;
    if(applying){status.textContent="Deletion is already in progress. Wait for it to finish.";return;}
    get<HTMLDetailsElement>("history-selection").open=true;
    void prepare(key);
  };
  root.addEventListener("icu-delete-summary",summarySelected);
  apply.onclick=async()=>{
    if(changedFilter())return;
    if(busy||apply.disabled||!preview)return;const selected=preview,mark=epoch;busy=true;applying=true;controls();
    try {
      const count=await invoke<number>("computer_history_delete_reviewed",{preview:selected,approved:true});
      if(stopped||mark!==epoch)return;
      reset();root.dispatchEvent(new CustomEvent("icu-history-cleared"));
      status.textContent=`Deleted ${count} observations and their derived memories. Capture is paused.`;
    }catch(error){if(!stopped&&mark===epoch){reset();status.textContent=`${String(error)} Review the selection again before retrying.`;}}
    finally{busy=false;applying=false;controls();}
  };
  const cleared=()=>reset();root.addEventListener("icu-history-cleared",cleared);
  const timer=setInterval(()=>{if(!app.isConnected){clearInterval(timer);return;}if(!busy)changedFilter();},200);
  controls();return()=>{stopped=true;epoch++;clearInterval(timer);root.removeEventListener("icu-history-cleared",cleared);root.removeEventListener("icu-delete-summary",summarySelected);};
}
