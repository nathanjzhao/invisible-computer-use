type WindowIdentity = {pid:number;window_id:number;app_name:string;title?:string;bounds:{x:number;y:number}};

/** Same-title windows need a distinction the user can match to the desktop. */
export function windowLabel(window:WindowIdentity,windows:WindowIdentity[]):string {
  const title=window.title||"untitled";
  const peers=windows.filter(w=>w.pid===window.pid&&w.app_name===window.app_name&&(w.title||"untitled")===title);
  if(peers.length<2)return title;
  const sorted=[...peers].sort((a,b)=>a.bounds.x-b.bounds.x||a.bounds.y-b.bounds.y||a.window_id-b.window_id);
  const index=sorted.findIndex(w=>w.window_id===window.window_id);
  if(peers.length===2){
    if(sorted[0].bounds.x!==sorted[1].bounds.x)return `${index===0?"Left":"Right"} window · ${title}`;
    if(sorted[0].bounds.y!==sorted[1].bounds.y)return `${index===0?"Upper":"Lower"} window · ${title}`;
  }
  return `Window ${index+1} of ${peers.length} · ${title}`;
}
