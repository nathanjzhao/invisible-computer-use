const OLD_QUOTE: &str = "I prefer cobalt notes for future tasks in this app.";
const OLD_NOTE: &str = "The user prefers cobalt notes for future tasks in this app.";
const NEW_GOAL: &str = "I prefer copper notes instead of cobalt notes for future tasks in this app. This replaces my earlier cobalt preference.";
const NEW_QUOTE: &str = "I prefer copper notes instead of cobalt notes for future tasks in this app.";
const NEW_NOTE: &str = "The user prefers copper notes instead of cobalt notes for future tasks in this app.";
const FOREIGN_ONLY: &str = "An unrelated earlier provider conversation about the private orchard.";

struct Case { _root: TempRoot, memory: MemoryService, old: Entry, new: ChatSummary }

fn case(tag: &str, use_memory: bool) -> Case {
    let (root, memory) = service(tag);
    let mut old_summary = summary("s-old", "Calculator", false, &[(Kind::Preference, OLD_NOTE)]);
    old_summary.bundle_id = Some(CALC.into()); old_summary.apps = vec![CALC.into()];
    old_summary.brain_choice = "openai".into(); old_summary.goals = vec![OLD_QUOTE.into(), FOREIGN_ONLY.into()];
    old_summary.consolidated = true;
    memory.write_summary(&old_summary).unwrap();
    let old = Entry { evidence: vec![old_summary.id], untrusted: false, user_said: vec![OLD_QUOTE.into()],
        ..entry(CALC, Kind::Preference, OLD_NOTE, T0) };
    memory.write_durable(&Durable { entries: vec![old.clone()] }).unwrap();
    let options = RunOptions { use_memory, ..opts() };
    let (block, used, grants, _) = memory.context_for_shared(&options, Some(CALC), "Calculator", NEW_GOAL, T0+10);
    if use_memory {
        assert!(block.as_ref().unwrap().contains(OLD_QUOTE));
        assert!(!block.as_ref().unwrap().contains(FOREIGN_ONLY));
        assert_eq!(grants.len(), 1);
    } else { assert!(block.is_none() && used.is_empty() && grants.is_empty()); }
    let record = RunRecord { used_memory: used.iter().map(|entry| entry.id.clone()).collect(),
        ..run("r-shared", NEW_GOAL, T0+10, T0+40, 3) };
    let record = RawRun(record).with_shared_preferences(grants).with_secrets(&[]);
    let reply = json!({"title":"Note color correction", "asked":"The user corrected the preferred note color.",
        "outcome":"The task finished.","preferences":[NEW_NOTE]}).to_string();
    let new = parse_extract(&reply, &Session { runs: vec![record] }, T0+100).unwrap();
    assert_eq!(new.shared_preferences.len(), usize::from(use_memory));
    memory.write_summary(&new).unwrap();
    Case { _root: root, memory, old, new }
}

struct Review {
    calls: Mutex<Vec<(Stage, String, Value)>>,
    reverse: bool,
    offline: bool,
    malformed: bool,
}
impl Review {
    fn new() -> Self { Self { calls: Mutex::new(vec![]), reverse: true, offline: false, malformed: false } }
    fn cross_count(&self) -> usize {
        self.calls.lock().unwrap().iter().filter(|(stage, _, value)| *stage == Stage::Judge &&
            value["candidates"].as_array().unwrap().iter().any(|candidate| candidate["text"] == OLD_QUOTE)).count()
    }
}
impl Models for Review {
    fn complete(&self, stage: Stage, choice: &str, _: &str, user: &str) -> Result<String, String> {
        let value: Value = serde_json::from_str(user).unwrap();
        self.calls.lock().unwrap().push((stage, choice.into(), value.clone()));
        assert_eq!(choice, "claude-code");
        assert!(!user.contains(FOREIGN_ONLY) && !user.contains("s-old") && !user.contains(OLD_NOTE),
            "An earlier provider's unapproved data reached this provider: {user}");
        match stage {
            Stage::Consolidate => {
                let entries: Vec<_> = value["new"].as_array().unwrap().iter().flat_map(|summary|
                    summary["notes"].as_array().unwrap().iter().filter(|note| note["kind"] == "preference")
                        .map(move |note| json!({"scope":summary["app_key"],"kind":"preference","text":note["text"],"evidence":[summary["id"]]}))).collect();
                Ok(json!({"entries":entries}).to_string())
            }
            Stage::Judge => {
                let cross = value["candidates"].as_array().unwrap().iter().any(|candidate| candidate["text"] == OLD_QUOTE);
                if cross && self.offline { return Err("provider connection unavailable".into()); }
                if cross && self.malformed { return Ok("invalid judge response".into()); }
                let verdicts: Vec<_> = value["candidates"].as_array().unwrap().iter().map(|candidate|
                    json!({"id":candidate["id"],"user_stated":true,"same_meaning":true,"contains_secret":false,
                        "later_reversed":cross && self.reverse,"exact_quote":if cross {OLD_QUOTE} else {NEW_QUOTE},"supersedes":false})).collect();
                Ok(json!({"candidates":verdicts,"message_secrets":[]}).to_string())
            }
            _ => panic!("Unexpected model stage: {stage:?}"),
        }
    }
}

#[test]
fn a_new_provider_retires_only_the_preference_approved_in_its_task() {
    let case = case("provider-positive", true); let models = Review::new();
    let report = case.memory.tick(T0+5000, false, &models);
    assert!(report.consolidated, "{report:?}");
    assert_eq!(models.cross_count(), 1);
    let entries = case.memory.durable().entries;
    assert!(!entries.iter().any(|entry| entry.id == case.old.id));
    assert!(entries.iter().any(|entry| entry.text == NEW_NOTE && !entry.untrusted && entry.user_said == [NEW_QUOTE]));
}

#[test]
fn foreign_preference_reviews_keep_application_scopes_separate() {
    let case=case("provider-app-batches",true);
    let app="com.example.Other";
    let mut source=case.memory.summaries()["s-old"].clone();source.id="s-other-old".into();
    source.bundle_id=Some(app.into());source.apps=vec![app.into()];case.memory.write_summary(&source).unwrap();
    let other=Entry{id:entry_id(app,OLD_NOTE),scope:app.into(),evidence:vec![source.id],..case.old.clone()};
    case.memory.write_durable(&Durable{entries:vec![case.old.clone(),other.clone()]}).unwrap();
    let (_,_,grants,_)=case.memory.context_for_shared(&opts(),Some(app),"Other",NEW_GOAL,T0+10);
    assert_eq!(grants.len(),1);
    let mut new=case.new.clone();new.id="s-other-new".into();new.bundle_id=Some(app.into());new.apps=vec![app.into()];
    new.shared_preferences=grants;case.memory.write_summary(&new).unwrap();
    let models=Review::new();assert!(case.memory.tick(T0+5000,false,&models).consolidated);
    assert_eq!(models.cross_count(),2);
    for (stage,_,input) in models.calls.lock().unwrap().iter() {
        if *stage!=Stage::Judge || !input["candidates"].as_array().unwrap().iter().any(|c|c["text"]==OLD_QUOTE){continue;}
        let scopes:BTreeSet<_>=input["candidates"].as_array().unwrap().iter().map(|c|c["scope"].as_str().unwrap()).collect();
        assert_eq!(scopes.len(),1);
        assert!(input["messages"].as_array().unwrap().iter().all(|m|scopes.contains(m["app"].as_str().unwrap())));
    }
    assert!(!case.memory.durable().entries.iter().any(|e|e.id==case.old.id||e.id==other.id));
}

#[test]
fn memory_off_and_legacy_summaries_grant_no_foreign_review() {
    for legacy in [false, true] {
        let case = case(if legacy { "provider-legacy" } else { "provider-off" }, legacy);
        if legacy {
            let mut old_wire = serde_json::to_value(&case.new).unwrap(); old_wire.as_object_mut().unwrap().remove("shared_preferences");
            let restored: ChatSummary = serde_json::from_value(old_wire).unwrap(); assert!(restored.shared_preferences.is_empty());
            case.memory.write_summary(&restored).unwrap();
        }
        let models = Review::new(); case.memory.tick(T0+5000, false, &models);
        assert_eq!(models.cross_count(), 0);
        assert!(case.memory.durable().entries.iter().any(|entry| entry.id == case.old.id));
    }
}

#[test]
fn changed_deleted_and_foreign_app_snapshots_are_not_review_authority() {
    for mode in ["changed", "deleted", "other-app", "altered-goal", "source-deleted"] {
        let mut case = case(&format!("provider-{mode}"), true);
        match mode {
            "changed" => { let mut old=case.old.clone();old.updated+=100;case.memory.write_durable(&Durable{entries:vec![old]}).unwrap(); }
            "deleted" => { case.memory.delete(&case.old.id).unwrap(); }
            "source-deleted" => { case.memory.delete("s-old").unwrap(); }
            "other-app" => {
                let mut source=case.memory.summaries()["s-old"].clone();source.id="s-other".into();
                source.bundle_id=Some("com.example.Other".into());source.apps=vec!["com.example.Other".into()];
                case.memory.write_summary(&source).unwrap();
                let other=Entry{scope:"com.example.Other".into(),id:entry_id("com.example.Other",OLD_NOTE),evidence:vec![source.id],..case.old.clone()};
                case.new.shared_preferences[0].entry_id=other.id.clone();case.new.shared_preferences[0].entry_sha256=shared::fingerprint(&other);
                case.memory.write_durable(&Durable{entries:vec![case.old.clone(),other]}).unwrap();
                case.memory.write_summary(&case.new).unwrap();
            }
            "altered-goal" => { case.new.shared_preferences[0].goal="A goal never typed in this task.".into();case.memory.write_summary(&case.new).unwrap(); }
            _ => unreachable!(),
        }
        let models=Review::new();case.memory.tick(T0+5000,false,&models);assert_eq!(models.cross_count(),0,"{mode}");
    }
}

#[test]
fn an_unconfirmed_reversal_keeps_the_earlier_preference() {
    let case=case("provider-not-reversed",true);let models=Review{reverse:false,..Review::new()};
    assert!(case.memory.tick(T0+5000,false,&models).consolidated);
    assert_eq!(models.cross_count(),1);assert!(case.memory.durable().entries.iter().any(|entry|entry.id==case.old.id));
}

#[test]
fn an_offline_shared_review_stays_pending_and_recovers() {
    let case=case("provider-offline",true);let offline=Review{offline:true,..Review::new()};
    assert!(!case.memory.tick(T0+5000,false,&offline).consolidated);
    assert!(!case.memory.summaries()[&case.new.id].consolidated);
    assert_eq!(case.memory.durable().entries,vec![case.old.clone()]);
    let resumed=MemoryService::open(&case._root.0);let up=Review::new();
    assert!(resumed.tick(T0+5000+BACKOFF_CAP_SECS+1,false,&up).consolidated);
    assert!(!resumed.durable().entries.iter().any(|entry|entry.id==case.old.id));
}

#[test]
fn malformed_shared_verdicts_have_a_persistent_refusal_budget() {
    let case=case("provider-malformed",true);let malformed=Review{malformed:true,..Review::new()};
    for attempt in 0..MAX_ATTEMPTS {
        let resumed=MemoryService::open(&case._root.0);
        let report=resumed.tick(T0+5000+30*i64::from(attempt),false,&malformed);
        assert_eq!(report.consolidated,attempt+1==MAX_ATTEMPTS);
    }
    assert_eq!(malformed.cross_count(),MAX_ATTEMPTS as usize);
    let entries=case.memory.durable().entries;
    assert!(entries.contains(&case.old));
    assert!(entries.iter().any(|entry|entry.text==NEW_NOTE && entry.untrusted && entry.user_said.is_empty()));
    let calls=malformed.calls.lock().unwrap().len();case.memory.tick(T0+9000,false,&malformed);
    assert_eq!(malformed.calls.lock().unwrap().len(),calls,"An idle store must not spend more refusal calls");
}

#[test]
fn redaction_and_partial_goals_drop_shared_review_grants() {
    let case=case("provider-redaction",true);let grant=case.new.shared_preferences[0].clone();
    for (goal,spans) in [(format!("{NEW_GOAL} My password is violet lantern."),vec![]),
                         (NEW_GOAL.to_owned(),vec!["cobalt".to_owned()]),
                         ("I prefer copper notes for future tasks in this app.".into(),vec!["cobalt".into()]),
                         ("x".repeat(601),vec![])] {
        let mut grant=grant.clone();grant.goal=goal.clone();
        let record=RunRecord{used_memory:vec![case.old.id.clone()],..run("redacted",&goal,T0+10,T0+40,3)};
        let record=RawRun(record).with_shared_preferences(vec![grant]).with_secrets(&spans);
        assert!(record.shared_preferences.is_empty());
    }
}

#[test]
fn demonstrations_and_external_context_cannot_grant_foreign_review() {
    let case=case("provider-not-a-task",true);
    let provenance:crate::trajectory::Provenance=serde_json::from_value(json!({"format":"icu","actor":"synthetic_fixture",
        "recording_sha256":"0".repeat(64),"manifest_sha256":"1".repeat(64),"app":CALC,"project":"fixture",
        "consent_id":"owned-test","clock":"UNIX_MS","first_mono_ns":0,"last_mono_ns":0,
        "started_utc":T0,"ended_utc":T0+40,"source_lines":[1],"observed_events":1,"omitted_events":0})).unwrap();
    for external in [false,true] {
        let record=RunRecord{shared_preferences:case.new.shared_preferences.clone(),used_memory:vec![case.old.id.clone()],
            external_context:external,trajectory:if external{None}else{Some(provenance.clone())},
            ..run("non-task",NEW_GOAL,T0+10,T0+40,3)};
        let reply=json!({"title":"Note preference","asked":"The user corrected the note color.","preferences":[NEW_NOTE]}).to_string();
        let summary=parse_extract(&reply,&Session{runs:vec![record]},T0+100).unwrap();
        assert!(summary.shared_preferences.is_empty());
    }
}

#[test]
fn truncated_quotes_and_observed_notes_cannot_be_shared_preferences() {
    let case=case("provider-bounds",true);let sums=case.memory.summaries();
    let mut old=case.old.clone();old.untrusted=true;
    assert!(shared::capture(&old,OLD_QUOTE,CALC,NEW_GOAL,T0+10,&sums).is_none());
    assert!(shared::capture(&case.old,OLD_QUOTE,"com.example.Other",NEW_GOAL,T0+10,&sums).is_none());
    assert!(shared::capture(&case.old,OLD_QUOTE,CALC,NEW_GOAL,T0-1,&sums).is_none());
    let quotes:Vec<String>=["alpha","beta","gamma","delta"].into_iter().map(|name|
        format!("I prefer plain copper notes with a calm layout and generous margins when working on the {name} planning worksheet because that keeps the review easy.")).collect();
    assert!(quotes.join(" / ").chars().count()>MAX_SAID_CHARS);
    let mut old=case.old.clone();old.text="The user prefers plain copper notes.".into();
    old.id=entry_id(CALC,&old.text);old.user_said=quotes.clone();
    let mut sums=sums;let source=sums.get_mut("s-old").unwrap();source.goals=quotes.clone();
    case.memory.write_summary(source).unwrap();case.memory.write_durable(&Durable{entries:vec![old]}).unwrap();
    let (block,used,grants,_)=case.memory.context_for_shared(&opts(),Some(CALC),"Calculator",NEW_GOAL,T0+10);
    assert_eq!(used.len(),1);assert!(block.unwrap().contains(&quotes[0]));
    assert!(grants.is_empty(),"The displayed prefix cannot authorize its unshared suffix");
}

struct ChangeDuringCall<'a> { memory:&'a MemoryService, inner:Review, stage:Stage, provider:bool, changed:std::sync::atomic::AtomicBool }
impl Models for ChangeDuringCall<'_> {
    fn complete(&self,stage:Stage,choice:&str,system:&str,user:&str)->Result<String,String>{
        let reply=self.inner.complete(stage,choice,system,user)?;
        if stage==self.stage && !self.changed.swap(true,std::sync::atomic::Ordering::SeqCst) {
            if self.provider { self.memory.update_settings(&json!({"provider":"codex"})).unwrap(); }
            else { self.memory.delete("all").unwrap(); }
        }
        Ok(reply)
    }
}

#[test]
fn deletion_and_provider_changes_stop_subsequent_model_stages() {
    for (stage,provider) in [(Stage::Consolidate,false),(Stage::Judge,false),(Stage::Consolidate,true)] {
        let case=case("provider-call-boundary",true);
        let models=ChangeDuringCall{memory:&case.memory,inner:Review::new(),stage,provider,changed:std::sync::atomic::AtomicBool::new(false)};
        let report=case.memory.tick(T0+5000,false,&models);assert!(!report.consolidated);
        assert_eq!(models.inner.cross_count(),0);
        assert_eq!(models.inner.calls.lock().unwrap().len(),if stage==Stage::Consolidate{1}else{2});
        if !provider {assert!(case.memory.durable().entries.is_empty());}
    }
}

struct ChangeAuthorization<'a> { memory:&'a MemoryService, old:Entry, source:bool, inner:Review, changed:std::sync::atomic::AtomicBool }
impl Models for ChangeAuthorization<'_> {
    fn complete(&self,stage:Stage,choice:&str,system:&str,user:&str)->Result<String,String>{
        let reply=self.inner.complete(stage,choice,system,user)?;
        if stage==Stage::Judge && !self.changed.swap(true,std::sync::atomic::Ordering::SeqCst) {
            let _guard=self.memory.store().unwrap();
            if self.source {
                let mut source=self.memory.summaries()["s-old"].clone();source.goals=vec!["The earlier quote was removed.".into()];
                self.memory.write_summary(&source).unwrap();
            } else {
                let mut entry=self.old.clone();entry.updated+=200;
                self.memory.write_durable(&Durable{entries:vec![entry]}).unwrap();
            }
        }
        Ok(reply)
    }
}

#[test]
fn a_peer_change_is_rechecked_before_sharing_the_old_quote() {
    for source in [false,true] {
        let case=case("provider-fresh-authority",true);
        let models=ChangeAuthorization{memory:&case.memory,old:case.old.clone(),source,inner:Review::new(),changed:std::sync::atomic::AtomicBool::new(false)};
        let report=case.memory.tick(T0+5000,false,&models);
        assert!(!report.consolidated);assert_eq!(models.inner.cross_count(),0);
        assert_eq!(report.idle_reason.as_deref(),Some("memory changed before shared preference review"));
        assert!(!case.memory.summaries()[&case.new.id].consolidated);
    }
}
