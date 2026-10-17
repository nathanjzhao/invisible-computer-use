//! Linux AT-SPI and XInput2 metadata under the existing History collector lease.
use computer_history::{
    capture::Source,
    linux::{EventKind, Events, LinuxSource},
    linux_input::{Input, InputSource},
    linux_gnome_input::GnomePointerSource,
    policy::{Context, Decision, Website},
    store::{CaptureMode, State, Store},
    Error, Result,
};
use serde_json::{json, Value};
use skysight::stream::{self, Observation, Output, Recorder, Target};
use std::time::{Duration, Instant};

enum NativeInput {
    X11(InputSource),
    Gnome(GnomePointerSource),
    Kwin(InputSource),
}
impl NativeInput {
    fn drain(&mut self) -> Result<(Vec<computer_history::linux_input::Event>,bool)> {
        match self { Self::X11(source) | Self::Kwin(source)=>source.drain(),Self::Gnome(source)=>source.drain() }
    }
    fn keyboard_refusal(&self) -> Option<Error> {
        match self { Self::X11(source) | Self::Kwin(source)=>source.keyboard_refusal(),Self::Gnome(source)=>source.keyboard_refusal() }
    }
}
struct Session {
    input: Option<NativeInput>,
    events: Events,
    recorder: Recorder,
    context: Context,
    scope_key: Option<String>,
    generation: String,
    revision: u64,
    snapshot_at: Instant,
    last_snapshot: Option<Value>,
    last_value: Option<Value>,
    partial: bool,
    input_targets: Vec<(computer_history::linux::ObjectKey, u64)>,
}
#[derive(Default)]
pub struct Collector {
    source: LinuxSource,
    session: Option<Session>,
    pruned: Option<Instant>,
    incomplete: bool,
    retry_at: Option<Instant>,
    failures: u32,
}
fn allowed(state: &State, context: &Context) -> bool {
    state.enabled
        && !state.paused
        && state.capture_mode == CaptureMode::InteractionEvents
        && context.process_id != i64::from(std::process::id())
        && context.website == Website::NotWeb
        && state.settings.evaluate(context) == Decision::Allowed
}
fn target(content: computer_history::capture::Content, context: &Context) -> Option<Target> {
    Some(Target {
        pid: Some(context.process_id),
        native_window_id: Some(u32::try_from(context.window_id).ok()?),
        role: Some(content.role),
        label: content.label,
        ..Default::default()
    })
}
// Seed only Linux's terminal baseline, after consent and verified native focus.
// The first real screen still takes the recorder's complete stateful redaction path.
fn initialize_terminal(recorder: &mut Recorder, application: &str, window: u32) {
    if stream::is_terminal(Some(application)) {
        recorder.observe(
            Observation::TerminalValue {
                window_id: window,
                value: String::new(),
            },
            skysight::now_secs(),
        );
    }
}
#[cfg(test)]
static SESSION_STARTS: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
impl Collector {
    pub fn refusal(&self) -> Option<&str> {
        self.source.refusal()
    }
    pub fn stop(&mut self) {
        self.session = None;
    }
    pub fn sample(
        &mut self,
        store: &Store,
        sink: &impl Fn(&Context, u64, &Value) -> std::result::Result<(), String>,
    ) -> Result<()> {
        if self.retry_at.is_some_and(|at| Instant::now() < at) {
            return Ok(());
        }
        match self.sample_inner(store, sink) {
            Ok(()) => {
                self.failures = 0;
                self.retry_at = None;
                Ok(())
            }
            Err(e)
                if e.code.starts_with("computerHistoryAccessibility")
                    || e.code == "computerHistoryTreeBudget"
                    || e.code == "computerHistoryAmbiguousFocus"
                    || e.code == "computerHistoryEventsLost"
                    || e.code == "computerHistoryWindowChanged"
                    || e.code == "computerHistoryInputScopeChanged" =>
            {
                if e.code == "computerHistoryInputScopeChanged" {
                    self.incomplete = true;
                    self.stop();
                }
                self.source.note_refusal(&e);
                self.failures = self.failures.saturating_add(1);
                self.retry_at = Some(
                    Instant::now() + Duration::from_millis(250 * (1u64 << self.failures.min(3))),
                );
                Ok(())
            }
            Err(e) => Err(e),
        }
    }
    fn sample_inner(
        &mut self,
        store: &Store,
        sink: &impl Fn(&Context, u64, &Value) -> std::result::Result<(), String>,
    ) -> Result<()> {
        if self
            .pruned
            .is_none_or(|at| at.elapsed() >= Duration::from_secs(30))
        {
            store.prune(computer_history::now_ms())?;
            self.pruned = Some(Instant::now());
        }
        let state = store.state()?;
        if !state.enabled || state.paused || state.capture_mode != CaptureMode::InteractionEvents {
            self.stop();
            return Ok(());
        }
        let Some(context) = self.source.identify()? else {
            self.stop();
            return Ok(());
        };
        if !allowed(&state, &context) {
            self.stop();
            return Ok(());
        }
        // Whole-window capture and point-bound clicks have their own security
        // checks. A transient, protected or unknown keyboard focus must not
        // discard their already queued metadata. No focused target means no
        // attributed key record; focused values independently revalidate scope.

        let window = u32::try_from(context.window_id).map_err(|_| {
            Error::new(
                "computerHistoryWindowUnsupported",
                "The native window identifier exceeds this recorder's range",
            )
        })?;
        let same = self.session.as_ref().is_some_and(|s| {
            s.context == context
                && s.scope_key == self.source.scope_key()
                && s.generation == state.generation
                && s.revision == state.revision
        });
        let mut output = Vec::new();
        if !same {
            self.stop();
            let input = if computer_history::linux::is_wayland() {
                // Older or unsupported helpers retain the existing explicitly
                // incomplete content capture; they do not invent input events.
                match GnomePointerSource::start(&mut self.source,&context) {
                    Ok(Some(source))=>Some(NativeInput::Gnome(source)),
                    Ok(None)=>match InputSource::start_kwin(&mut self.source,&context) {
                        Ok(source)=>Some(NativeInput::Kwin(source)),
                        Err(error)=>{self.source.note_refusal(&error);None}
                    },
                    Err(error)=>{self.source.note_refusal(&error);None}
                }
            } else {
                Some(NativeInput::X11(InputSource::start(&mut self.source, &context)?))
            };
            let events = self.source.start_events(&context)?;
            let mut recorder = Recorder::new(
                stream::ObservationSettings {
                    default_behavior: stream::DefaultBehavior::DontObserve,
                    rules: vec![stream::ObservationRule {
                        scope: stream::RuleScope::App,
                        bundle_id: Some(context.application.clone()),
                        url_domain: None,
                        observe: true,
                    }],
                    ..Default::default()
                },
                i64::from(std::process::id()),
            )
            .with_scrub(Box::new(|s| crate::memory::redact_prose(s).0));
            recorder.start(skysight::now_secs());
            output.extend(recorder.observe(
                Observation::Focus {
                    app: stream::AppInfo {
                        bundle_id: Some(context.application.clone()),
                        name: context.application.clone(),
                        pid: context.process_id,
                        ..Default::default()
                    },
                    window: Some(stream::WindowInfo {
                        id: window,
                        ..Default::default()
                    }),
                },
                skysight::now_secs(),
            ));
            initialize_terminal(&mut recorder, &context.application, window);
            #[cfg(test)]
            SESSION_STARTS.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            self.session = Some(Session {
                input,
                events,
                recorder,
                context: context.clone(),
                scope_key: self.source.scope_key(),
                generation: state.generation,
                revision: state.revision,
                snapshot_at: Instant::now() - Duration::from_secs(2),
                last_snapshot: None,
                last_value: None,
                partial: self.incomplete || computer_history::linux::is_wayland(),
                input_targets: Vec::new(),
            });
        }
        let session = self.session.as_mut().unwrap();
        let authorized = || {
            store.state().is_ok_and(|current| {
                current.generation == session.generation
                    && current.revision == session.revision
                    && allowed(&current, &context)
            })
        };
        if !authorized() {
            self.stop();
            return Ok(());
        }
        let (events, lost) = session.events.drain();
        self.source.focus_events(&events, lost);
        let (inputs, input_lost) = session
            .input
            .as_mut()
            .map(NativeInput::drain)
            .transpose()?
            .unwrap_or_default();
        let keyboard_refusal = session.input.as_ref().and_then(NativeInput::keyboard_refusal);
        if keyboard_refusal.is_some() {
            self.incomplete = true;
            session.partial = true;
        }
        if input_lost {
            self.incomplete = true;
            session.partial = true;
            session.recorder.discard_input_attribution();
        }
        if lost || events.iter().any(|e| e.kind == EventKind::Window) {
            self.incomplete = true;
            session.partial = true;
            session.recorder.discard_pending();
            // A gap or activation transition invalidates attribution and terminal state.
            self.stop();
            return Err(Error::new(
                "computerHistoryEventsLost",
                if lost {
                    "AT-SPI events were lost"
                } else if input_lost {
                    "XInput event target was unavailable or delayed"
                } else {
                    "The active accessible window changed"
                },
            ));
        }
        let terminal = stream::is_terminal(Some(&context.application));
        // Terminal text only enters the stateful redaction path. There is no
        // separate focused-value or window-snapshot projection of this content.
        if authorized() && terminal {
            if let Some((text, partial)) = self.source.terminal_text(&context)? {
                session.partial |= partial;
                output.extend(session.recorder.observe(
                    Observation::TerminalValue {
                        window_id: window,
                        value: text,
                    },
                    skysight::now_secs(),
                ));
            }
        }
        for event in inputs {
            if !authorized() {
                break;
            }
            let Some(hit) = event.content(&mut self.source, &context)? else {
                session.partial = true;
                continue;
            };
            session
                .input_targets
                .push((event.object.clone(), event.timestamp_ms));
            let observation = match event.input {
                Input::Click { right } => Observation::Click {
                    button: if right {
                        stream::Button::Right
                    } else {
                        stream::Button::Left
                    },
                    count: 1,
                    ctrl: false,
                    target: target(hit, &context),
                },
                Input::Key {
                    name,
                    ctrl,
                    alt,
                    shift,
                    win,
                } => {
                    let key = match name {
                        "return" => stream::Key::Return,
                        "tab" => stream::Key::Tab,
                        "escape" => stream::Key::Escape,
                        _ => continue,
                    };
                    Observation::Key {
                        chars: String::new(),
                        key,
                        key_name: name.into(),
                        mods: stream::Mods {
                            cmd: win,
                            ctrl,
                            alt,
                            shift,
                        },
                        target: target(hit, &context),
                        secure_input: false,
                    }
                }
            };
            // Event-time device metadata is sampled; exact delivery remains unproven.
            session.partial = true;
            output.extend(
                session
                    .recorder
                    .observe(observation, (event.timestamp_ms / 1000) as i64),
            );
        }
        session
            .input_targets
            .retain(|(_, at)| computer_history::now_ms().saturating_sub(*at) <= 2000);
        output.extend(session.recorder.tick(skysight::now_secs()));
        if !self.source.verify_window(&context)? {
            self.incomplete = true;
            self.stop();
            return Err(Error::new(
                "computerHistoryWindowChanged",
                "Native window or session changed during observation",
            ));
        }
        for item in output {
            let Output::Record(record) = item else {
                continue;
            };
            if record.pid != Some(context.process_id) || record.window_id != Some(window) {
                continue;
            }
            let kind = record.kind.clone();
            let partial = session.partial || record.observation_truncated;
            let payload = json!({"capture_mode":"interaction_events","secure":false,"role":record.role.unwrap_or_else(||"AXInteraction".into()),
                "label":record.label.or(record.title),"value":record.text.or(record.command),"button":record.button,"keys":record.keys,"click_count":record.click_count,
                "source_timestamp":record.ts,"capture_incomplete":partial,"truncated":{"role":false,"label":false,"value":partial}});
            Self::commit(
                &mut self.source,
                store,
                &context,
                &session.generation,
                session.revision,
                &kind,
                payload,
                sink,
            )?;
        }
        if !terminal {
            {
                for event in events.into_iter().filter(|e| e.kind == EventKind::Invoke) {
                    if !authorized() {
                        break;
                    }
                    if session.input_targets.iter().any(|(key, at)| {
                        key == &event.object && at.abs_diff(event.timestamp_ms) <= 500
                    }) {
                        continue;
                    }
                    let timestamp_ms = event.timestamp_ms;
                    let Some(content) = self.source.event_content(&context, &event)? else {
                        session.partial = true;
                        continue;
                    };
                    let label_cut = content
                        .label
                        .as_ref()
                        .is_some_and(|v| v.chars().count() > 512);
                    let id_cut = content
                        .identifier
                        .as_ref()
                        .is_some_and(|v| v.chars().count() > 256);
                    let partial = session.partial || label_cut || id_cut;
                    // AT-SPI confirms a control state change, not its physical origin.
                    // Do not invent a mouse button, click count or shortcut.
                    let payload = json!({"capture_mode":"interaction_events","secure":false,
                        "role":content.role,
                        "label":content.label.map(|v|session.recorder.clean(&v.chars().take(512).collect::<String>())),
                        "identifier":content.identifier.map(|v|session.recorder.clean(&v.chars().take(256).collect::<String>())),
                        "source_timestamp":timestamp_ms/1000,
                        "observation":"provider_state_change",
                        "capture_incomplete":partial,
                        "truncated":{"role":false,"label":label_cut,"identifier":id_cut,"value":false}});
                    Self::commit(
                        &mut self.source,
                        store,
                        &context,
                        &session.generation,
                        session.revision,
                        "ui.invoke",
                        payload,
                        sink,
                    )?;
                }
            }
            // Keep the observed field state, not a guessed sequence of typed
            // characters. This also represents paste and replacement faithfully.
            if authorized() {
                if let Some(content) = self.source.read(&context, CaptureMode::Values)? {
                    if let Some(raw) = content.value {
                        let value_cut = raw.chars().count() > 2048;
                        let label_cut = content
                            .label
                            .as_ref()
                            .is_some_and(|s| s.chars().count() > 512);
                        let id_cut = content
                            .identifier
                            .as_ref()
                            .is_some_and(|s| s.chars().count() > 256);
                        let partial = session.partial || value_cut || label_cut || id_cut;
                        let clean_value = crate::memory::clean_observed_value(
                            content.label.as_deref().unwrap_or(""),
                            &raw.chars().take(2048).collect::<String>(),
                            &|text| session.recorder.clean(text),
                        );
                        let payload = json!({"capture_mode":"interaction_events","secure":false,
                        "role":content.role,"label":content.label.map(|s|session.recorder.clean(&s.chars().take(512).collect::<String>())),
                        "identifier":content.identifier.map(|s|session.recorder.clean(&s.chars().take(256).collect::<String>())),"value":clean_value,
                        "capture_incomplete":partial,"truncated":{"role":false,"label":label_cut,"identifier":id_cut,"value":value_cut},
                        "observation":"focused_field_state"});
                        if self.source.verify(&context)?
                            && session.last_value.as_ref() != Some(&payload)
                        {
                            Self::commit(
                                &mut self.source,
                                store,
                                &context,
                                &session.generation,
                                session.revision,
                                "ui.value",
                                payload.clone(),
                                sink,
                            )?;
                            session.last_value = Some(payload);
                        }
                    }
                }
            }
        }
        if authorized() && !terminal && session.snapshot_at.elapsed() >= Duration::from_millis(500)
        {
            session.snapshot_at = Instant::now();
            if let Some(content) = self.source.native_window_text(&context)? {
                let raw = content.value.unwrap_or_default();
                let clean = crate::history::clean_window_snapshot(&session.recorder, &raw);
                let partial = session.partial
                    || raw.contains("[Partial accessible window snapshot]")
                    || clean.chars().count() > 32768;
                let payload = json!({"capture_mode":"interaction_events","secure":false,"role":"AXWindowSnapshot","label":"Native window context",
                    "value":clean.chars().take(32768).collect::<String>(),"capture_incomplete":partial,"truncated":{"role":false,"label":false,"value":partial}});
                if session.last_snapshot.as_ref() != Some(&payload) {
                    Self::commit(
                        &mut self.source,
                        store,
                        &context,
                        &session.generation,
                        session.revision,
                        "ui.snapshot",
                        payload.clone(),
                        sink,
                    )?;
                    session.last_snapshot = Some(payload);
                }
            }
        }
        if let Some(error) = keyboard_refusal {
            self.source.note_refusal(&error);
        }
        Ok(())
    }
    fn commit(
        source: &mut LinuxSource,
        store: &Store,
        context: &Context,
        generation: &str,
        revision: u64,
        kind: &str,
        payload: Value,
        sink: &impl Fn(&Context, u64, &Value) -> std::result::Result<(), String>,
    ) -> Result<()> {
        let state = store.state()?;
        if state.generation != generation || state.revision != revision || !allowed(&state, context)
        {
            return Ok(());
        }
        if !source.verify_window(context)? || (kind == "ui.value" && !source.verify(context)?) {
            return Ok(());
        }
        let Some(observation) = store.observation()? else {
            return Ok(());
        };
        if !observation.matches_revision(generation, revision) {
            return Ok(());
        }
        if let Some(ticket) = store.bind(observation, context.clone())? {
            let at = ticket.timestamp();
            store.commit_with(ticket, context, kind, payload.clone(), || {
                sink(context, at, &payload).map_err(|e| Error::new("computerHistoryMemoryWrite", e))
            })?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn initial_terminal_output_uses_the_redacted_terminal_path() {
        let mut recorder = Recorder::new(stream::ObservationSettings::default(), 99);
        recorder.start(0);
        recorder.observe(
            Observation::Focus {
                app: stream::AppInfo {
                    bundle_id: Some("xfce4-terminal".into()),
                    name: "terminal".into(),
                    pid: 42,
                    ..Default::default()
                },
                window: Some(stream::WindowInfo {
                    id: 7,
                    ..Default::default()
                }),
            },
            0,
        );
        initialize_terminal(&mut recorder, "xfce4-terminal", 7);
        let output = recorder.observe(
            Observation::TerminalValue {
                window_id: 7,
                value: "Harmless first-focus output\nPassword: ordinary-secret-sentinel\n$ ".into(),
            },
            1,
        );
        let text = format!("{output:?}");
        assert!(text.contains("Harmless first-focus output"));
        assert!(!text.contains("ordinary-secret-sentinel"));
        let repeated = recorder.observe(
            Observation::TerminalValue {
                window_id: 7,
                value: "Harmless first-focus output\nPassword: ordinary-secret-sentinel\n$ ".into(),
            },
            2,
        );
        let repeated = format!("{repeated:?}");
        assert!(!repeated.contains("Harmless first-focus output"));
        assert!(!repeated.contains("ordinary-secret-sentinel"));
    }
    #[test]
    fn revoked_paused_browser_and_self_contexts_are_not_admitted() {
        let root =
            std::env::temp_dir().join(format!("icu-h1-policy-{}", computer_history::now_ms()));
        let store = Store::open(root.clone()).unwrap();
        let mut state = store.state().unwrap();
        let mut context = Context {
            application: "fixture".into(),
            process_id: 42,
            window_id: 12,
            website: Website::NotWeb,
        };
        state.enabled = true;
        state.capture_mode = CaptureMode::InteractionEvents;
        state.settings.observation.default_application_behavior =
            computer_history::policy::Behavior::Observe;
        assert!(allowed(&state, &context));
        state.paused = true;
        assert!(!allowed(&state, &context));
        state.paused = false;
        state.enabled = false;
        assert!(!allowed(&state, &context));
        state.enabled = true;
        state.capture_mode = CaptureMode::Semantic;
        assert!(!allowed(&state, &context));
        state.capture_mode = CaptureMode::InteractionEvents;
        context.website = Website::Unknown;
        assert!(!allowed(&state, &context));
        context.website = Website::NotWeb;
        context.process_id = i64::from(std::process::id());
        assert!(!allowed(&state, &context));
        std::fs::remove_dir_all(root).unwrap();
    }
    /// Invoked explicitly in the owned Docker desktops; never enables capture in a normal test run.
    #[test]
    #[ignore = "requires an isolated desktop and ICU_H1_DEV_SCOPE"]
    fn desktop_candidate() {
        let scope = std::env::var("ICU_H1_DEV_SCOPE").expect("explicit dev scope");
        let seconds = std::env::var("ICU_H1_DEV_SECONDS")
            .unwrap()
            .parse::<u64>()
            .unwrap()
            .min(60);
        let root = std::env::temp_dir().join(format!(
            "icu-h1-desktop-{}-{}",
            std::process::id(),
            computer_history::now_ms()
        ));
        let service = crate::history::Service::open(root.clone(), false, false);
        let store = service.store.as_ref().unwrap();
        let mut settings = store.settings().unwrap().0;
        settings.observation.default_application_behavior =
            computer_history::policy::Behavior::Observe;
        store.update(settings, None).unwrap();
        store.configure(true, 7, true).unwrap();
        let state = store.state().unwrap();
        let mode = if scope == "metadata" {
            CaptureMode::ApplicationMetadata
        } else {
            CaptureMode::InteractionEvents
        };
        store
            .capture_mode(mode, true, (&state.generation, state.revision))
            .unwrap();
        service.start(|_, _, _| Ok(()));
        let mut last_error = Value::Null;
        for tick in 0..seconds * 20 {
            std::thread::sleep(Duration::from_millis(50));
            let status = service.status().unwrap();
            let error = status["collector"]["error"].clone();
            if tick % 20 == 0 || error != last_error {
                println!("{}", json!({"heartbeat":status["collector"]}));
            }
            last_error = error;
        }
        service.shutdown();
        println!(
            "{}",
            json!({"session_starts": SESSION_STARTS.load(std::sync::atomic::Ordering::SeqCst)})
        );
        std::thread::sleep(Duration::from_secs(2));
        for entry in std::fs::read_dir(root.join("activity")).unwrap().flatten() {
            if entry.path().extension().is_some_and(|e| e == "jsonl") {
                print!("{}", std::fs::read_to_string(entry.path()).unwrap());
            }
        }
        std::fs::remove_dir_all(root).unwrap();
    }
}
