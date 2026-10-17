import { showUtilityPanel } from "./panel-navigation";
export type RecordingPreview = {
  source_format?:string;
  includes_values?:boolean;
  reviewed_bytes?:number|null;
  incomplete_observations?:number;
  capture_gap_events?: number;
  capture_loss_unknown?: boolean;
  recording_sha256: string;
  manifest_sha256: string;
  apps: Record<string, number>;
  steps: Record<string, { source_line: number; action: string }[]>;
  semantic_events: number;
  secure_events_omitted: number;
  has_values: boolean;
  memory_policy: string;
};
type Invoke = <T>(command: string, args?: Record<string, unknown>) => Promise<T>;

export function wireTrajectoryImport(root: HTMLElement, invoke: Invoke, refresh: () => Promise<void>) {
  const field = <T extends HTMLElement>(id: string) => root.querySelector<T>(`#${id}`)!;
  let reviewed: { path: string; preview: RecordingPreview } | null = null;
  const path = field<HTMLInputElement>("trajectory-path");
  const scope = field<HTMLSelectElement>("trajectory-app");
  const message = field<HTMLElement>("trajectory-message");
  const review = field<HTMLButtonElement>("trajectory-review");
  const save = field<HTMLButtonElement>("trajectory-import");
  const consent = field<HTMLInputElement>("trajectory-consent");
  const values = field<HTMLInputElement>("trajectory-values");
  const back = field<HTMLButtonElement>("trajectory-back");
  const workflow = field<HTMLButtonElement>("workflow-generate");
  back?.addEventListener("click",()=>showUtilityPanel(root.ownerDocument,"history-panel"));
  let busy = false;
  let page = 0;
  let reviewEpoch = 0;
  let displayedScope: string | null = null;
  let approval: string | null = null;
  let pageRows: {source_line:number;action:string}[] | null = null;
  const prev = field<HTMLButtonElement>("trajectory-prev");
  const next = field<HTMLButtonElement>("trajectory-next");
  const fingerprint = () => JSON.stringify([reviewed?.path, reviewed?.preview.recording_sha256,reviewed?.preview.manifest_sha256,
    reviewed?.preview.reviewed_bytes, path.value.trim(), scope.value, Boolean(values?.checked),
    ...["trajectory-project","trajectory-goal","trajectory-provider","trajectory-actor"].map(id=>field<HTMLInputElement|HTMLSelectElement>(id).value)]);
  const resetConsent = () => { approval=null;consent.checked = false; save.disabled = true; if(workflow)workflow.disabled=true; root.dispatchEvent(new CustomEvent("icu-workflow-invalidated")); };
  const invalidate = () => { reviewEpoch++; reviewed = null; page=0; pageRows=null; resetConsent(); field<HTMLElement>("trajectory-options").hidden = true; field<HTMLOListElement>("trajectory-preview-steps").replaceChildren(); field<HTMLElement>("trajectory-preview-count").textContent = ""; };
  root.ownerDocument.addEventListener("icu-history-cleared", () => {
    invalidate();
    message.textContent = "History cleared. Review the recording again before sharing it.";
  });
  const renderSteps = () => {
    if (displayedScope !== scope.value) {
      // Native WebKit pickers can change selectedness without a DOM change
      // notification. Never show or approve rows from the previous app.
      displayedScope=scope.value;page=0;pageRows=null;resetConsent();
    }
    const rows = pageRows ?? reviewed?.preview.steps[scope.value] ?? [];
    const total = reviewed?.preview.apps[scope.value] ?? 0;
    field<HTMLOListElement>("trajectory-preview-steps").replaceChildren(...rows.map(row => {
      const li = document.createElement("li");
      li.textContent = `Line ${row.source_line}: ${row.action}`;
      return li;
    }));
    field<HTMLElement>("trajectory-preview-count").textContent = `${total} eligible events will be imported in ${Math.ceil(total / 40)} chunk(s). Showing chunk ${page + 1}. Review these observations before sharing them with the selected model. Previously imported history observations will be skipped.`;
    if(prev) prev.disabled=busy||page===0;
    if(next) next.disabled=busy||page+1>=Math.ceil(total/40);
  };
  const loadPage = async (index:number) => {
    if(busy||!reviewed)return;
    const original=reviewed;const app=scope.value;busy=true;consent.disabled=true;resetConsent();renderSteps();
    try{
      const result=await invoke<{steps:{source_line:number;action:string}[]}>("trajectory_preview_page",{request:{directory:original.path,app,page:index,expected_sha256:original.preview.recording_sha256,expected_manifest_sha256:original.preview.manifest_sha256,include_values:Boolean(original.preview.includes_values),reviewed_bytes:original.preview.reviewed_bytes??null}});
      if(reviewed!==original||scope.value!==app)return;
      page=index;pageRows=result.steps;
    }catch(error){invalidate();message.textContent=String(error);}
    finally{busy=false;consent.disabled=false;resetConsent();renderSteps();}
  };
  prev?.addEventListener("click",()=>void loadPage(page-1));
  next?.addEventListener("click",()=>void loadPage(page+1));
  path.addEventListener("input", invalidate);
  values?.addEventListener("change",invalidate);
  const scopeChanged=()=>{page=0;pageRows=null;resetConsent();renderSteps();};
  scope.addEventListener("change",scopeChanged);
  scope.addEventListener("input",scopeChanged);
  scope.addEventListener("blur",()=>{if(displayedScope!==scope.value)renderSteps();});
  for (const id of ["trajectory-project", "trajectory-goal", "trajectory-provider", "trajectory-actor"]) {
    field(id).addEventListener("input", resetConsent);
    field(id).addEventListener("change", resetConsent);
  }
  consent.addEventListener("change", () => {
    if(displayedScope!==scope.value){renderSteps();message.textContent="The selected app changed. Review its observations before approving them.";return;}
    if(!reviewed||reviewed.path!==path.value.trim()||Boolean(reviewed.preview.includes_values)!==Boolean(values?.checked)){
      invalidate();message.textContent="The recording or value scope changed. Review it again before sharing.";return;
    }
    approval=consent.checked?fingerprint():null;
    save.disabled = busy || !reviewed || !consent.checked; if(workflow)workflow.disabled=save.disabled;
    if(!consent.checked)root.dispatchEvent(new CustomEvent("icu-workflow-invalidated"));
  });
  const stillApproved=()=>{
    if(reviewed&&consent.checked&&approval===fingerprint()&&displayedScope===scope.value)return true;
    resetConsent();renderSteps();message.textContent="The reviewed app, source or sharing choices changed. Review and approve them again.";return false;
  };
  review.addEventListener("click", async () => {
    if (busy) return;
    invalidate(); busy = true; review.disabled = true;
    const epoch = reviewEpoch;
    const selectedPath = path.value.trim();
    const includeValues=Boolean(values?.checked);
    try {
      const preview = await invoke<RecordingPreview>("trajectory_preview", { directory: selectedPath,includeValues });
      if (epoch !== reviewEpoch || path.value.trim() !== selectedPath||Boolean(values?.checked)!==includeValues) return;
      scope.replaceChildren(...Object.entries(preview.apps).map(([app, count]) => new Option(`${app} (${count} events)`, app)));
      reviewed = { path: selectedPath, preview };page=0;pageRows=null;
      renderSteps();
      message.textContent = `${preview.semantic_events} eligible observations across this source. ${preview.secure_events_omitted} protected observations excluded. ${preview.incomplete_observations??0} observations may contain partial text. ${preview.capture_gap_events ?? 0} capture gaps reported.${preview.capture_loss_unknown ? " The number of missed actions is unknown." : ""} ${preview.memory_policy}`;
      field<HTMLElement>("trajectory-options").hidden = false;
    } catch (error) { if (epoch === reviewEpoch) message.textContent = String(error); }
    finally { busy = false; review.disabled = false; renderSteps(); }
  });
  const request = () => {
    if(!reviewed)throw new Error("Review a recording first");
    return {
      directory: reviewed.path, expected_sha256: reviewed.preview.recording_sha256,
      expected_manifest_sha256: reviewed.preview.manifest_sha256,
      app: scope.value, project: field<HTMLInputElement>("trajectory-project").value.trim(),
      consent_id: `review-${crypto.randomUUID()}`, actor: field<HTMLSelectElement>("trajectory-actor").value,
      goal: field<HTMLInputElement>("trajectory-goal").value.trim(),
      provider: field<HTMLSelectElement>("trajectory-provider").value, approved: true,
      include_values:Boolean(reviewed.preview.includes_values),reviewed_bytes:reviewed.preview.reviewed_bytes??null,
    };
  };
  root.addEventListener("icu-workflow-request",()=>{
    if(busy||!reviewed||!consent.checked||!stillApproved())return;
    root.dispatchEvent(new CustomEvent("icu-workflow-source",{detail:request()}));
  });
  save.addEventListener("click", async () => {
    if (busy || !reviewed || !consent.checked || !stillApproved()) return;
    const selectedRequest=request();
    busy = true; save.disabled = true; review.disabled = true;
    message.textContent = "Importing reviewed events and checking for secrets…";
    try {
      const result=await invoke<{chunks?:number;skipped_previously_imported?:number}>("trajectory_import", { request:selectedRequest });
      message.textContent = `Imported ${result.chunks ?? 1} chunk(s). ${result.skipped_previously_imported??0} observations already in memory were skipped. Reviewed observations will be summarized in bounded idle passes for later tasks in this app.`;
      invalidate(); await refresh();
    } catch (error) { message.textContent = String(error); }
    finally { busy = false; review.disabled = false; save.disabled = !reviewed || !consent.checked; }
  });
  root.addEventListener("icu-review-history",event=>{
    const selected=(event as CustomEvent<{path?:string}>).detail?.path;
    if(busy||!selected)return;
    path.value=selected;if(values)values.checked=false;invalidate();
    root.hidden=false;
    showUtilityPanel(root.ownerDocument,"memory-pop");
    if(back)back.hidden=false;
    const disclosure=root.matches("#trajectory-tools")?root as HTMLDetailsElement:root.querySelector<HTMLDetailsElement>("#trajectory-tools");if(disclosure)disclosure.open=true;
    review.click();
  });
  // Poll only selectedness, never the source or a model. This covers native
  // accessibility selection routes that omit DOM input/change notifications.
  const timer=setInterval(()=>{
    if(!root.isConnected){clearInterval(timer);return;}
    if(!busy&&reviewed&&displayedScope!==scope.value)renderSteps();
  },200);
  return()=>clearInterval(timer);
}
