//! Consent-scoped import of taiku-observer semantic events. This module never
//! starts a recorder, reads keys, copies screenshots, or executes recorded input.
//! The wire schema follows taiku-observer-schema's Event and Anchor fields.
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::{collections::BTreeMap, io::Read, path::Path};
mod history_file;
#[cfg(test)] mod history_tests;

const MAX_BYTES: u64 = 16 * 1024 * 1024;
const MAX_EVENTS: usize = 100_000;
pub const CHUNK_STEPS: usize = 40;
const MAX_STEPS: usize = CHUNK_STEPS;
const MAX_PREVIEW_APPS: usize = 128;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Provenance {
    /// Local deduplication receipts. Omitted from model-facing provenance.
    #[serde(default,skip_serializing_if="BTreeMap::is_empty")] pub source_event_hashes:BTreeMap<String,String>,
    #[serde(default,skip_serializing_if="Vec::is_empty")] pub observed_domains:Vec<String>,
    #[serde(default,skip_serializing_if="std::ops::Not::not")] pub includes_values: bool,
    #[serde(default,skip_serializing_if="Option::is_none")] pub reviewed_bytes: Option<u64>,
    #[serde(default,skip_serializing_if="Option::is_none")] pub first_unix_ms: Option<u64>,
    #[serde(default,skip_serializing_if="Option::is_none")] pub last_unix_ms: Option<u64>,
    #[serde(default)] pub incomplete_observations: usize,
    #[serde(default)] pub capture_gap_events: usize,
    #[serde(default)] pub capture_loss_unknown: bool,
    #[serde(default, skip_serializing_if="Option::is_none")]
    pub batch_id: Option<String>,
    #[serde(default)] pub chunk_index: usize,
    #[serde(default)] pub chunk_count: usize,
    #[serde(default)] pub total_events: usize,
    pub format: String,
    pub actor: Actor,
    pub recording_sha256: String,
    pub manifest_sha256: String,
    pub app: String,
    pub project: String,
    pub consent_id: String,
    pub clock: String,
    pub first_mono_ns: i64,
    pub last_mono_ns: i64,
    pub started_utc: i64,
    pub ended_utc: i64,
    pub source_lines: Vec<usize>,
    pub observed_events: usize,
    pub omitted_events: usize,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all="snake_case")]
pub enum Actor { Human, Agent, SyntheticFixture }

#[derive(Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ImportRequest {
    #[serde(default)] pub include_values: bool,
    #[serde(default)] pub reviewed_bytes: Option<u64>,
    pub directory: String,
    pub app: String,
    pub project: String,
    pub consent_id: String,
    pub actor: Actor,
    pub goal: String,
    /// No automatic provider selection for an externally recorded demonstration.
    pub provider: String,
    /// Must match the preview the user reviewed, preventing changed-file imports.
    pub expected_sha256: String,
    pub expected_manifest_sha256: String,
    pub approved: bool,
}

#[derive(Deserialize)]
struct Event { ts_mono_ns: i64, kind: String, payload: Value }
#[derive(Deserialize)]
struct Anchor { mono_ns: i64, epoch_utc_ns: i64, qpf: i64 }
#[derive(Deserialize)]
struct Manifest { master_clock: String, anchors: Vec<Anchor> }

#[derive(Serialize)]
pub struct Preview {
    pub source_format:String,
    pub includes_values:bool,
    pub reviewed_bytes:Option<u64>,
    pub incomplete_observations:usize,
    pub capture_gap_events: usize,
    pub capture_loss_unknown: bool,
    pub recording_sha256: String,
    pub manifest_sha256: String,
    pub clock: String,
    pub apps: BTreeMap<String, usize>,
    /// The exact bounded, redacted steps import will keep, grouped by app.
    pub steps: BTreeMap<String, Vec<PreviewStep>>,
    pub semantic_events: usize,
    pub secure_events_omitted: usize,
    pub has_values: bool,
    pub memory_policy: &'static str,
}

#[derive(Serialize)]
pub struct PreviewStep {
    pub source_line: usize,
    pub action: String,
}

fn incomplete(e:&Event)->bool {
    e.payload.get("truncated").and_then(Value::as_object).is_some_and(|v|v.values().any(|v|v==true))
        || text(&e.payload,"value_kind")=="typing_fragment"
        || e.payload.get("capture_incomplete")==Some(&Value::Bool(true))
        || text(&e.payload,"label").chars().count()>120 || text(&e.payload,"role").chars().count()>60
        || text(&e.payload,"value").chars().count()>2048
}
fn observed_text(value:&str,limit:usize)->String {
    let clean=value.chars().filter(|c|!c.is_control()||c.is_whitespace()).collect::<String>();
    crate::memory::redact_prose(&clean).0.chars().take(limit).collect()
}
fn semantic_action(e: &Event,include_values:bool) -> String {
    let typing_fragment=text(&e.payload,"value_kind")=="typing_fragment";
    let role=observed_text(text(&e.payload,"role"),60);
    // A window.changed crumb (kind ui.window) carries its name in payload.title,
    // not payload.label (skysight focus() sets title, never label), so fall back
    // to the title or the crumb renders with an empty name.
    let raw_label=match text(&e.payload,"label") {
        "" if e.kind=="ui.window" => text(&e.payload,"title"),
        label => label,
    };
    let label=crate::memory::redact_prose(raw_label).0;
    let detail=match e.kind.as_str(){
        "ui.click"|"ui.drag"=>match text(&e.payload,"button"){"left"=>" (left button)","right"=>" (right button)","middle"=>" (middle button)",_=>""}.to_string(),
        "ui.scroll"=>{
            let axis=|key:&str|e.payload.get(key).and_then(Value::as_f64).filter(|v|v.is_finite()).map(|v|if v>0.{"positive"}else if v<0.{"negative"}else{"zero"}).unwrap_or("unknown");
            format!(" (native delta x: {}, y: {})",axis("dx"),axis("dy"))
        },_=>String::new(),
    };
    let mut action=if e.kind=="history.app_focus"{"Observed application activity".to_string()}else if typing_fragment {
        format!("Observed typing fragment on {} {:?}",role,label.chars().take(120).collect::<String>())
    }else{format!("Observed {}{} on {} {:?}",e.kind,detail,role.chars().take(60).collect::<String>(),label.chars().take(120).collect::<String>())};
    if let Some(domain)=e.payload.get("domain").and_then(Value::as_str).filter(|d|computer_history::policy::domain(d).is_ok_and(|v|v==*d)){action.push_str(&format!("; website {domain}"));}
    let partial=incomplete(e)||e.payload.get("value").and_then(Value::as_str).is_some_and(|v|v.chars().count()>2048);
    if typing_fragment{action.push_str("; [partial observation; not a complete field value]");}
    else if partial{action.push_str("; [partial observation; text may be truncated]");}
    if include_values {
        if let Some(value)=e.payload.get("value").and_then(Value::as_str){
            let value=value.chars().filter(|c|!c.is_control()||c.is_whitespace()).collect::<String>();
            let value=if typing_fragment {
                observed_text(&value,2048)
            }else if e.kind=="ui.snapshot" && role=="AXWindowSnapshot" {
                crate::memory::clean_control_snapshot(&value,&|text| crate::memory::clean_native_field("",text))
            } else if matches!(role.as_str(),"AXTextField"|"AXTextArea"|"AXComboBox") {
                crate::memory::clean_native_field(text(&e.payload,"label"),&value)
            } else { observed_text(&value,2048) };
            let description=if typing_fragment{"observed typing fragment"}else{"observed value"};
            action.push_str(&format!("; {description} {:?}",value.chars().take(2048).collect::<String>()));
        }
        if e.kind=="ui.shortcut" {
            if let Some(keys)=e.payload.get("keys").and_then(Value::as_str).filter(|s|s.len()<=80 && s.bytes().all(|b|b.is_ascii_alphanumeric()||b"+_-.".contains(&b))) {
                action.push_str(&format!("; observed shortcut {keys:?}"));
            }
        }
    }
    action
}

pub struct Imported {
    pub provenance: Provenance,
    pub app_name: String,
    pub bundle_id: Option<String>,
    pub steps: Vec<crate::memory::Step>,
    pub chunk_times: Vec<(i64,i64,i64,i64)>,
    pub started: i64,
    pub ended: i64,
    pub incomplete_lines:std::collections::BTreeSet<usize>,
    pub already_imported:usize,
    pub source_event_ids:Vec<String>,
    pub domains_by_line:BTreeMap<usize,String>,
}

fn read_bounded(path: &Path) -> Result<Vec<u8>, String> {
    let mut file = std::fs::File::open(path).map_err(|_| "recording file cannot be read")?.take(MAX_BYTES + 1);
    let mut bytes = Vec::new(); file.read_to_end(&mut bytes).map_err(|_| "recording read failed")?;
    if bytes.len() as u64 > MAX_BYTES { return Err("recording exceeds the 16 MiB import limit; split it into sessions".into()); }
    Ok(bytes)
}
fn sha(bytes: &[u8]) -> String { format!("{:x}", Sha256::digest(bytes)) }
fn safe_metadata(s: &str) -> bool { !s.is_empty() && s.len() <= 120 && s.bytes().all(|b| b.is_ascii_alphanumeric() || b"._-".contains(&b)) }
fn text<'a>(p: &'a Value, key: &str) -> &'a str { p.get(key).and_then(Value::as_str).unwrap_or("") }
fn bundle_id(p:&Value)->Option<&str> {
    let id=text(p,"bundle_id");
    // Some older observer builds stringify a missing CF value as "[ NULL ]".
    // Such a placeholder must not merge unrelated applications into one scope.
    (!id.is_empty() && id.len()<=240 && id.bytes().all(|c|c.is_ascii_alphanumeric()||b"._-".contains(&c))
        && id.bytes().any(|c|c.is_ascii_alphanumeric())).then_some(id)
}
fn app(p: &Value) -> &str { bundle_id(p).unwrap_or_else(||text(p,"app")) }
fn semantic(e: &Event) -> bool { matches!(e.kind.as_str(), "ui.focus" | "ui.click" | "ui.invoke" | "ui.value" | "ui.drag" | "ui.scroll" | "ui.snapshot" | "ui.shortcut" | "ui.submit" | "ui.selection" | "ui.terminal" | "ui.window" | "history.app_focus") }
fn capture_gaps(events:&[(usize,Event)])->(usize,bool){
    let unavailable=|e:&Event|e.kind=="observer.ui_start"&&e.payload.get("enabled")==Some(&Value::Bool(false));
    let degraded=|e:&Event|e.kind=="observer.ui_click_source"&&text(&e.payload,"source")=="button_poll_fallback";
    let count=events.iter().filter(|(_,e)|matches!(e.kind.as_str(),"ui.pointer_gap"|"ui.click_gap")||unavailable(e)||degraded(e)).count();
    let unknown=events.iter().any(|(_,e)|unavailable(e)||degraded(e)||matches!(e.kind.as_str(),"ui.pointer_gap"|"ui.click_gap")&&matches!(text(&e.payload,"reason"),"tap_disabled"|"pointer_tap_unavailable"|"pointer_hook_unavailable"|"desktop_unavailable"|"accessibility_permission_unavailable")||e.payload.get("lost_count_unknown")==Some(&Value::Bool(true)));
    (count,unknown)
}
fn excluded(e: &Event) -> bool {
    e.payload.get("secure").and_then(Value::as_bool) != Some(false)
        || e.payload.get("content_excluded").and_then(Value::as_bool) == Some(true)
        || text(&e.payload,"value_kind") == "secure"
        || text(&e.payload,"subrole") == "AXSecureTextField"
        || text(&e.payload,"role").contains("Secure")
}
fn load(directory: &Path) -> Result<(Manifest, Vec<(usize, Event)>, String, String), String> {
    let bytes = read_bounded(&directory.join("manifest.json"))?;
    let manifest: Manifest = serde_json::from_slice(&bytes).map_err(|_| "invalid observer manifest")?;
    if !matches!(manifest.master_clock.as_str(), "QPC" | "BOOTTIME" | "CONTINUOUS") || manifest.anchors.is_empty() {
        return Err("a supported master clock and clock anchors are required".into());
    }
    if manifest.anchors.iter().any(|a| a.mono_ns < 0 || a.epoch_utc_ns < 0 || a.qpf <= 0)
        || manifest.anchors.windows(2).any(|a| a[0].mono_ns >= a[1].mono_ns) {
        return Err("invalid or unordered clock anchors".into());
    }
    let manifest_hash = sha(&bytes);
    let bytes = read_bounded(&directory.join("events.jsonl"))?;
    let contents = std::str::from_utf8(&bytes).map_err(|_| "events are not UTF-8")?;
    let mut events = Vec::new();
    for (index,line) in contents.lines().enumerate() {
        if line.trim().is_empty() { continue; }
        if events.len() >= MAX_EVENTS { return Err("too many events for one import".into()); }
        let e: Event = serde_json::from_str(line).map_err(|_| format!("invalid event at line {}",index+1))?;
        if e.ts_mono_ns < 0 || !e.payload.is_object() { return Err(format!("invalid event at line {}", index+1)); }
        events.push((index+1,e));
    }
    // Capture threads append concurrently. Sort by capture time, retaining line
    // numbers for provenance and stable ordering of simultaneous events.
    events.sort_by_key(|(line,e)| (e.ts_mono_ns,*line));
    // macOS observer ui.value omits bundle_id. Bind it only to a preceding
    // identity observation for the same PID and app name, within this recording.
    // A process start/exit ends that binding so PID reuse cannot merge apps.
    let mut identities: BTreeMap<i64,(String,String)> = BTreeMap::new();
    for (_,event) in &mut events {
        let Some(pid)=event.payload.get("pid").and_then(Value::as_i64) else {continue};
        if matches!(event.kind.as_str(),"proc.start"|"proc.exit") {identities.remove(&pid);continue;}
        if !event.kind.starts_with("ui.") {continue;}
        let name=text(&event.payload,"app").to_string();
        let bundle=text(&event.payload,"bundle_id").to_string();
        if !name.is_empty() && !bundle.is_empty() {identities.insert(pid,(name,bundle));}
        else if !name.is_empty() && bundle.is_empty() {
            if let Some((known_name,known_bundle))=identities.get(&pid).filter(|(known_name,_)| known_name==&name) {
                let _=known_name;
                event.payload["bundle_id"]=Value::String(known_bundle.clone());
            }
        }
    }
    Ok((manifest,events,sha(&bytes),manifest_hash))
}

pub fn preview(directory: &Path) -> Result<Preview,String> {preview_with_values(directory,false)}
pub fn preview_with_values(directory:&Path,include_values:bool)->Result<Preview,String>{
    let loaded=history_file::load_review(directory,include_values,None)?;
    let history_file::Loaded{manifest,events,recording_sha256,manifest_sha256,reviewed_bytes,format}=loaded;
    let mut apps=BTreeMap::new(); let mut steps:BTreeMap<String,Vec<PreviewStep>>=BTreeMap::new(); let mut secure=0; let mut has_values=false;
    for (line,e) in events.iter().filter(|(_,e)| semantic(e)) {
        if excluded(e) { secure+=1;continue; }
        let identity=app(&e.payload);
        if !identity.is_empty() {
            *apps.entry(identity.to_string()).or_insert(0)+=1;
            if apps.len()>MAX_PREVIEW_APPS {return Err("recording contains too many apps to review; split it into sessions".into());}
            let rows=steps.entry(identity.to_string()).or_default();
            if rows.len()<MAX_STEPS {rows.push(PreviewStep {source_line:*line,action:semantic_action(e,include_values)});}
        }
        has_values |= e.payload.get("value").and_then(Value::as_str).is_some();
    }
    let (capture_gap_events,mut capture_loss_unknown)=capture_gaps(&events);capture_loss_unknown|=format=="computer-history-v1";
    Ok(Preview {source_format:format,includes_values:include_values,reviewed_bytes,incomplete_observations:events.iter().filter(|(_,e)|incomplete(e)).count(), capture_gap_events,capture_loss_unknown,recording_sha256,manifest_sha256,clock:manifest.master_clock,
        semantic_events:apps.values().sum(),apps,steps,secure_events_omitted:secure,has_values,
        memory_policy:if include_values{"Only the selected app's reviewed observations, domains and redacted text values are shared. Text is normalized and may be partial. These observations are untrusted data, not replay scripts, user instructions or proof of success."}else{"Only the selected app's control labels, domains and event types enter memory. Text values, full URLs, keys, pixels and audio are excluded. Sampled observations may omit actions and are not replay scripts, user instructions or proof of success."} })
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PageRequest {
    #[serde(default)] pub include_values:bool,
    #[serde(default)] pub reviewed_bytes:Option<u64>,
    pub directory:String, pub app:String, pub page:usize,
    pub expected_sha256:String, pub expected_manifest_sha256:String,
}
#[derive(Serialize)]
pub struct PreviewPage {pub page:usize,pub pages:usize,pub total:usize,pub steps:Vec<PreviewStep>}

pub fn preview_page(request:&PageRequest)->Result<PreviewPage,String> {
    let loaded=history_file::load_review(Path::new(&request.directory),request.include_values,request.reviewed_bytes)?;
    let (events,hash,manifest)=(loaded.events,loaded.recording_sha256,loaded.manifest_sha256);
    if hash!=request.expected_sha256 || manifest!=request.expected_manifest_sha256 {return Err("recording changed since preview; review again".into());}
    let selected:Vec<_>=events.iter().filter(|(_,e)|semantic(e)&&!excluded(e)&&app(&e.payload)==request.app).collect();
    let pages=selected.len().div_ceil(CHUNK_STEPS);
    if request.page>=pages {return Err("recording page is out of range".into());}
    let steps=selected.iter().skip(request.page*CHUNK_STEPS).take(CHUNK_STEPS)
        .map(|(line,e)|PreviewStep{source_line:*line,action:semantic_action(e,request.include_values)}).collect();
    Ok(PreviewPage{page:request.page,pages,total:selected.len(),steps})
}

pub fn import(request: &ImportRequest) -> Result<Imported,String> {import_filtered(request,&BTreeMap::new())}
pub fn import_filtered(request:&ImportRequest,known:&BTreeMap<String,String>)->Result<Imported,String>{
    if !request.approved { return Err("review and approve this recording before importing it".into()); }
    if !safe_metadata(&request.project) || !safe_metadata(&request.consent_id) || request.app.is_empty() || request.app.len()>240 {
        return Err("project, consent id, and one app identity are required".into());
    }
    if request.goal.trim().is_empty() || request.goal.chars().count()>600 { return Err("a task description of at most 600 characters is required".into()); }
    if request.provider=="auto" || !crate::memory::PROVIDERS.contains(&request.provider.as_str()) { return Err("choose an explicit memory provider".into()); }
    let loaded=history_file::load_review(Path::new(&request.directory),request.include_values,request.reviewed_bytes)?;
    let history_file::Loaded{manifest,events,recording_sha256:hash,manifest_sha256:manifest_hash,reviewed_bytes,format}=loaded;
    if hash!=request.expected_sha256 || manifest_hash!=request.expected_manifest_sha256 {
        return Err("recording changed since preview; review it again".into());
    }
    let is_history=format=="computer-history-v1";
    let eligible:Vec<_>=events.iter().filter(|(_,e)|semantic(e)&&!excluded(e)&&app(&e.payload)==request.app).collect();
    if is_history {for (_,e) in &eligible{if known.get(text(&e.payload,"history_event_id")).is_some_and(|h|h!=text(&e.payload,"history_event_hash")){return Err("an imported history event has changed; its original identity cannot be reused".into())}}}
    let selected:Vec<_>=eligible.iter().copied().filter(|(_,e)|!is_history||!known.contains_key(text(&e.payload,"history_event_id"))).collect();
    let already_imported=eligible.len()-selected.len();
    if selected.len()<if is_history{1}else{2}{return Err(if is_history{"no new eligible history observations remain for this app"}else{"at least two eligible semantic events from the selected app are required"}.into())}
    let utc = |mono:i64| -> Result<i64,String> {
        let anchor=manifest.anchors.iter().min_by_key(|a| (i128::from(a.mono_ns)-i128::from(mono)).abs()).unwrap();
        let ns=i128::from(anchor.epoch_utc_ns)+i128::from(mono)-i128::from(anchor.mono_ns);
        i64::try_from(ns / 1_000_000_000).map_err(|_| "clock conversion overflow".into())
    };
    let first=selected.first().unwrap().1.ts_mono_ns; let last=selected.last().unwrap().1.ts_mono_ns;
    let started=utc(first)?; let ended=utc(last)?;
    if started<0 || ended<started || ended>crate::memory::now()+300 {
        return Err("invalid recording wall-clock interval".into());
    }
    let steps=selected.iter().map(|(_,e)| {
        crate::memory::Step { action:semantic_action(e,request.include_values),status:"observed; effect unverified".into(),code:None }
    }).collect();
    let payload=&selected[0].1.payload;
    let app_name=if text(payload,"app").is_empty() {request.app.clone()} else {text(payload,"app").to_string()};
    let bundle_id=bundle_id(payload).map(str::to_string);
    let chunk_times=selected.chunks(CHUNK_STEPS).map(|chunk| {
        let first=chunk.first().unwrap().1.ts_mono_ns;let last=chunk.last().unwrap().1.ts_mono_ns;
        Ok((first,last,utc(first)?,utc(last)?))
    }).collect::<Result<Vec<_>,String>>()?;
    Ok(Imported {started,ended,steps,chunk_times,app_name,bundle_id,already_imported,domains_by_line:selected.iter().filter_map(|(line,e)|e.payload.get("domain").and_then(Value::as_str).filter(|d|computer_history::policy::domain(d).is_ok_and(|v|v==*d)).map(|d|(*line,d.to_owned()))).collect(),source_event_ids:if is_history{selected.iter().map(|(_,e)|text(&e.payload,"history_event_id").into()).collect()}else{vec![]},incomplete_lines:selected.iter().filter(|(_,e)|incomplete(e)).map(|(line,_)|*line).collect(),provenance:Provenance {observed_domains:selected.iter().filter_map(|(_,e)|e.payload.get("domain").and_then(Value::as_str).filter(|d|computer_history::policy::domain(d).is_ok_and(|v|v==*d)).map(str::to_owned)).collect::<std::collections::BTreeSet<_>>().into_iter().collect(),source_event_hashes:if is_history{selected.iter().map(|(_,e)|(text(&e.payload,"history_event_id").into(),text(&e.payload,"history_event_hash").into())).collect()}else{BTreeMap::new()},includes_values:request.include_values,reviewed_bytes,first_unix_ms:is_history.then_some((first/1_000_000) as u64),last_unix_ms:is_history.then_some((last/1_000_000) as u64),incomplete_observations:selected.iter().filter(|(_,e)|incomplete(e)).count(),capture_gap_events:capture_gaps(&events).0,capture_loss_unknown:capture_gaps(&events).1||is_history,batch_id:None,chunk_index:0,chunk_count:selected.len().div_ceil(CHUNK_STEPS),total_events:selected.len(),format,actor:request.actor,
        recording_sha256:hash,manifest_sha256:manifest_hash,app:request.app.clone(),project:request.project.clone(),
        consent_id:request.consent_id.clone(),clock:manifest.master_clock,first_mono_ns:if is_history{0}else{first},last_mono_ns:if is_history{0}else{last},
        started_utc:started,ended_utc:ended,
        source_lines:selected.iter().map(|(line,_)| *line).collect(),observed_events:selected.len(),
        omitted_events:events.len()-selected.len()}})
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::memory::{MemoryService,Models,Stage,RunOptions};
    use serde_json::json;
    use std::sync::atomic::{AtomicU64,Ordering};
    #[test]
    fn missing_native_bundle_placeholders_do_not_merge_app_scopes() {
        let one=json!({"bundle_id":"[ NULL ]","app":"PointerFixture"});
        let two=json!({"bundle_id":"[ NULL ]","app":"OtherFixture"});
        assert_eq!(app(&one),"PointerFixture");assert_eq!(app(&two),"OtherFixture");
        assert!(bundle_id(&one).is_none());
        assert_eq!(bundle_id(&json!({"bundle_id":"dev.icu.fixture","app":"Renamed"})),Some("dev.icu.fixture"));
    }
    #[test]
    fn ui_window_crumb_renders_its_title_when_label_is_absent() {
        // window.changed events (kind ui.window) carry the window name in
        // payload.title, not payload.label. Without the fallback the crumb
        // renders an empty name.
        let window=Event{ts_mono_ns:100,kind:"ui.window".into(),
            payload:json!({"title":"Project Atlas","role":"AXWindow"})};
        let action=semantic_action(&window,false);
        assert!(action.contains("Project Atlas"),"ui.window title missing from crumb: {action}");
        // A present label still wins, and non-ui.window kinds are unchanged.
        let labeled=Event{ts_mono_ns:100,kind:"ui.window".into(),
            payload:json!({"label":"Main Window","title":"ignored","role":"AXWindow"})};
        assert!(semantic_action(&labeled,false).contains("Main Window"));
        let focus=Event{ts_mono_ns:100,kind:"ui.focus".into(),
            payload:json!({"title":"NotUsedHere","role":"AXButton"})};
        assert!(!semantic_action(&focus,false).contains("NotUsedHere"),"title fallback must be ui.window only");
    }
    static NEXT:AtomicU64=AtomicU64::new(1);
    struct Dir(std::path::PathBuf);
    impl Drop for Dir {fn drop(&mut self) {let _=std::fs::remove_dir_all(&self.0);}}
    struct Offline;
    impl Models for Offline {fn complete(&self,_:Stage,_:&str,_:&str,_:&str)->Result<String,String>{Err("offline".into())}}
    fn fixture()->(Dir,ImportRequest) {
        let dir=Dir(std::env::temp_dir().join(format!("icu-observer-import-{}-{}",std::process::id(),NEXT.fetch_add(1,Ordering::SeqCst))));
        std::fs::create_dir_all(&dir.0).unwrap();
        let manifest=json!({"master_clock":"CONTINUOUS","anchors":[{"mono_ns":100,"epoch_utc_ns":1_790_000_000_000_000_000i64,"qpf":1_000_000_000}]});
        std::fs::write(dir.0.join("manifest.json"),manifest.to_string()).unwrap();
        let events=[
            json!({"ts_mono_ns":300,"kind":"ui.click","payload":{"bundle_id":"test.calc","role":"AXButton","label":"Multiply","secure":false}}),
            json!({"ts_mono_ns":200,"kind":"ui.focus","payload":{"bundle_id":"test.calc","role":"AXTextField","label":"Result","secure":false,"value":"NEVER_PERSIST_THIS_VALUE"}}),
            json!({"ts_mono_ns":400,"kind":"ui.value","payload":{"bundle_id":"test.calc","role":"AXTextField","label":"NEVER_PERSIST_SECURE_LABEL","secure":true,"value":"NEVER_PERSIST_SECRET"}}),
            json!({"ts_mono_ns":500,"kind":"ui.click","payload":{"bundle_id":"other.app","role":"AXButton","label":"NEVER_PERSIST_OTHER_APP","secure":false}}),
            json!({"ts_mono_ns":600,"kind":"input.key","payload":{"vk":35,"text":"NEVER_PERSIST_KEY"}}),
        ];
        std::fs::write(dir.0.join("events.jsonl"),events.iter().map(|e|e.to_string()+"\n").collect::<String>()).unwrap();
        let p=preview(&dir.0).unwrap();
        let request=ImportRequest { include_values:false,reviewed_bytes:None,directory:dir.0.to_string_lossy().into(),app:"test.calc".into(),project:"math".into(),consent_id:"session-opt-in".into(),actor:Actor::SyntheticFixture,goal:"The demonstration shows multiplication in this app.".into(),provider:"local".into(),expected_sha256:p.recording_sha256,expected_manifest_sha256:p.manifest_sha256,approved:true};
        (dir,request)
    }
    #[test]
    fn reviewed_named_values_keep_identifiers_but_not_credentials() {
        let (dir,mut request)=fixture();
        let rows=[
            json!({"ts_mono_ns":100,"kind":"ui.value","payload":{"bundle_id":"test.calc","role":"AXTextField","label":"Project","value":"Juniper1","secure":false}}),
            json!({"ts_mono_ns":200,"kind":"ui.snapshot","payload":{"bundle_id":"test.calc","role":"AXWindowSnapshot","value":"AXTextField \"Note\": \"Office365\"\nAXTextField \"Password\": \"PrivateCanary73\"","secure":false}}),
            json!({"ts_mono_ns":300,"kind":"ui.value","payload":{"bundle_id":"test.calc","role":"AXTextField","label":"Note","value":"The password is PrivateCanary73.","secure":false}}),
            json!({"ts_mono_ns":400,"kind":"ui.value","payload":{"bundle_id":"test.calc","role":"AXTextField","label":"Note","value":"sk-proj-abcdefghijklmnopqrstuvwx123456","secure":false}}),
        ];
        std::fs::write(dir.0.join("events.jsonl"),rows.iter().map(|e|e.to_string()+"\n").collect::<String>()).unwrap();
        let preview=preview_with_values(&dir.0,true).unwrap();
        request.include_values=true;request.expected_sha256=preview.recording_sha256;
        request.expected_manifest_sha256=preview.manifest_sha256;
        let reviewed=&preview.steps["test.calc"];
        let imported=import(&request).unwrap();
        let actions=reviewed.iter().map(|step|step.action.clone()).collect::<Vec<_>>();
        assert_eq!(actions,imported.steps.iter().map(|step|step.action.clone()).collect::<Vec<_>>());
        let text=actions.join("\n");
        assert!(text.contains("Juniper1") && text.contains("Office365"),"{text}");
        assert!(!text.contains("PrivateCanary73") && !text.contains("abcdefghijklmnopqrstuvwx123456"));
        // Exercise the actual journal and later extraction boundary, not only
        // the pre-journal trajectory representation.
        let memory=crate::memory::MemoryService::open(dir.0.join("memory"));
        memory.import_trajectory(&request,&Offline).unwrap();
        let runs=memory.pending_runs();
        let journal=serde_json::to_string(&runs).unwrap();
        assert!(journal.contains("Juniper1") && journal.contains("Office365"),"{journal}");
        assert!(!journal.contains("PrivateCanary73") && !journal.contains("abcdefghijklmnopqrstuvwx123456"));
        let sessions=crate::memory::sessions(runs,1800);
        let extracted=crate::memory::extract_input(&sessions[0]).to_string();
        assert!(extracted.contains("Juniper1") && extracted.contains("Office365"),"{extracted}");
        let fact="The product in Juniper1's project was Office365.";
        let reply=json!({"title":"Reviewed project","asked":"The user asked to review the recording.",
            "outcome":"Observed fields.","worked":[],"corrections":[],
            "preferences":["The user prefers Office365."],
            "app_facts":[fact,"The project was UnseenProject73."]});
        let summary=crate::memory::parse_extract(&reply.to_string(),&sessions[0],crate::memory::now()).unwrap();
        assert_eq!(summary.notes.len(),1,"{:?}",summary.dropped);
        assert_eq!(summary.notes[0].text,fact);
        assert_eq!(summary.notes[0].kind,crate::memory::Kind::AppFact);
        assert!(summary.untrusted && summary.cleared.is_empty());
        assert!(summary.observed_note_clearances.contains_key(fact));
        let summary:crate::memory::ChatSummary=serde_json::from_str(&serde_json::to_string(&summary).unwrap()).unwrap();
        let evidence=BTreeMap::from([(summary.id.clone(),summary.clone())]);
        let consolidation=crate::memory::consolidate_input(&[],&[summary.clone()],&evidence).to_string();
        assert!(consolidation.contains("Office365"),"{consolidation}");
        let proposed=json!({"entries":[{"scope":"test.calc","kind":"app_fact","text":fact,"evidence":[summary.id],"replaces":[]},
            {"scope":"test.calc","kind":"preference","text":"The user prefers Office365.","evidence":[summary.id],"replaces":[]}]});
        let (entries,_)=crate::memory::propose(&proposed.to_string(),&[],&evidence).unwrap();
        assert_eq!(entries.len(),1);assert_eq!(entries[0].kind,crate::memory::Kind::AppFact);
        let (judge,_)=crate::memory::judge_input(&entries,&[],&evidence,&[]);
        assert!(judge["candidates"][0]["text"].as_str().unwrap().contains("Office365"));
        let (durable,dropped)=crate::memory::settle(entries.clone(),&crate::memory::Judgement::default(),&[],&evidence,crate::memory::now());
        assert_eq!(durable.len(),1,"{dropped:?}");
        assert!(durable[0].untrusted && durable[0].user_said.is_empty());
        assert!(crate::memory::check_text(&durable[0].text,200,&crate::memory::evidence_grounding(&durable[0],&evidence)).is_ok());
        let mut edited=summary.clone();edited.notes[0].text="The product was UnseenProject73.".into();
        let edited_evidence=BTreeMap::from([(edited.id.clone(),edited)]);
        assert!(crate::memory::propose(&proposed.to_string(),&[],&edited_evidence).unwrap().0.is_empty(),
            "A changed note cannot borrow the old note's field context");
        let mut unrelated=summary.clone();
        unrelated.notes.push(crate::memory::Note{kind:crate::memory::Kind::AppFact,text:"An unrelated note named Office365.".into()});
        let input=crate::memory::consolidate_input(&[],&[unrelated],&evidence);
        assert!(!input["new"][0]["notes"][1]["text"].as_str().unwrap().contains("Office365"));
        let labels_only=preview_with_values(&dir.0,false).unwrap();
        let text=serde_json::to_string(&labels_only.steps).unwrap();
        assert!(!text.contains("Juniper1") && !text.contains("Office365"));
    }

    #[test]
    fn preview_shows_exact_imported_steps_without_values_or_protected_labels() {
        let (dir,request)=fixture();
        let preview=preview(&dir.0).unwrap();
        let rows=&preview.steps["test.calc"];
        let imported=import(&request).unwrap();
        assert_eq!(rows.iter().map(|r|r.action.clone()).collect::<Vec<_>>(),imported.steps.iter().map(|s|s.action.clone()).collect::<Vec<_>>());
        assert_eq!(rows.iter().map(|r|r.source_line).collect::<Vec<_>>(),imported.provenance.source_lines);
        let json=serde_json::to_string(rows).unwrap();
        for secret in ["NEVER_PERSIST_THIS_VALUE","NEVER_PERSIST_SECURE_LABEL","NEVER_PERSIST_KEY","NEVER_PERSIST_OTHER_APP"] {assert!(!json.contains(secret));}
    }
    #[test]
    fn preview_and_import_have_the_same_bounded_slice() {
        let (dir,mut request)=fixture();
        let events=(0..55).map(|n|json!({"ts_mono_ns":100+n,"kind":"ui.click","payload":{"bundle_id":"test.calc","role":"AXButton","label":format!("Button {n}"),"secure":false}}).to_string()+"\n").collect::<String>();
        std::fs::write(dir.0.join("events.jsonl"),events).unwrap();
        let p=preview(&dir.0).unwrap();request.expected_sha256=p.recording_sha256.clone();
        assert_eq!(p.apps["test.calc"],55);assert_eq!(p.steps["test.calc"].len(),40);
        let imported=import(&request).unwrap();assert_eq!(imported.steps.len(),55);assert_eq!(imported.provenance.omitted_events,0);assert_eq!(imported.chunk_times.len(),2);
        assert_eq!(p.steps["test.calc"].last().unwrap().action,imported.steps[39].action);
    }
    #[test]
    fn observer_import_orders_events_and_excludes_values_secrets_and_other_apps() {
        let (_dir,request)=fixture();let imported=import(&request).unwrap();
        assert_eq!(imported.provenance.source_lines,vec![2,1]);
        assert_eq!(imported.provenance.observed_events,2);assert_eq!(imported.provenance.omitted_events,3);
        assert_eq!(imported.started,1_790_000_000);assert_eq!(imported.started,imported.ended);
        let text=serde_json::to_string(&imported.steps).unwrap();assert!(!text.contains("NEVER_PERSIST"));
        assert!(text.contains("Multiply"));assert!(text.contains("effect unverified"));
    }
    #[test]
    fn import_requires_consent_identity_reviewed_bytes_and_explicit_provider() {
        let (_dir,request)=fixture();
        for change in 0..6 {
            let mut q=request.clone();match change {
                0=>q.approved=false,1=>q.consent_id.clear(),2=>q.expected_sha256="0".repeat(64),
                3=>q.provider="auto".into(),4=>q.expected_manifest_sha256="0".repeat(64),_=>q.app="other.app".into()
            }
            assert!(import(&q).is_err(),"change {change} admitted");
        }
    }
    #[test]
    fn long_recording_is_atomic_chunked_ordered_and_deleted_as_one_source() {
        let (dir,mut request)=fixture();
        let events=(0..81).map(|n|json!({"ts_mono_ns":100+n,"kind":"ui.click","payload":{"bundle_id":"test.calc","role":"AXButton","label":format!("Control {n}"),"secure":false}}).to_string()+"\n").collect::<String>();
        std::fs::write(dir.0.join("events.jsonl"),events).unwrap();request.expected_sha256=preview(&dir.0).unwrap().recording_sha256;
        let memory=MemoryService::open(dir.0.join("memory"));let result=memory.import_trajectory(&request,&Offline).unwrap();
        assert_eq!(result["chunks"],3);assert_eq!(result["steps"],81);
        let mut runs=memory.pending_runs();runs.sort_by_key(|r|r.trajectory.as_ref().unwrap().chunk_index);
        assert_eq!(runs.iter().map(|r|r.steps.len()).collect::<Vec<_>>(),[40,40,1]);
        let lines:Vec<_>=runs.iter().flat_map(|r|r.trajectory.as_ref().unwrap().source_lines.clone()).collect();assert_eq!(lines,(1..=81).collect::<Vec<_>>());
        let sessions=crate::memory::sessions(runs.clone(),1800);assert_eq!(sessions.len(),3);assert!(sessions.iter().all(|s|!s.is_short()));
        assert!(memory.import_trajectory(&request,&Offline).is_err());
        let page=preview_page(&PageRequest { include_values:false,reviewed_bytes:None,directory:request.directory.clone(),app:request.app.clone(),page:2,expected_sha256:request.expected_sha256.clone(),expected_manifest_sha256:request.expected_manifest_sha256.clone()}).unwrap();
        assert_eq!(page.steps.len(),1);assert_eq!(page.steps[0].source_line,81);
        assert_eq!(memory.delete(&runs[1].run_id).unwrap(),3);assert!(memory.pending_runs().is_empty());
        assert!(!memory.root().join("runs").join(result["id"].as_str().unwrap()).exists());
    }
    #[test]
    fn pointer_semantics_and_capture_gaps_survive_import_without_raw_coordinates(){
        let (dir,mut q)=fixture();let events=[
            json!({"ts_mono_ns":100,"kind":"ui.click","payload":{"bundle_id":"test.calc","secure":false,"role":"Button","label":"Menu","button":"right","x":12345}}),
            json!({"ts_mono_ns":200,"kind":"ui.scroll","payload":{"bundle_id":"test.calc","secure":false,"role":"ScrollArea","label":"History","dx":0,"dy":-4}}),
            json!({"ts_mono_ns":300,"kind":"ui.drag","payload":{"bundle_id":"test.calc","secure":false,"role":"Slider","label":"Volume","button":"left","from_x":67890}}),
            json!({"ts_mono_ns":400,"kind":"ui.pointer_gap","payload":{"reason":"tap_disabled","count":1}})
        ];
        std::fs::write(dir.0.join("events.jsonl"),events.iter().map(|e|e.to_string()+"\n").collect::<String>()).unwrap();
        let p=preview(&dir.0).unwrap();assert_eq!(p.capture_gap_events,1);assert!(p.capture_loss_unknown);q.expected_sha256=p.recording_sha256;
        let i=import(&q).unwrap();assert_eq!(i.steps.len(),3);let text=serde_json::to_string(&i.steps).unwrap();
        assert!(text.contains("right button")&&text.contains("native delta x: zero, y: negative")&&text.contains("ui.drag"));
        assert!(!text.contains("12345")&&!text.contains("67890"));assert_eq!(i.provenance.capture_gap_events,1);
    }
    #[test]
    fn unavailable_and_legacy_degraded_sensors_are_visible_in_review(){
        let events=vec![(1,Event{ts_mono_ns:1,kind:"observer.ui_start".into(),payload:json!({"enabled":false})}),(2,Event{ts_mono_ns:2,kind:"observer.ui_click_source".into(),payload:json!({"source":"button_poll_fallback"})})];
        assert_eq!(capture_gaps(&events),(2,true));
    }
    #[test]
    fn journal_keeps_provenance_without_claiming_model_decisions_or_success() {
        let (dir,request)=fixture();let memory=MemoryService::open(dir.0.join("memory"));
        let result=memory.import_trajectory(&request,&Offline).unwrap();
        let runs=memory.pending_runs();assert_eq!(runs.len(),1);let run=&runs[0];
        assert_eq!(run.decisions,0);assert_eq!(run.outcome,"observed");assert_eq!(run.trajectory.as_ref().unwrap().actor,Actor::SyntheticFixture);
        assert!(!crate::memory::sessions(runs.clone(),1800)[0].is_short());
        assert!(memory.import_trajectory(&request,&Offline).is_err(),"duplicate import");
        let stored=std::fs::read_to_string(dir.0.join("memory/runs").join(format!("{}.json",result["id"].as_str().unwrap()))).unwrap();
        assert!(!stored.contains("NEVER_PERSIST"));
        assert!(memory.context_for(&RunOptions::default(),Some("other.app"),"Other","multiply").1.is_empty());
        assert_eq!(memory.delete(result["id"].as_str().unwrap()).unwrap(),1);assert!(memory.pending_runs().is_empty());
    }
    #[test]
    fn malformed_tail_and_missing_clock_fail_the_whole_import() {
        let (dir,request)=fixture();
        let mut data=std::fs::read_to_string(dir.0.join("events.jsonl")).unwrap();data.push_str("{truncated");
        std::fs::write(dir.0.join("events.jsonl"),data).unwrap();assert!(import(&request).is_err());
        std::fs::write(dir.0.join("manifest.json"),"{}").unwrap();assert!(preview(&dir.0).is_err());
    }
    #[test]
    fn value_events_inherit_only_a_live_matching_process_identity() {
        let (dir,_)=fixture();
        let events=[
            json!({"ts_mono_ns":100,"kind":"ui.app","payload":{"pid":40,"app":"Calc","bundle_id":"test.calc"}}),
            json!({"ts_mono_ns":200,"kind":"ui.value","payload":{"pid":40,"app":"Calc","secure":false,"label":"Result"}}),
            json!({"ts_mono_ns":300,"kind":"ui.value","payload":{"pid":41,"app":"Calc","secure":false,"label":"Other process"}}),
            json!({"ts_mono_ns":400,"kind":"proc.exit","payload":{"pid":40}}),
            json!({"ts_mono_ns":500,"kind":"ui.value","payload":{"pid":40,"app":"Calc","secure":false,"label":"Reused PID"}}),
        ];
        std::fs::write(dir.0.join("events.jsonl"),events.iter().map(|e|e.to_string()+"\n").collect::<String>()).unwrap();
        let p=preview(&dir.0).unwrap();assert_eq!(p.apps["test.calc"],1);assert_eq!(p.apps["Calc"],2);
    }
    #[test]
    fn a_recordings_explicit_provider_wins_over_the_global_memory_default() {
        struct OnlyLocal(std::sync::Mutex<Vec<String>>);
        impl Models for OnlyLocal {
            fn complete(&self,stage:Stage,choice:&str,_:&str,_:&str)->Result<String,String> {
                self.0.lock().unwrap().push(choice.into());assert_eq!(choice,"local");
                match stage {Stage::Secrets=>Ok("{\"secrets\":[]}".into()),_=>Err("stop after routing check".into())}
            }
        }
        let (dir,request)=fixture();let memory=MemoryService::open(dir.0.join("memory"));
        memory.update_settings(&json!({"provider":"openai"})).unwrap();
        let model=OnlyLocal(Default::default());memory.import_trajectory(&request,&model).unwrap();
        memory.tick(crate::memory::now()+1801,false,&model);
        assert_eq!(*model.0.lock().unwrap(),vec!["local","local"]);
    }
    #[test]
    fn rejected_recording_notes_retry_with_feedback_instead_of_becoming_empty_memories() {
        struct Retry(std::sync::Mutex<usize>);
        impl Models for Retry {
            fn complete(&self,stage:Stage,_:&str,_:&str,user:&str)->Result<String,String> {
                Ok(match stage {
                    Stage::Secrets=>"{\"secrets\":[]}".into(),
                    Stage::Extract=>{
                        let mut n=self.0.lock().unwrap();*n+=1;
                        let input:Value=serde_json::from_str(user).unwrap();
                        assert_eq!(input.get("validation_feedback").is_some(),*n>1);
                        let note=if *n==1 {"The control lives at /private/unapproved."} else {"The calculation controls include a Multiply button."};
                        json!({"title":"Calculation controls","asked":"The user described multiplication.","outcome":"The demonstration ended.","worked":[],"preferences":[],"corrections":[],"app_facts":[note]}).to_string()
                    },
                    Stage::Consolidate=>"{\"entries\":[]}".into(),
                    Stage::Judge=>"{\"candidates\":[],\"message_secrets\":[]}".into(),
                })
            }
        }
        let (dir,request)=fixture();let memory=MemoryService::open(dir.0.join("memory"));let model=Retry(Default::default());
        memory.import_trajectory(&request,&model).unwrap();let clock=crate::memory::now()+1801;
        let first=memory.tick(clock,false,&model);assert!(!first.refused.is_empty());assert!(memory.summaries().is_empty());assert_eq!(memory.pending_runs().len(),1);
        let second=memory.tick(clock+30,false,&model);assert_eq!(second.extracted.len(),1);assert_eq!(*model.0.lock().unwrap(),2);
        let summary=memory.summaries().into_values().next().unwrap();assert_eq!(summary.notes.len(),1);assert!(summary.untrusted);
        assert!(summary.outcome.contains("not independently verified"));
    }
    #[test]
    fn paused_import_keeps_a_journal_but_runs_no_generation_until_resume() {
        struct Count(std::sync::Mutex<usize>);
        impl Models for Count {
            fn complete(&self,_:Stage,_:&str,_:&str,_:&str)->Result<String,String> {
                *self.0.lock().unwrap()+=1;Err("generation observed".into())
            }
        }
        let (dir,request)=fixture();let memory=MemoryService::open(dir.0.join("memory"));let model=Count(Default::default());
        memory.set_paused(true).unwrap();memory.import_trajectory(&request,&model).unwrap();
        let tick=memory.tick(crate::memory::now()+1801,false,&model);
        assert_eq!(tick.idle_reason.as_deref(),Some("paused"));assert_eq!(*model.0.lock().unwrap(),0);
        assert_eq!(memory.pending_runs().len(),1);assert!(memory.summaries().is_empty());
        memory.set_paused(false).unwrap();memory.tick(crate::memory::now()+1831,false,&model);
        assert_eq!(*model.0.lock().unwrap(),1);
    }
    #[test]
    fn existing_journals_and_summaries_without_recording_fields_still_load() {
        let (dir,_)=fixture();let memory=MemoryService::open(dir.0.join("memory"));
        let mut recorder=crate::memory::Recorder::default();recorder.observe("start",&json!({"brain":"ollama:test"}));
        recorder.observe("think",&json!({}));recorder.observe("think",&json!({}));recorder.observe("done",&json!({"summary":"The result is 42."}));
        let run=recorder.finish_raw("legacy-run","Compute six times seven.","Calc",Some("test.calc"),100,120,&RunOptions::default(),vec![]).detectors_only();
        let mut old=serde_json::to_value(&run).unwrap();old.as_object_mut().unwrap().remove("trajectory");
        skysight::private::create_dir(&dir.0.join("memory/runs")).unwrap();
        skysight::private::write(&dir.0.join("memory/runs/legacy-run.json"),serde_json::to_vec(&old).unwrap()).unwrap();
        let loaded=memory.pending_runs();assert_eq!(loaded.len(),1);assert!(loaded[0].trajectory.is_none());assert_eq!(loaded[0].outcome,"done");
        let summary=crate::memory::parse_extract(&json!({"title":"Calculation","asked":"The user asked for multiplication.","outcome":"The result was 42.","worked":[],"corrections":[],"preferences":[],"app_facts":[]}).to_string(),&crate::memory::Session{runs:loaded},130).unwrap();
        let mut old=serde_json::to_value(&summary).unwrap();old.as_object_mut().unwrap().remove("recordings");
        skysight::private::create_dir(&dir.0.join("memory/summaries")).unwrap();
        skysight::private::write(&dir.0.join("memory/summaries").join(format!("{}.json",summary.id)),serde_json::to_vec(&old).unwrap()).unwrap();
        let loaded=memory.summaries();assert_eq!(loaded.len(),1);assert!(loaded.values().next().unwrap().recordings.is_empty());
    }
}

#[cfg(test)]
mod live {
    use super::*;
    use crate::memory::{MemoryService, Models, Stage, RunOptions};
    use serde_json::json;
    use std::sync::Mutex;
    struct Real { calls: Mutex<Vec<Value>> }
    impl Models for Real {
        fn complete(&self, stage:Stage, choice:&str, system:&str, user:&str)->Result<String,String> {
            if self.calls.lock().unwrap().len()>=12 {return Err("trajectory acceptance model budget exhausted".into());}
            assert_eq!(choice,"claude-code");
            let started=std::time::Instant::now();
            let reply=crate::brain::claude_code_complete(None,"claude-sonnet-5",system,user);
            self.calls.lock().unwrap().push(json!({"stage":format!("{stage:?}"),"user":user,"reply":reply.as_ref().ok(),"error":reply.as_ref().err(),"ms":started.elapsed().as_millis()}));
            reply
        }
    }
    #[test]
    #[ignore = "imports a disposable fixture recording and calls a real model, requires explicit paths"]
    fn real_model_recording_summary_retrieval_and_delete() {
        let source=std::env::var("ICU_TRAJECTORY_FIXTURE_RECORDING").expect("recorded fixture path required");
        let out=std::env::var("ICU_TRAJECTORY_ACCEPTANCE_OUT").expect("evidence output path required");
        let out=Path::new(&out);assert!(!out.exists(),"fresh acceptance directory required");std::fs::create_dir_all(out).unwrap();
        let p=preview(Path::new(&source)).unwrap();let memory=MemoryService::open(out.join("memory"));
        let request=ImportRequest { include_values:false,reviewed_bytes:None,directory:source,app:"dev.icu.paired.fixture".into(),project:"paired-benchmark".into(),consent_id:"owned-fixture-20261008".into(),actor:Actor::SyntheticFixture,goal:"This demonstration shows the calculation controls for multiplication in PairFixture.".into(),provider:"claude-code".into(),expected_sha256:p.recording_sha256,expected_manifest_sha256:p.manifest_sha256,approved:true };
        let model=Real {calls:Mutex::new(Vec::new())};
        let result=memory.import_trajectory(&request,&model);
        let mut receipt=json!({"import":result.as_ref().ok(),"import_error":result.as_ref().err(),"passed":false,"desktop_input":false});
        if let Ok(imported)=result {
            let journal=memory.pending_runs();receipt["journal"]=json!(journal);
            let clock=crate::memory::now()+1801;let mut ticks=Vec::new();
            for attempt in 0..3 {
                let tick=memory.tick(clock+attempt*30,false,&model);
                let retry=!tick.refused.is_empty() && tick.extracted.is_empty();
                ticks.push(json!({"extracted":tick.extracted,"refused":tick.refused,"consolidated":tick.consolidated,"dropped":tick.consolidation_dropped,"skipped":tick.skipped,"idle_reason":tick.idle_reason}));
                if !retry {break;}
            }
            receipt["ticks"]=json!(ticks);
            let summaries=memory.summaries();receipt["summaries"]=json!(summaries);receipt["durable"]=json!(memory.durable());
            let (context,entries,_)=memory.context_for(&RunOptions::default(),Some("dev.icu.paired.fixture"),"PairFixture","Multiply two numbers with the calculation controls.");
            let other=memory.context_for(&RunOptions::default(),Some("other.app"),"Other","Multiply two numbers with the calculation controls.").1;
            let disabled=memory.context_for(&RunOptions{use_memory:false,..RunOptions::default()},Some("dev.icu.paired.fixture"),"PairFixture","Multiply two numbers.").1;
            receipt["reuse_context"]=json!(context);receipt["reused_entries"]=json!(entries);receipt["other_app_entries"]=json!(other);receipt["disabled_entries"]=json!(disabled);
            let deleted=memory.delete(imported["id"].as_str().unwrap());receipt["deleted"]=json!(deleted.as_ref().ok());
            let after=memory.context_for(&RunOptions::default(),Some("dev.icu.paired.fixture"),"PairFixture","Multiply two numbers with the calculation controls.").1;
            receipt["after_delete_entries"]=json!(after);
            receipt["passed"]=json!(!summaries.is_empty() && summaries.values().all(|s|!s.recordings.is_empty() && s.untrusted)
                && !entries.is_empty() && entries.iter().all(|e|e.untrusted) && other.is_empty() && disabled.is_empty()
                && deleted.is_ok() && after.is_empty() && memory.summaries().is_empty());
        }
        receipt["model_calls"]=json!(*model.calls.lock().unwrap());
        std::fs::write(out.join("result.json"),serde_json::to_vec_pretty(&receipt).unwrap()).unwrap();
        assert_eq!(receipt["passed"],true,"inspect {}",out.join("result.json").display());
    }
}
