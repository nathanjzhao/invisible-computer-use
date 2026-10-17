//! Reviewable citations for the exact sources given to a task.
use super::*;
use sha2::Digest;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SourceKey {
    pub kind: String,
    pub id: String,
    pub sha256: String,
    #[serde(default,skip_serializing_if="Vec::is_empty")]
    pub field_hashes: Vec<String>,
}

#[derive(Clone, Debug, Serialize)]
pub struct SourceLink {
    pub key: SourceKey,
    pub title: String,
    pub app: String,
    pub at: i64,
}

#[derive(Clone, Debug, Serialize)]
pub struct SourceView {
    pub source: SourceLink,
    pub text: String,
}

impl MemoryService {
    fn source_view(&self, kind: &str, id: &str, fields: Option<&BTreeSet<String>>) -> Option<SourceView> {
        // A stored citation never grants permission to follow a substituted
        // directory outside the selected memory store.
        let directories: &[&str]=match kind {"history"=>&["computer-history","skysight","skysight/content"],"session"=>&["summaries"],_=>return None};
        if directories.iter().any(|name|std::fs::symlink_metadata(self.root.join(name)).ok()
            .is_none_or(|m|!m.is_dir() || m.file_type().is_symlink())) {return None;}
        match kind {
            "history" => {
                if id.len()!=69 || !id.ends_with(".json") || !id[..64].bytes().all(|b|b.is_ascii_hexdigit()) {return None;}
                if !self.root.join("computer-history/settings.json").is_file() {return None;}
                let history=computer_history::store::Store::open(self.root.join("computer-history")).ok()?;
                let view=self.history_summary_list(&history).ok()?.into_iter().find(|v|v.id==id)?;
                let observations=match fields {
                    Some(fields)=>self.history_selected_source_entries(&history,&view.app,id,fields),
                    None=>self.history_source_entries(&history,&view.app,"recall source",Some(id)),
                };
                let field_hashes=if fields.is_some() {observations.iter().map(|entry|format!("{:x}",sha2::Sha256::digest(entry.text.as_bytes()))).collect::<BTreeSet<_>>().into_iter().collect()} else {vec![]};
                let mut text=view.text;
                if observations.is_empty() {
                    text.push_str("\n\nOriginal field observations are unavailable. The generated summary is retained separately.");
                } else {
                    text.push_str("\n\nRetained observed fields\n");
                    for observation in observations {text.push_str(&format!("\n{}",observation.text));}
                }
                Some(SourceView {source:SourceLink {key:SourceKey {kind:kind.into(),id:id.into(),sha256:view.sha256,field_hashes},
                    title:view.title,app:view.app,at:(view.from_ms/1000) as i64},text})
            },
            "session" => {
                if !safe_id(id) || !id.starts_with("s-") {return None;}
                let path=self.summaries_dir().join(format!("{id}.json"));
                if std::fs::symlink_metadata(&path).ok()?.len()>512*1024 {return None;}
                let raw=skysight::private::read_bounded(&path,512*1024)?;
                let summary:ChatSummary=serde_json::from_str(&raw).ok()?;
                if summary.id!=id {return None;}
                let sha256=format!("{:x}",sha2::Sha256::digest(serde_json::to_vec(&summary).ok()?));
                let mut text=format!("{}\n\n{}\n\n{}",summary.title,summary.asked,summary.outcome);
                for note in &summary.notes {text.push_str(&format!("\n\n{}",note.text));}
                for origin in &summary.recordings {text.push_str(&format!("\n\nRecording project: {}",origin.project));}
                Some(SourceView {source:SourceLink {key:SourceKey {kind:kind.into(),id:id.into(),sha256,field_hashes:vec![]},
                    title:summary.title,app:summary.app_name,at:summary.ended},text})
            },
            _=>None,
        }
    }

    pub(crate) fn source_links(&self, entries: &[Entry]) -> Vec<SourceLink> {
        let ids=entries.iter().flat_map(|e|e.evidence.iter()).collect::<BTreeSet<_>>();
        ids.into_iter().filter_map(|id|{
            let kind=if id.starts_with("s-") {"session"} else {"history"};
            if kind=="session" {return self.source_view(kind,id,None);}
            let fields=entries.iter().filter(|entry|entry.evidence.contains(id))
                .map(|entry|format!("{:x}",sha2::Sha256::digest(entry.text.as_bytes()))).collect::<BTreeSet<_>>();
            let selected=self.source_view(kind,id,Some(&fields));
            if selected.as_ref().is_some_and(|view|!view.source.key.field_hashes.is_empty()) {selected}
            else {self.source_view(kind,id,None)}
        })
            .map(|view|view.source).collect()
    }

    pub fn memory_source(&self, key: &SourceKey) -> Result<SourceView,String> {
        if key.sha256.len()!=64 || !key.sha256.bytes().all(|b|b.is_ascii_hexdigit()) {
            return Err("Invalid memory source reference".into());
        }
        let fields=key.field_hashes.iter().cloned().collect::<BTreeSet<_>>();
        if fields.len()!=key.field_hashes.len() || fields.len()>128 || (key.kind!="history" && !fields.is_empty())
            || fields.iter().any(|hash|hash.len()!=64 || !hash.bytes().all(|b|b.is_ascii_hexdigit())) {
            return Err("Invalid memory source field reference".into());
        }
        let generation=self.generation();
        let view=self.source_view(&key.kind,&key.id,(!fields.is_empty()).then_some(&fields)).ok_or("This memory source was removed or is unavailable")?;
        if view.source.key!=*key || self.generation()!=generation {
            return Err("This memory source changed. Start a new task to review current context.".into());
        }
        Ok(view)
    }
}
