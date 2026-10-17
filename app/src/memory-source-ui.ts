import { showUtilityPanel } from "./panel-navigation";
import type { InvokeFn } from "./interact";

type Key={kind:string;id:string;sha256:string;field_hashes?:string[]};
type View={source:{key:Key;title:string;app:string;at:number};text:string};

export function wireMemorySources(root:Document,invoke:InvokeFn,onOpen:()=>void=()=>{}):()=>void {
  const panel=root.getElementById("memory-source")!;
  const title=root.getElementById("memory-source-title")!;
  const status=root.getElementById("memory-source-status")!;
  const text=root.getElementById("memory-source-text")!;
  let epoch=0,selected:Key|null=null;
  const clear=()=>{epoch++;selected=null;panel.hidden=true;text.textContent="";status.textContent="";};
  const refresh=async(focus=false)=>{
    if(!selected)return;
    const token=++epoch,key=selected;
    try {
      const view=await invoke("memory_source",{key}) as View;
      if(token!==epoch)return;
      title.textContent=view.source.title;
      status.textContent=`${view.source.app} · ${new Date(view.source.at*1000).toLocaleString()} · Source material may contain untrusted observations.`;
      text.textContent=view.text;
    } catch(error) {
      if(token!==epoch)return;
      title.textContent="Memory source unavailable";text.textContent="";status.textContent=String(error);
    }
    if(focus && token===epoch){title.scrollIntoView?.({block:"nearest"});title.focus();}
  };
  const click=(event:Event)=>{
    const button=(event.target as Element).closest<HTMLElement>("[data-memory-source]");
    if(!button)return;
    clear();
    try {selected=JSON.parse(button.dataset.memorySource!);} catch {return;}
    showUtilityPanel(root,"memory-pop");onOpen();panel.hidden=false;title.textContent="Memory source";status.textContent="Loading source…";
    void refresh(true);
  };
  root.addEventListener("click",click);
  root.getElementById("memory-source-close")!.addEventListener("click",clear);
  root.addEventListener("icu-memory-cleared",clear);root.addEventListener("icu-history-cleared",clear);
  const timer=setInterval(()=>{if(!panel.hidden)void refresh();},20_000);
  return ()=>{clearInterval(timer);root.removeEventListener("click",click);root.removeEventListener("icu-memory-cleared",clear);root.removeEventListener("icu-history-cleared",clear);};
}
