import { showUtilityPanel } from "./panel-navigation";

export type WorkflowPlan = {
  title:string; description:string; parameters:{key:string;label:string;description:string}[];
  steps:{description:string;evidence_lines:number[]}[]; verification:string;
};
export type Workflow = {id:string;app_name:string;app_key:string;plan:WorkflowPlan;source?:{format?:string}};
export type ListedWorkflow = {workflow:Workflow;sha256:string};
export type WorkflowInvocation = {id:string;expected_sha256:string;values:Record<string,string>;approved:boolean};
type Invoke = <T>(command:string,args?:Record<string,unknown>)=>Promise<T>;
type Generated = {request_id:string;proposal_sha256:string;plan:WorkflowPlan};
// A staged routine from a fleet offer arrives on this event (fleet-ui.ts) with the
// operator label so the consent line can name who offered it.
type StagedDetail = {generated:Generated;issuerLabel?:string};

export function wireWorkflows(root:HTMLElement,invoke:Invoke,onUse:(entry:ListedWorkflow,request:WorkflowInvocation)=>void) {
  const get=<T extends HTMLElement>(id:string)=>root.querySelector<T>(`#${id}`)!;
  const generate=get<HTMLButtonElement>("workflow-generate"),cancel=get<HTMLButtonElement>("workflow-cancel");
  const draft=get<HTMLElement>("workflow-draft"),title=get<HTMLInputElement>("workflow-draft-title"),description=get<HTMLTextAreaElement>("workflow-draft-description"),verification=get<HTMLTextAreaElement>("workflow-draft-verification");
  const draftParameters=get<HTMLElement>("workflow-draft-parameters"),draftSteps=get<HTMLOListElement>("workflow-draft-steps");
  const consent=get<HTMLInputElement>("workflow-save-consent"),save=get<HTMLButtonElement>("workflow-save");
  const message=get<HTMLElement>("workflow-message"),list=get<HTMLElement>("workflow-list");
  const inputPanel=get<HTMLElement>("workflow-inputs"),inputFields=get<HTMLElement>("workflow-input-fields");
  let generation=0,listGeneration=0,active:string|null=null,proposal:Generated|null=null,selected:ListedWorkflow|null=null;
  const stop=()=>{
    generation++;
    if(active)void invoke("workflow_cancel",{requestId:active}).catch(()=>{});
    active=null;proposal=null;draft.hidden=true;cancel.hidden=true;title.value="";description.value="";verification.value="";draftParameters.replaceChildren();draftSteps.replaceChildren();consent.checked=false;save.disabled=true;
  };
  const invalidate=()=>{stop();message.textContent="";};
  root.addEventListener("icu-workflow-invalidated",invalidate);
  const sourceDeleted=()=>{
    invalidate();listGeneration++;selected=null;inputPanel.hidden=true;inputFields.replaceChildren();
    get('workflow-selected-title').textContent='';list.replaceChildren();
    void refresh();
  };
  root.ownerDocument.addEventListener("icu-history-cleared",sourceDeleted);
  root.ownerDocument.addEventListener("icu-memory-cleared",sourceDeleted);
  cancel.onclick=()=>{stop();message.textContent="Workflow draft cancelled.";};
  draft.oninput=event=>{if(event.target===consent)return;consent.checked=false;save.disabled=true;};
  consent.onchange=()=>{save.disabled=!proposal||!consent.checked;};
  generate.onclick=()=>root.dispatchEvent(new CustomEvent("icu-workflow-request"));
  const DEFAULT_CONSENT="I reviewed this workflow and want to save it for reuse.";
  // Fill the existing draft form from a Generated proposal and reveal it. Shared by the
  // recording path (icu-workflow-source) and the fleet-offer path (icu-workflow-staged);
  // `consentLabel` lets a dispatched routine name who offered it.
  const renderDraft=(response:Generated,consentLabel:string)=>{
    proposal=response;title.value=response.plan.title;description.value=response.plan.description;verification.value=response.plan.verification;
    draftParameters.replaceChildren();draftSteps.replaceChildren();
    for(const parameter of response.plan.parameters){
      const row=document.createElement("p");row.textContent=`Input: ${parameter.label}. ${parameter.description}`;draftParameters.append(row);
    }
    for(const [index,step] of response.plan.steps.entries()){
      const row=document.createElement("li"),text=document.createElement("textarea"),source=document.createElement("small");
      text.value=step.description;text.rows=2;text.maxLength=800;text.setAttribute("aria-label",`Workflow step ${index+1}`);
      source.textContent=`Recorded observations: ${step.evidence_lines.join(", ")}`;row.append(text,source);draftSteps.append(row);
    }
    consent.checked=false;save.disabled=true;
    if(consent.nextSibling)consent.nextSibling.textContent=` ${consentLabel}`;
    draft.hidden=false;
    if(!root.closest<HTMLElement>("#memory-pop")?.hidden){draft.scrollIntoView?.({block:"start"});title.focus();}
  };
  root.addEventListener("icu-workflow-source",async event=>{
    stop();const epoch=generation;const requestId=`wf-${crypto.randomUUID()}`;active=requestId;cancel.hidden=false;
    message.textContent="Drafting a reusable workflow from the reviewed observations…";
    const recording=(event as CustomEvent<Record<string,unknown>>).detail;
    try{
      const response=await invoke<Generated>("workflow_generate",{request:{request_id:requestId,recording}});
      if(epoch!==generation)return;
      renderDraft(response,DEFAULT_CONSENT);
      message.textContent="Review the draft and its result check. Saving does not run it.";
    }catch(error){if(epoch===generation){stop();message.textContent=String(error);}}
  });
  // A routine staged from a fleet offer (fleet-ui.ts already called fleet_stage and has
  // a Generated draft in the Runtime). Show it in the same review form; the only run
  // trigger remains the user pressing Send after Save and Choose.
  root.addEventListener("icu-workflow-staged",event=>{
    const detail=(event as CustomEvent<StagedDetail>).detail;
    stop();active=detail.generated.request_id;cancel.hidden=false;
    if(root instanceof HTMLDetailsElement)root.open=true;
    const who=detail.issuerLabel?`from ${detail.issuerLabel} `:"";
    renderDraft(detail.generated,`I reviewed this routine ${who}and want to save it on this computer so I can run it.`);
    message.textContent="Review this routine from your organization. Saving does not run it.";
  });
  save.onclick=async()=>{
    if(!proposal||!consent.checked)return;
    const epoch=generation;save.disabled=true;cancel.disabled=true;consent.disabled=true;
    try{
      const plan:WorkflowPlan={...proposal.plan,title:title.value,description:description.value,verification:verification.value,
        steps:proposal.plan.steps.map((step,index)=>({...step,description:draftSteps.querySelectorAll<HTMLTextAreaElement>("textarea")[index].value}))};
      await invoke<ListedWorkflow>("workflow_save",{request:{request_id:proposal.request_id,expected_proposal_sha256:proposal.proposal_sha256,plan,approved:true}});
      if(epoch!==generation)return;
      active=null;stop();message.textContent="Workflow saved. Choose it below to supply new inputs.";await refresh();
    }catch(error){if(epoch===generation){message.textContent=String(error);consent.checked=false;}}
    finally{cancel.disabled=false;consent.disabled=false;}
  };
  const select=(entry:ListedWorkflow)=>{
    selected=entry;get("workflow-selected-title").textContent=`${entry.workflow.plan.title} in ${entry.workflow.app_name}`;
    inputFields.replaceChildren();
    for(const parameter of entry.workflow.plan.parameters){
      const label=document.createElement("label");label.textContent=parameter.label;
      const input=document.createElement("textarea");input.rows=2;input.maxLength=8000;input.dataset.inputKey=parameter.key;
      input.setAttribute("aria-label",`Workflow input: ${parameter.label}`);input.placeholder=parameter.description;
      label.append(input);inputFields.append(label);
    }
    inputPanel.hidden=false;
  };
  const refresh=async()=>{
    const epoch=++listGeneration;
    try{
      const entries=await invoke<ListedWorkflow[]>("workflow_list");
      if(epoch!==listGeneration)return;
      if(selected&&!entries.some(entry=>entry.workflow.id===selected!.workflow.id&&entry.sha256===selected!.sha256)){
        selected=null;inputPanel.hidden=true;inputFields.replaceChildren();get('workflow-selected-title').textContent='';
      }
      list.replaceChildren();
      for(const entry of entries){
        const row=document.createElement("div");row.className="mem-row";
        const label=document.createElement("span");label.textContent=`${entry.workflow.plan.title} (${entry.workflow.app_name})`;
        if(entry.workflow.source?.format==="fleet-dispatch/v1"){const badge=document.createElement("span");badge.className="muted fleet-badge";badge.textContent=" From your organization";label.append(badge);}
        const use=document.createElement("button");use.type="button";use.className="ghost";use.textContent=`Choose ${entry.workflow.plan.title}`;use.onclick=()=>select(entry);
        const remove=document.createElement("button");remove.type="button";remove.className="ghost";remove.textContent=`Delete ${entry.workflow.plan.title}`;
        remove.onclick=async()=>{try{await invoke("workflow_delete",{id:entry.workflow.id,expectedSha256:entry.sha256});await refresh();}catch(error){message.textContent=String(error);}};
        row.append(label,use,remove);list.append(row);
      }
      if(!entries.length)list.textContent="No saved workflows.";
    }catch(error){if(epoch===listGeneration)message.textContent=String(error);}
  };
  get<HTMLButtonElement>("workflow-refresh").onclick=()=>void refresh();
  get<HTMLButtonElement>("workflow-use").onclick=()=>{
    if(!selected)return;
    const values:Record<string,string>={};
    for(const field of inputFields.querySelectorAll<HTMLTextAreaElement>("textarea")){
      if(!field.value.trim()){message.textContent="Fill every workflow input before preparing the task.";field.focus();return;}
      values[field.dataset.inputKey!]=field.value;
    }
    onUse(selected,{id:selected.workflow.id,expected_sha256:selected.sha256,values,approved:true});
    showUtilityPanel(root.ownerDocument,null);
  };
  void refresh();
  return {invalidate,refresh};
}
