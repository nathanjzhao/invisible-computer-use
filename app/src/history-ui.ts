import { showUtilityPanel, toggleUtilityPanel } from "./panel-navigation";
type Invoke=<T>(command:string,args?:Record<string,unknown>)=>Promise<T>;
type Behavior="observe"|"do_not_observe";
type Rule={scope:"app";bundleID:string}|{scope:"url";urlDomain:string};
export type HistorySettings={observation:{defaultApplicationBehavior:Behavior;defaultURLBehavior:Behavior;allowlist:Rule[];blocklist:Rule[]}};
export type HistoryStatus={enabled:boolean;paused:boolean;capturing:boolean;capture_mode:string;semantic_supported?:boolean;interaction_supported?:boolean;helper_setup_supported?:boolean;helper_notice?:string|null;browser_capture?:boolean;browser_capture_supported?:boolean;browser_permission_required?:boolean;browser_capture_notice?:string;revision:number;generation:string;settings:HistorySettings;retention_days:number;collector:null|{error?:string|null;mode:string};recent_activity_files:string[]};
export function historyText(s:HistoryStatus):string{
  if(!s.enabled)return "Computer History is disabled.";
  if(s.paused)return "Computer History is paused.";
  if(s.collector?.error)return `Capture is unavailable: ${s.collector.error}`;
  return s.capturing?(s.capture_mode==="interaction_events"?"Recording allowed native interactions and window text. Browser content is excluded.":s.capture_mode==="values"?"Recording allowed focused controls and text values.":s.capture_mode==="semantic"?"Recording allowed focused control labels.":"Recording allowed application activity."):"History is enabled; waiting for the desktop collector.";
}
const lines=(value:string)=>[...new Set(value.split(/\r?\n/).map(s=>s.trim()).filter(Boolean))];
export function historySettings(values:{app:Behavior;url:Behavior;allowApps:string;blockApps:string;allowUrls:string;blockUrls:string}):HistorySettings{
  const rules=(apps:string,urls:string):Rule[]=>[...lines(apps).map(bundleID=>({scope:"app" as const,bundleID})),...lines(urls).map(urlDomain=>({scope:"url" as const,urlDomain}))];
  return {observation:{defaultApplicationBehavior:values.app,defaultURLBehavior:values.url,allowlist:rules(values.allowApps,values.allowUrls),blocklist:rules(values.blockApps,values.blockUrls)}};
}
export function wireHistory(root:Document,invoke:Invoke):()=>void{
  const get=<T extends HTMLElement>(id:string)=>root.getElementById(id) as T;
  const button=get<HTMLButtonElement>("history-button"),message=get("history-message"),status=get("history-status");
  const toggle=get<HTMLButtonElement>("history-toggle"),pause=get<HTMLButtonElement>("history-pause"),save=get<HTMLButtonElement>("history-save"),reload=get<HTMLButtonElement>("history-reload"),remove=get<HTMLButtonElement>("history-delete"),confirm=get<HTMLInputElement>("history-delete-confirm"),consent=get<HTMLInputElement>("history-consent"),days=get<HTMLInputElement>("history-retention");
  const app=get<HTMLSelectElement>("history-default-app"),url=get<HTMLSelectElement>("history-default-url"),allowApps=get<HTMLTextAreaElement>("history-allow-apps"),blockApps=get<HTMLTextAreaElement>("history-block-apps"),allowUrls=get<HTMLTextAreaElement>("history-allow-urls"),blockUrls=get<HTMLTextAreaElement>("history-block-urls");
  let latest:HistoryStatus|undefined,basis:HistoryStatus|undefined,busy=false,dirty=false,stopped=false;
  let filesKey: string | undefined;
  const helperSetup=get<HTMLButtonElement>("history-helper-setup");
  const scope=get<HTMLSelectElement>("history-scope"),scopeConsent=get<HTMLInputElement>("history-scope-consent"),scopeSave=get<HTMLButtonElement>("history-scope-save");
  let scopeBasis:HistoryStatus|undefined;
  const web=get<HTMLInputElement>("history-browser-enabled"),webConsent=get<HTMLInputElement>("history-browser-consent"),webSave=get<HTMLButtonElement>("history-browser-save"),webPermission=get<HTMLButtonElement>("history-browser-permission"),webApp=get<HTMLSelectElement>("history-browser-app");
  let webBasis:HistoryStatus|undefined;
  const scopeRank=(mode:string)=>["application_metadata","semantic","values","interaction_events"].indexOf(mode);
  const controls=()=>{
    toggle.disabled=busy||!latest||(!latest.enabled&&(!consent.checked||dirty||!days.validity.valid));
    pause.disabled=busy||!latest?.enabled||(latest.paused&&dirty);save.disabled=busy||!basis||!days.validity.valid;reload.disabled=busy;remove.disabled=busy||!confirm.checked;
    toggle.textContent=latest?.enabled?"Disable history":"Enable history";pause.textContent=latest?.paused?"Resume":"Pause";
    helperSetup.hidden=!latest?.helper_setup_supported;
    helperSetup.disabled=busy||dirty||!latest?.enabled||!latest?.helper_setup_supported;
    scope.disabled=busy||!latest?.semantic_supported;
    const interactions=Array.from(scope.options).find(option=>option.value==="interaction_events");
    if(interactions)interactions.disabled=!(latest?.interaction_supported??latest?.semantic_supported);
    scopeSave.disabled=busy||!scopeBasis||dirty||Boolean(scope.selectedOptions[0]?.disabled)||scope.value===latest?.capture_mode||(scopeRank(scope.value)>scopeRank(scopeBasis.capture_mode)&&!scopeConsent.checked);
    web.disabled=busy||!latest?.browser_capture_supported;
    webSave.disabled=busy||!webBasis||dirty||web.checked===Boolean(latest?.browser_capture)||(web.checked&&!webConsent.checked);
    webPermission.disabled=busy||!latest?.browser_capture||!latest?.browser_capture_supported||latest?.browser_permission_required===false;
  };
  const fill=(s:HistoryStatus)=>{basis=s;dirty=false;app.value=s.settings.observation.defaultApplicationBehavior;url.value=s.settings.observation.defaultURLBehavior;days.value=String(s.retention_days);
    const values=(rules:Rule[],scope:"app"|"url")=>rules.filter(r=>r.scope===scope).map(r=>r.scope==="app"?r.bundleID:r.urlDomain).join("\n");
    allowApps.value=values(s.settings.observation.allowlist,"app");blockApps.value=values(s.settings.observation.blocklist,"app");allowUrls.value=values(s.settings.observation.allowlist,"url");blockUrls.value=values(s.settings.observation.blocklist,"url");
  };
  const render=(s:HistoryStatus,reset=false)=>{latest=s;status.textContent=historyText(s);button.textContent=s.capturing?"● History":s.paused&&s.enabled?"History paused":"History";
    get("history-helper-notice").textContent=s.helper_notice??"";get("history-helper-notice").hidden=!s.helper_notice;
    if(reset||!scopeBasis){scope.value=s.capture_mode;scopeBasis=undefined;scopeConsent.checked=false;}
    if(reset||!webBasis){web.checked=Boolean(s.browser_capture);webBasis=undefined;webConsent.checked=false;}
    get("history-browser-status").textContent=!s.browser_capture_supported?"Browser capture is unavailable on this platform.":s.browser_capture?(s.browser_permission_required===false?"Browser capture is enabled, subject to accessibility and website rules.":"Browser capture is enabled, subject to macOS access and website rules."):"Browser capture is disabled.";
    get("history-browser-automation").hidden=s.browser_permission_required===false;
    get("history-browser-notice").textContent=s.browser_capture_notice??"";
    get("history-browser-notice").hidden=!s.browser_capture_notice;
    if(reset||!dirty)fill(s);const files=get<HTMLUListElement>("history-files");const nextFilesKey=JSON.stringify(s.recent_activity_files);if(nextFilesKey!==filesKey){filesKey=nextFilesKey;files.replaceChildren();for(const path of s.recent_activity_files){const li=root.createElement("li");const label=root.createElement("span");label.textContent=path;const review=root.createElement("button");review.type="button";review.className="ghost";review.textContent="Review for memory";review.onclick=()=>{const memory=root.getElementById("memory-pop");if(memory){showUtilityPanel(root,"memory-pop");root.getElementById("trajectory-tools")?.dispatchEvent(new CustomEvent("icu-review-history",{detail:{path}}));}};li.append(label,review);files.append(li);}}controls();
  };
  const action=async(fn:()=>Promise<HistoryStatus>,reset=false)=>{busy=true;controls();try{render(await fn(),reset);message.textContent="";}catch(error){message.textContent=String(error);}finally{busy=false;controls();}};
  button.onclick=()=>{toggleUtilityPanel(root,"history-panel");};
  for(const field of [app,url,allowApps,blockApps,allowUrls,blockUrls,days])field.oninput=()=>{dirty=true;consent.checked=false;message.textContent="Save or reload rules before enabling or resuming history.";controls();};
  consent.onchange=controls;confirm.onchange=controls;
  helperSetup.onclick=()=>void action(()=>invoke<HistoryStatus>("computer_history_setup_desktop",{approved:true,generation:latest!.generation,revision:latest!.revision}));
  scope.onchange=()=>{scopeBasis??=latest;scopeConsent.checked=false;controls();};scopeConsent.onchange=controls;
  scopeSave.onclick=()=>void action(()=>invoke<HistoryStatus>("computer_history_capture_mode",{mode:scope.value,approved:scopeConsent.checked,generation:scopeBasis!.generation,revision:scopeBasis!.revision}),true);
  web.onchange=()=>{webBasis??=latest;webConsent.checked=false;controls();};webConsent.onchange=controls;
  webSave.onclick=()=>void action(()=>invoke<HistoryStatus>("computer_history_browser_capture",{enabled:web.checked,approved:webConsent.checked,generation:webBasis!.generation,revision:webBasis!.revision}),true);
  webPermission.onclick=()=>void action(async()=>{await invoke<void>("computer_history_browser_permission",{application:webApp.value});return invoke<HistoryStatus>("computer_history_status");});
  toggle.onclick=()=>void action(async()=>{const s=await invoke<HistoryStatus>("computer_history_configure",{enabled:!latest!.enabled,retentionDays:latest!.retention_days,approved:consent.checked});consent.checked=false;return s;});
  pause.onclick=()=>void action(()=>invoke<HistoryStatus>("computer_history_pause",{paused:!latest!.paused}));
  reload.onclick=()=>void action(()=>invoke<HistoryStatus>("computer_history_status"),true);
  save.onclick=()=>void action(async()=>{
    const settings=historySettings({app:app.value as Behavior,url:url.value as Behavior,allowApps:allowApps.value,blockApps:blockApps.value,allowUrls:allowUrls.value,blockUrls:blockUrls.value});
    const saved=await invoke<HistoryStatus>("computer_history_settings_set",{settings,generation:basis!.generation,revision:basis!.revision});
    if(Number(days.value)!==saved.retention_days){return invoke<HistoryStatus>("computer_history_retention_set",{retentionDays:Number(days.value),generation:saved.generation,revision:saved.revision});}
    return saved;
  },true);
  remove.onclick=()=>void action(async()=>{await invoke<number>("computer_history_clear");confirm.checked=false;root.dispatchEvent(new CustomEvent("icu-history-cleared"));return invoke<HistoryStatus>("computer_history_status");});
  const poll=async()=>{if(busy||stopped)return;try{const s=await invoke<HistoryStatus>("computer_history_status");if(!stopped)render(s);}catch(e){if(!stopped)message.textContent=String(e);}};
  void poll();const timer=setInterval(()=>void poll(),2000);controls();return()=>{stopped=true;clearInterval(timer);};
}
