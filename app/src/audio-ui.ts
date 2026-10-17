import { toggleUtilityPanel } from "./panel-navigation";
type Invoke = <T>(command: string, args?: Record<string, unknown>) => Promise<T>;
export type AudioStatus = {state:string;microphone?:boolean;system?:boolean;directory?:string;error?:string;unavailable_sources?:string[]};
export function audioStatusText(s:AudioStatus):string {
  if(s.state==="recording")return `Recording ${[s.microphone?"microphone":"",s.system?"system sound":""].filter(Boolean).join(" and ")}.`;
  if(s.state==="stopping")return "Stopping and saving audio…";
  if(s.state==="saved")return `Saved audio in ${s.directory}.`;
  if(s.state==="partial")return `Saved available audio in ${s.directory}. Unavailable: ${(s.unavailable_sources??[]).join(", ")}.`;
  if(s.state==="unavailable")return `No audio captured. Unavailable: ${(s.unavailable_sources??[]).join(", ")}.`;
  if(s.state==="failed")return s.error??"Audio recording failed.";
  return "No recording active.";
}
export function wireAudio(root:Document,invoke:Invoke):void {
  const get=<T extends HTMLElement>(id:string)=>root.getElementById(id) as T;
  const button=get<HTMLButtonElement>("audio-button"),mic=get<HTMLInputElement>("audio-microphone"),system=get<HTMLInputElement>("audio-system"),consent=get<HTMLInputElement>("audio-consent"),seconds=get<HTMLInputElement>("audio-seconds"),start=get<HTMLButtonElement>("audio-start"),stop=get<HTMLButtonElement>("audio-stop"),exportWav=get<HTMLButtonElement>("audio-export"),status=get("audio-status");
  let active=false,busy=false,exportable=false,lastStatus="";
  const controls=()=>{mic.disabled=system.disabled=seconds.disabled=consent.disabled=active||busy;start.disabled=active||busy||!consent.checked||(!mic.checked&&!system.checked)||!seconds.validity.valid;stop.disabled=!active||busy;exportWav.disabled=active||busy||!exportable;};
  const render=(s:AudioStatus)=>{active=s.state==="recording"||s.state==="stopping";exportable=s.state==="saved"||s.state==="partial";button.textContent=active?"● Recording audio":"Audio";status.textContent=audioStatusText(s);controls();};
  button.onclick=()=>{toggleUtilityPanel(root,"audio-panel");};
  for(const input of [mic,system,seconds])input.oninput=()=>{consent.checked=false;controls();};
  consent.onchange=controls;
  start.onclick=async()=>{busy=true;controls();try{render(await invoke<AudioStatus>("audio_start",{request:{microphone:mic.checked,system:system.checked,seconds:Number(seconds.value),consent:consent.checked}}));}catch(e){status.textContent=String(e);}finally{consent.checked=false;busy=false;controls();}};
  stop.onclick=async()=>{busy=true;controls();try{render(await invoke<AudioStatus>("audio_stop"));}catch(e){status.textContent=String(e);}finally{busy=false;controls();}};
  exportWav.onclick=async()=>{busy=true;controls();status.textContent="Exporting WAV files…";try{const result=await invoke<{directory:string;files:unknown[]}>("audio_export_wav");status.textContent=`Saved ${result.files.length} WAV ${result.files.length===1?"file":"files"} in ${result.directory}.`;}catch(e){status.textContent=String(e);}finally{busy=false;controls();}};
  const poll=async()=>{if(busy)return;try{const value=await invoke<AudioStatus>("audio_status");const encoded=JSON.stringify(value);if(encoded!==lastStatus){lastStatus=encoded;render(value);}}catch(e){status.textContent=String(e);}};
  void poll();setInterval(()=>void poll(),1000);controls();
}
