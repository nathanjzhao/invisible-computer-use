export type SavedTask={id:string;goal:string;app_name:string;decisions:number;redacted:boolean;omitted_history?:number};
export function renderSavedTasks(list:HTMLElement,rows:SavedTask[],load:(row:SavedTask)=>void,discard:(id:string)=>Promise<void>,error:(message:string)=>void){
 list.replaceChildren();
 if(!rows.length){list.textContent="No saved task progress.";return;}
 for(const row of rows){
  const entry=document.createElement("div"),text=document.createElement("p"),open=document.createElement("button"),remove=document.createElement("button");
  text.textContent=`${row.app_name}: ${row.goal} (${row.decisions} decisions)${row.redacted?". Some text was redacted; review the goal.":""}${row.omitted_history?` ${row.omitted_history} older observations omitted.`:""}`;
  open.className=remove.className="ghost";open.type=remove.type="button";open.textContent="Load saved task";remove.textContent="Discard progress";
  open.onclick=()=>load(row);
  remove.onclick=async()=>{remove.disabled=true;try{await discard(row.id);entry.remove();if(!list.children.length)list.textContent="No saved task progress.";}catch(e){error(String(e));}finally{remove.disabled=false;}};
  entry.append(text,open,remove);list.append(entry);
 }
}
