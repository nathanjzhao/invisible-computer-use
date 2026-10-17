//! Skysight's sensor and normalizer inside the one Computer History lifecycle.
//! No second host, storage root, policy owner or collector lease is started.
use computer_history::{capture::Source as FocusSource, policy::{Context, Decision, Website}, store::{CaptureMode, State, Store}, Error, Result};
use serde_json::{json, Value};
use skysight::{host::Source as EventSource, stream::{self, Observation, Output, Recorder, Target}};
use std::time::{Duration, Instant};

struct Session {
    source: skysight::macos::MacSource,
    recorder: Recorder,
    generation: String,
    revision: u64,
    context: Context,
    started_ms: u64,
    last_snapshot: Option<Value>,
    snapshot_at: Instant,
    // A fresh window snapshot is worth re-walking only after an interaction
    // (or a focus change); an idle window is not re-read every second.
    activity_since_snapshot: bool,
}
impl Drop for Session { fn drop(&mut self) { self.source.stop(); } }

#[derive(Default)]
pub struct Collector { session: Option<Session>, idle_ticks: u32 }

/// Idle backoff: sample fast right after activity, then slow the empty ticks so
/// an idle session costs near the heartbeat rate instead of a fixed 50 Hz.
const POLL_FAST: Duration = Duration::from_millis(20);
fn idle_backoff(idle_ticks: u32) -> Duration {
    match idle_ticks {
        0..=1 => POLL_FAST,
        2..=4 => Duration::from_millis(60),
        5..=9 => Duration::from_millis(120),
        _ => Duration::from_millis(250),
    }
}
/// A tick re-walks accessibility (the post-drain identify/read/verify) only when
/// it has something to attribute: an event was drained or buffer-flushed, or the
/// focus generation just changed. An empty tick on the same window does not.
fn should_reverify(focus_unchanged: bool, has_output: bool) -> bool { !focus_unchanged || has_output }
fn next_idle_ticks(idle_ticks: u32, active: bool) -> u32 { if active { 0 } else { idle_ticks.saturating_add(1) } }

fn eligible_state(state:&State, context:&Context)->bool {
    state.enabled && !state.paused && state.capture_mode == CaptureMode::InteractionEvents
        && context.process_id != i64::from(std::process::id()) && context.website == Website::NotWeb
        && state.settings.evaluate(context) == Decision::Allowed
}
fn native_target(target:Option<&Target>,context:&Context)->bool {
    target.is_some_and(|t| !t.is_secure() && t.pid == Some(context.process_id)
        && t.native_window_id.map(i64::from) == Some(context.window_id)
        && t.role.as_deref().is_some_and(|r| matches!(r,"AXTextField"|"AXTextArea"|"AXStaticText"|"AXButton"|"AXCheckBox"|"AXRadioButton"|"AXPopUpButton"|"AXMenuButton"|"AXSlider"|"AXComboBox")))
}
fn observation_allowed(event:&Observation,context:&Context)->bool {
    match event {
        Observation::Focus{app,window}=>app.pid==context.process_id && app.bundle_id.as_deref()==Some(&context.application)
            && window.as_ref().is_some_and(|w| i64::from(w.id)==context.window_id && !w.private && w.url.is_none() && w.frame_urls.is_empty()),
        Observation::Click{target,..}|Observation::Drag{target}|Observation::Selection{target,..}=>native_target(target.as_ref(),context),
        Observation::Key{target,secure_input,..}=>!*secure_input && native_target(target.as_ref(),context),
        Observation::TerminalValue{window_id,..}=>i64::from(*window_id)==context.window_id && stream::is_terminal(Some(&context.application)),
        Observation::Locked(value)=>!*value,
    }
}
fn record_payload(record:skysight::EventRecord)->Value {
    let typing_fragment=record.kind==stream::TEXT_INPUT;
    let source_timestamp=record.observed_at.as_ref().unwrap_or(&record.ts).clone();
    json!({"capture_mode":"interaction_events","secure":false,"role":record.role.unwrap_or_else(||"AXInteraction".into()),
        "label":record.label.or(record.title),"value":record.text.or(record.command),"button":record.button,
        "value_kind":typing_fragment.then_some("typing_fragment"),
        "keys":record.keys,"click_count":record.click_count,"source_timestamp":source_timestamp,
        "normalizer_timestamp":record.ts,
        "truncated":{"role":false,"label":false,"value":record.observation_truncated},"capture_incomplete":record.observation_truncated})
}
impl Collector {
    pub fn stop(&mut self) { self.session=None; }
    /// How long the collector loop should sleep before the next sample: fast
    /// while interacting, longer once the session has been idle.
    pub fn backoff(&self) -> Duration { idle_backoff(self.idle_ticks) }
    pub fn sample(&mut self,store:&Store,sink:&impl Fn(&Context,u64,&Value)->std::result::Result<(),String>)->Result<()> {
        let active=self.sample_inner(store,sink)?;
        self.idle_ticks=next_idle_ticks(self.idle_ticks,active);
        Ok(())
    }
    /// Returns whether this tick was active (drained/flushed an event, or focus
    /// changed), so the loop can reset its idle backoff.
    fn sample_inner(&mut self,store:&Store,sink:&impl Fn(&Context,u64,&Value)->std::result::Result<(),String>)->Result<bool> {
        let state=store.state()?;
        let mut focus=computer_history::macos::MacSource::default();
        let Some(context)=focus.identify()? else {self.stop();return Ok(false)};
        if !eligible_state(&state,&context) {self.stop();return Ok(false)}
        // Use the established ancestry/security check before starting or reading
        // an event sensor. Unknown text subroles and embedded web focus fail shut.
        if focus.read(&context,CaptureMode::Semantic)?.is_none() {self.stop();return Ok(false)}
        let current=self.session.as_ref().is_some_and(|s|s.generation==state.generation && s.revision==state.revision && s.context==context);
        if !current {
            self.stop();
            let mut source=skysight::macos::MacSource::new();
            source.start().map_err(|e|Error::new("computerHistoryInputMonitoringRequired",e))?;
            // The history policy has already admitted this one native app. The
            // normalizer independently excludes secure/own/system applications.
            let mut recorder=Recorder::new(stream::ObservationSettings{default_behavior:stream::DefaultBehavior::DontObserve,
                rules:vec![stream::ObservationRule{scope:stream::RuleScope::App,bundle_id:Some(context.application.clone()),url_domain:None,observe:true}],
                ..Default::default()},i64::from(std::process::id())).with_scrub(Box::new(|s|crate::memory::redact_prose(s).0));
            recorder.start(skysight::now_secs());
            self.session=Some(Session{source,recorder,generation:state.generation,revision:state.revision,context:context.clone(),started_ms:computer_history::now_ms(),last_snapshot:None,snapshot_at:Instant::now()-Duration::from_secs(2),activity_since_snapshot:true});
        }
        let session=self.session.as_mut().unwrap();
        let Some(observation)=store.observation()? else {self.stop();return Ok(false)};
        let mut output=Vec::new();
        let budget=Instant::now()+Duration::from_millis(150);
        // Resolve queued input before taking another window snapshot. One
        // event per policy/snapshot cycle otherwise falls behind ordinary typing.
        for _ in 0..64 {
            let Some((event,ts))=session.source.next(Duration::from_millis(1)) else {break};
            if ts < (session.started_ms/1000) as i64 || ts>skysight::now_secs()+1 || !observation_allowed(&event,&context) {
                self.stop();
                return Ok(false);
            }
            output.extend(session.recorder.observe(event,ts));
            if Instant::now()>=budget {break;}
        }
        // Flush any buffered typing that has now gone idle; this is the recorder's
        // own buffer (no accessibility call), so it is safe on an otherwise idle tick.
        output.extend(session.recorder.tick_when_drained(skysight::now_secs(),session.source.has_pending()));
        let active=should_reverify(current,!output.is_empty());
        if active {
            // Re-walk accessibility to re-prove focus ONLY when there is something to
            // attribute. An idle tick on the same window skips these guards entirely.
            if focus.identify()?.as_ref()!=Some(&context) || focus.read(&context,CaptureMode::Semantic)?.is_none()
                || !focus.verify(&context)? {self.stop();return Ok(false)}
            // Validate the exact pre-read policy revision before publishing any item.
            if store.bind(observation,context.clone())?.is_none() {self.stop();return Ok(false)}
            if !output.is_empty() { session.activity_since_snapshot=true; }
            for item in output {
                let Output::Record(record)=item else {continue};
                if record.pid!=Some(context.process_id) || record.window_id.map(i64::from)!=Some(context.window_id)
                    || record.bundle_id.as_deref()!=Some(&context.application) || record.url.is_some() {continue}
                let kind=record.kind.clone();let payload=record_payload(record);
                Self::commit(store,&context,session,&kind,payload,sink)?;
            }
        }
        // A post-interaction snapshot supplies labels and outcomes that a focused
        // field alone omits. Taken only after activity (and never for terminals),
        // so an idle window is not re-walked over accessibility every second.
        if session.activity_since_snapshot && session.snapshot_at.elapsed()>=Duration::from_secs(1) && !stream::is_terminal(Some(&context.application)) {
            session.snapshot_at=Instant::now();
            session.activity_since_snapshot=false;
            let Some(pre_read)=store.observation()? else {return Ok(active)};
            if let Some(content)=computer_history::macos::native_window_text(&context)? {
                let raw=content.value.unwrap_or_default();
                let clean=crate::history::clean_window_snapshot(&session.recorder,&raw);
                let incomplete=raw.contains("[Partial accessible window snapshot]") || clean.chars().count()>32768;
                let payload=json!({"capture_mode":"interaction_events","secure":false,"role":"AXWindowSnapshot","label":"Native window context",
                    "value":clean.chars().take(32768).collect::<String>(),"capture_incomplete":incomplete,
                    "truncated":{"role":false,"label":false,"value":incomplete}});
                if store.bind(pre_read,context.clone())?.is_some() && session.last_snapshot.as_ref()!=Some(&payload) {
                    Self::commit(store,&context,session,"ui.snapshot",payload.clone(),sink)?;
                    session.last_snapshot=Some(payload);
                }
            }
        }
        Ok(active)
    }
    fn commit(store:&Store,context:&Context,session:&Session,kind:&str,payload:Value,
              sink:&impl Fn(&Context,u64,&Value)->std::result::Result<(),String>)->Result<()> {
        let state=store.state()?;
        if state.generation!=session.generation || state.revision!=session.revision || !eligible_state(&state,context) {return Ok(())}
        let mut identity=computer_history::macos::MacSource::default();
        if identity.identify()?.as_ref()!=Some(context) {return Ok(())}
        let Some(observation)=store.observation()? else {return Ok(())};
        if !observation.matches_revision(&session.generation,session.revision) {return Ok(())}
        if let Some(ticket)=store.bind(observation,context.clone())? {
            let timestamp=ticket.timestamp();
            store.commit_with(ticket,context,kind,payload.clone(),||sink(context,timestamp,&payload).map_err(|e|Error::new("computerHistoryMemoryWrite",e)))?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn idle_sampling_backs_off_and_skips_the_accessibility_reverify() {
        // An idle tick (focus unchanged, nothing drained/flushed) must NOT run
        // the post-drain identify/read/verify guard.
        assert!(!should_reverify(true, false), "idle same-window tick must not re-walk AX");
        // It still runs when an event was processed, or the focus generation changed.
        assert!(should_reverify(true, true), "a drained/flushed event must re-verify");
        assert!(should_reverify(false, false), "a focus-generation change must re-verify");

        // Idle ticks ramp the sleep; any activity snaps it back to fast polling.
        let mut idle = 0u32;
        assert_eq!(idle_backoff(idle), POLL_FAST);
        for _ in 0..20 { idle = next_idle_ticks(idle, false); }
        assert_eq!(idle_backoff(idle), Duration::from_millis(250), "sustained idle reaches the cap");
        assert_eq!(idle_backoff(next_idle_ticks(idle, true)), POLL_FAST, "activity returns to fast polling");

        // Over 10s with no input, the backed-off loop wakes far fewer times than
        // the old fixed 50 Hz (~500 ticks, each of which re-walked accessibility).
        let mut elapsed = Duration::ZERO;
        let mut ticks = 0u32;
        let mut wakeups = 0u32;
        while elapsed < Duration::from_secs(10) {
            elapsed += idle_backoff(ticks);
            ticks = next_idle_ticks(ticks, false);
            wakeups += 1;
        }
        assert!(wakeups < 60, "10s idle woke {wakeups} times, must be far below the old ~500");
    }
    #[test]
    fn typing_payload_marks_its_value_as_a_fragment() {
        let payload=record_payload(skysight::EventRecord{kind:stream::TEXT_INPUT.into(),text:Some("partial".into()),..Default::default()});
        assert_eq!(payload["value_kind"],"typing_fragment");
        let field=record_payload(skysight::EventRecord{kind:"ui.value".into(),text:Some("complete".into()),..Default::default()});
        assert!(field["value_kind"].is_null());
    }
    #[test]
    fn buffered_typing_keeps_input_time_separate_from_publication() {
        let payload=record_payload(skysight::EventRecord {
            kind:stream::TEXT_INPUT.into(),ts:"2026-10-15T22:00:09Z".into(),
            observed_at:Some("2026-10-15T22:00:01Z".into()),..Default::default()
        });
        assert_eq!(payload["source_timestamp"],"2026-10-15T22:00:01Z");
        assert_eq!(payload["normalizer_timestamp"],"2026-10-15T22:00:09Z");
    }
    #[test]
    fn split_typing_bursts_do_not_clear_a_credential_fragment() {
        let mut recorder=Recorder::new(stream::ObservationSettings::default(),99)
            .with_scrub(Box::new(|text|crate::memory::redact_prose(text).0));
        recorder.start(100);
        recorder.observe(Observation::Focus {
            app:stream::AppInfo{bundle_id:Some("dev.fixture.editor".into()),name:"Editor".into(),pid:42,..Default::default()},
            window:Some(stream::WindowInfo{id:7,..Default::default()}),
        },100);
        let target=Some(stream::Target{role:Some("AXTextField".into()),label:Some("Note".into()),pid:Some(42),native_window_id:Some(7),..Default::default()});
        let mut outputs=Vec::new();
        for (at,body) in [(101,"My password is "),(111,"PrivateCanary73")] {
            for c in body.chars() {
                outputs.extend(recorder.observe(Observation::Key{chars:c.to_string(),key:stream::Key::Char,
                    mods:stream::Mods::default(),target:target.clone(),secure_input:false,key_name:String::new()},at));
            }
            outputs.extend(recorder.tick(at+stream::IDLE_FLUSH_SECS+1));
        }
        assert!(outputs.iter().filter_map(|out|out.record()).any(|record|record.kind==stream::TEXT_INPUT));
        assert!(!outputs.iter().filter_map(|out|out.record()).any(|record|record.text.as_deref().is_some_and(|text|text.contains("PrivateCanary73"))),
            "A typing fragment lacks the earlier field content needed to clear shape ambiguity");
    }

    #[test]
    fn terminal_output_preserves_later_lines_after_a_hidden_password() {
        let mut recorder=Recorder::new(stream::ObservationSettings::default(),1)
            .with_scrub(Box::new(|text|crate::memory::redact_prose(text).0));
        recorder.start(100);
        recorder.observe(Observation::Focus {
            app:stream::AppInfo{bundle_id:Some("com.googlecode.iterm2".into()),name:"iTerm".into(),pid:42,..Default::default()},
            window:Some(stream::WindowInfo{id:7,title:Some("bash".into()),..Default::default()})
        },100);
        recorder.observe(Observation::TerminalValue{window_id:7,value:"bash$ ".into()},100);
        let output=recorder.observe(Observation::TerminalValue{window_id:7,
            value:"bash$ read value\nPassword: violet lantern ninety one\nbash$ echo spruce terminal after visible\nspruce terminal after visible\nbash$ ".into()},101);
        let text=output.iter().filter_map(|item|item.record()).filter_map(|r|r.text.as_deref()).collect::<Vec<_>>().join("\n");
        assert!(!text.contains("violet")&&!text.contains("lantern"),"Secret reached terminal output: {text}");
        assert!(text.contains("spruce terminal after visible"),"Unrelated terminal lines disappeared: {text}");
    }
    #[test]
    fn sampled_terminal_response_keeps_safe_padded_output_with_host_scrub() {
        let mut recorder=Recorder::new(stream::ObservationSettings::default(),99)
            .with_scrub(Box::new(|text|crate::memory::redact_prose(text).0));
        recorder.start(100);
        recorder.observe(Observation::Focus {
            app:stream::AppInfo{bundle_id:Some("xfce4-terminal".into()),name:"Terminal".into(),pid:42,..Default::default()},
            window:Some(stream::WindowInfo{id:7,title:Some("History terminal".into()),..Default::default()})
        },100);
        let observe=|r:&mut Recorder,value:String|r.observe(Observation::TerminalValue{window_id:7,value},101);
        observe(&mut recorder,String::new());
        let prefix="Harmless native terminal output 82\nPassword:\n";
        let padding="\n".repeat(24);
        let mut output=observe(&mut recorder,format!("{prefix}{padding}"));
        let response="violet lantern ninety one";
        for (i,_) in response.char_indices().skip(1) {
            output.extend(observe(&mut recorder,format!("{prefix}{}{padding}",&response[..i])));
        }
        output.extend(observe(&mut recorder,format!("{prefix}{response}{padding}")));
        output.extend(observe(&mut recorder,format!("{prefix}{response}\nSafe native terminal recovery 82\nsession-test@fixture:~$ {padding}")));
        let text=output.iter().filter_map(|o|o.record()).filter_map(|r|r.text.as_deref()).collect::<Vec<_>>().join("\n");
        assert!(!text.contains(response),"Prompt response reached host output");
        assert!(text.contains("Safe native terminal recovery 82"),"Safe terminal output disappeared: {text:?}");
    }
    #[test]
    fn interaction_events_require_current_native_ancestry_and_no_secure_input() {
        let context=Context{application:"test.app".into(),process_id:42,window_id:7,website:Website::NotWeb};
        let target=Target{role:Some("AXButton".into()),label:Some("Export".into()),pid:Some(42),native_window_id:Some(7),..Default::default()};
        let click=|t|Observation::Click{button:stream::Button::Left,count:1,ctrl:false,target:Some(t)};
        assert!(observation_allowed(&click(target.clone()),&context));
        for t in [Target{native_window_id:None,..target.clone()},Target{native_window_id:Some(8),..target.clone()},Target{pid:Some(99),..target.clone()},Target{secure:true,..target.clone()}] {
            assert!(!observation_allowed(&click(t),&context));
        }
    }
    #[test]
    fn browser_unknown_origin_and_unapproved_scopes_never_start_events() {
        let context=Context{application:"test.app".into(),process_id:42,window_id:7,website:Website::NotWeb};
        let mut state=State{version:1,generation:"test".into(),revision:1,enabled:true,paused:false,retention_days:7,
            settings:computer_history::policy::Settings::default(),capture_mode:CaptureMode::InteractionEvents,browser_capture:true,summary_epoch:String::new()};
        assert!(!eligible_state(&state,&context));
        state.settings.observation.allowlist.push(computer_history::policy::Rule::App{bundle_id:"test.app".into()});
        assert!(eligible_state(&state,&context));
        for website in [Website::Unknown,Website::Url("https://allowed.example".into())] {
            assert!(!eligible_state(&state,&Context{website,..context.clone()}));
        }
        state.capture_mode=CaptureMode::Values;assert!(!eligible_state(&state,&context));
        state.capture_mode=CaptureMode::InteractionEvents;state.paused=true;assert!(!eligible_state(&state,&context));
    }
    #[test]
    fn event_epoch_cannot_adopt_a_post_delete_or_resumed_revision() {
        let root=std::env::temp_dir().join(format!("icu-event-epoch-{}",std::process::id()));
        let _=std::fs::remove_dir_all(&root);
        let store=Store::open(root.clone()).unwrap();
        store.configure(true,7,true).unwrap();
        let old=store.state().unwrap();
        assert!(store.observation().unwrap().unwrap().matches_revision(&old.generation,old.revision));
        store.pause(true).unwrap();store.pause(false).unwrap();
        assert!(!store.observation().unwrap().unwrap().matches_revision(&old.generation,old.revision));
        let before_clear=store.state().unwrap();store.clear().unwrap();
        assert!(store.observation().unwrap().is_none());
        store.pause(false).unwrap();
        assert!(!store.observation().unwrap().unwrap().matches_revision(&before_clear.generation,before_clear.revision));
        std::fs::remove_dir_all(root).unwrap();
    }
}
