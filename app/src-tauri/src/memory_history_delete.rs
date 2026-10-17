//! Reviewed derivative identities outlive expired raw History files.
use super::MemoryService;
use crate::trajectory::Provenance;
use computer_history::store::DeleteFilter;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all="snake_case")]
pub enum DerivativeKind { Run, Session, Workflow }
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReviewedDerivative { pub kind:DerivativeKind, pub id:String, pub sha256:String }
fn digest<T: Serialize>(value:&T)->Result<String,String>{
    Ok(format!("{:x}",Sha256::digest(serde_json::to_vec(value).map_err(|e|e.to_string())?)))
}
fn selected(p:&Provenance,filter:Option<&DeleteFilter>)->Result<bool,String>{
    if p.format!="computer-history-v1" {return Ok(false)}
    let Some(f)=filter else{return Ok(true)};
    if f.application.as_ref().is_some_and(|app|app!=&p.app){return Ok(false)}
    if !f.event_ids.is_empty(){return Ok(p.source_event_hashes.keys().any(|id|f.event_ids.contains(id)))}
    if f.from_ms.is_none() && f.to_ms.is_none(){return Ok(true)}
    let (first,last)=p.first_unix_ms.zip(p.last_unix_ms)
        .ok_or("An older imported recording lacks source timestamps. Select all recorded times to include it.")?;
    Ok(f.from_ms.is_none_or(|from|last>=from) && f.to_ms.is_none_or(|to|first<to))
}
impl MemoryService {
    pub(super) fn cleanup_history_temporaries_locked(&self)->Result<(),String>{
        'directories: for (directory,kind) in [(self.root.clone(),"root"),(self.runs_dir(),"runs"),(self.summaries_dir(),"summaries"),
            (self.root.join("workflows"),"workflows"),(self.skysight_root().join("content"),"content")] {
            let relative=directory.strip_prefix(&self.root).map_err(|_|"Unconfined memory directory")?;
            let mut paths=vec![self.root.clone()];let mut current=self.root.clone();
            for part in relative.components(){current.push(part);paths.push(current.clone());}
            for path in paths {
                let metadata=match std::fs::symlink_metadata(path){Ok(m)=>m,Err(e) if e.kind()==std::io::ErrorKind::NotFound=>continue 'directories,Err(e)=>return Err(e.to_string())};
                if !metadata.is_dir()||metadata.file_type().is_symlink(){return Err("History cleanup refuses linked memory directories".into())}
            }
            for entry in std::fs::read_dir(&directory).map_err(|e|e.to_string())? {
                let entry=entry.map_err(|e|e.to_string())?;let name=entry.file_name();let name=name.to_string_lossy();
                let Some(stem)=name.strip_suffix(".tmp") else{continue};
                let file_type=entry.file_type().map_err(|e|e.to_string())?;
                if kind=="runs"&&stem.strip_prefix(".batch-").is_some_and(|id|(id.starts_with("demo-")||id.starts_with("task-"))&&super::safe_id(id)) {
                    if file_type.is_dir(){std::fs::remove_dir_all(entry.path()).map_err(|e|e.to_string())?;}
                    else if file_type.is_symlink(){std::fs::remove_file(entry.path()).map_err(|e|e.to_string())?;}
                    continue;
                }
                let owned=match kind {
                    "root"=>matches!(stem,"settings"|"durable"|"MEMORY"|"history-summary-policy"),
                    "runs"=>(stem.starts_with("run-")||stem.starts_with("demo-")||stem.starts_with("task-"))&&super::safe_id(stem),
                    "summaries"=>stem.starts_with("s-")&&super::safe_id(stem),
                    "workflows"=>stem.starts_with("wf-")&&super::safe_id(stem),
                    "content"=>(stem.len()==64&&stem.bytes().all(|b|b.is_ascii_hexdigit()))||stem=="progress",
                    _=>false,
                };
                if owned&&(file_type.is_file()||file_type.is_symlink()){std::fs::remove_file(entry.path()).map_err(|e|e.to_string())?;}
            }
        }
        Ok(())
    }
    fn history_derivatives_locked(&self,filter:Option<&DeleteFilter>)->Result<Vec<ReviewedDerivative>,String>{
        let mut items=Vec::new();
        for (_,run) in self.run_files(){
            if run.trajectory.as_ref().map(|p|selected(p,filter)).transpose()?.unwrap_or(false){
                items.push(ReviewedDerivative{kind:DerivativeKind::Run,id:run.run_id.clone(),sha256:digest(&run)?});
            }
        }
        for (id,summary) in self.summaries(){
            let mut hit=false;
            for p in &summary.recordings {hit|=selected(p,filter)?;}
            if hit {items.push(ReviewedDerivative{kind:DerivativeKind::Session,id,sha256:digest(&summary)?});}
        }
        for item in self.workflow_list_locked(){
            if selected(&item.workflow.source,filter)? {
                items.push(ReviewedDerivative{kind:DerivativeKind::Workflow,id:item.workflow.id,sha256:item.sha256});
            }
        }
        Ok(items)
    }
    pub fn history_derivative_selection(&self,filter:&DeleteFilter)->Result<Vec<ReviewedDerivative>,String>{
        let _guard=self.store()?;let items=self.history_derivatives_locked(Some(filter))?;
        if items.len()>5000{return Err("Select a shorter time range or one app before deleting History memories".into())}
        Ok(items)
    }
    pub(super) fn validate_history_derivatives_locked(&self,reviewed:&[ReviewedDerivative])->Result<BTreeSet<(DerivativeKind,String)>,String>{
        if reviewed.len()>5000{return Err("Too many reviewed History memories".into())}
        if reviewed.is_empty(){return Ok(BTreeSet::new())}
        let mut current=BTreeMap::new();
        for item in self.history_derivatives_locked(None)? {
            if current.insert((item.kind,item.id),item.sha256).is_some(){return Err("History memory identities are ambiguous".into())}
        }
        let mut selected=BTreeSet::new();
        for item in reviewed {
            if !super::safe_id(&item.id) {return Err("Invalid reviewed History memory identifier".into())}
            let key=(item.kind,item.id.clone());
            if current.get(&key)!=Some(&item.sha256) || !selected.insert(key) {
                return Err("A reviewed History memory changed or was removed. Review the deletion again.".into());
            }
        }
        Ok(selected)
    }
}
