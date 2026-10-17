//! Windows UIA and action metadata under the existing History collector lease.
use computer_history::{
    capture::Source,
    policy::{Context, Decision, Website},
    store::{CaptureMode, State, Store},
    windows::{Invocations, WindowsSource},
    windows_input::{Input, InputSource},
    Error, Result,
};
use serde_json::{json, Value};
use skysight::stream::{self, Observation, Output, Recorder, Target};
use std::time::{Duration, Instant};

struct Session {
    input: InputSource,
    invocations: Option<Invocations>,
    recorder: Recorder,
    context: Context,
    generation: String,
    revision: u64,
    snapshot_at: Instant,
    last_snapshot: Option<Value>,
    last_value: Option<Value>,
    partial: bool,
}
#[derive(Default)]
pub struct Collector {
    source: WindowsSource,
    session: Option<Session>,
    pruned: Option<Instant>,
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
fn key(vk: u32) -> (stream::Key, String) {
    match vk {
        0x0D => (stream::Key::Return, "return".into()),
        0x09 => (stream::Key::Tab, "tab".into()),
        0x1B => (stream::Key::Escape, "escape".into()),
        0x41..=0x5A => (
            stream::Key::Char,
            (vk as u8 as char).to_ascii_lowercase().to_string(),
        ),
        0x30..=0x39 => (stream::Key::Char, (vk as u8 as char).to_string()),
        _ => (stream::Key::Other, format!("key_{vk:02x}")),
    }
}
impl Collector {
    pub fn stop(&mut self) {
        self.session = None;
    }
    pub fn sample(
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
        let (focused, focus_handle) = self.source.focus_target(&context)?.unwrap_or_default();
        let window = u32::try_from(context.window_id).map_err(|_| {
            Error::new(
                "computerHistoryWindowUnsupported",
                "The native window identifier exceeds this recorder's range",
            )
        })?;
        let same = self.session.as_ref().is_some_and(|s| {
            s.context == context && s.generation == state.generation && s.revision == state.revision
        });
        let mut output = Vec::new();
        if !same {
            self.stop();
            let input = InputSource::start(context.process_id, context.window_id)?;
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
            self.session = Some(Session {
                input,
                invocations: if stream::is_terminal(Some(&context.application)) {
                    None
                } else {
                    Some(Invocations::start_with_values(&context, store, (&state.generation, state.revision))?)
                },
                recorder,
                context: context.clone(),
                generation: state.generation,
                revision: state.revision,
                snapshot_at: Instant::now() - Duration::from_secs(2),
                last_snapshot: None,
                last_value: None,
                partial: false,
            });
        }
        let session = self.session.as_mut().unwrap();
        if session.input.take_lost() {
            session.partial = true;
            session.recorder.discard_pending();
        }
        let terminal = stream::is_terminal(Some(&context.application));
        // Terminal text only enters the stateful redaction path. There is no
        // separate focused-value or window-snapshot projection of this content.
        if terminal {
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
        let mut value_boundary_ms = 0;
        for _ in 0..64 {
            let Some(event) = session.input.next() else {
                break;
            };
            if event.process_id != context.process_id || event.window_id != context.window_id {
                session.partial = true;
                continue;
            }
            let now = computer_history::now_ms();
            let age = now.saturating_sub(event.timestamp_ms);
            // A bounded window read can itself take 900 ms. Keep a short
            // backlog while revalidating the exact native target and policy;
            // delayed semantic resolution remains explicitly incomplete.
            if event.timestamp_ms > now || age > 2_000 {
                session.partial = true;
                continue;
            }
            session.partial |= age > 500;
            if matches!(&event.input, Input::Click { .. } | Input::Key { vk: 0x0D | 0x09, .. }) {
                value_boundary_ms = value_boundary_ms.max(event.timestamp_ms);
            }
            let at = (event.timestamp_ms / 1000) as i64;
            let observation = match event.input {
                Input::Click {
                    x,
                    y,
                    right,
                    target: native_target,
                } => {
                    let hit = self.source.target_at(&context, x, y, native_target)?;
                    let Some(hit) = hit else {
                        session.partial = true;
                        continue;
                    };
                    Observation::Click {
                        button: if right {
                            stream::Button::Right
                        } else {
                            stream::Button::Left
                        },
                        count: 1,
                        ctrl: false,
                        target: target(hit, &context),
                    }
                }
                Input::Key {
                    vk,
                    ctrl,
                    alt,
                    shift,
                    win,
                    focus,
                } => {
                    // A virtual UIA element cannot prove where an earlier key
                    // landed. Keep its state snapshots, not attributed keys.
                    if focus_handle == 0
                        || focus != focus_handle
                        || !self.source.verify(&context)?
                    {
                        session.partial = true;
                        continue;
                    }
                    let (key, name) = key(vk);
                    Observation::Key {
                        chars: String::new(),
                        key,
                        mods: stream::Mods {
                            cmd: win,
                            ctrl,
                            alt,
                            shift,
                        },
                        target: target(focused.clone(), &context),
                        secure_input: false,
                        key_name: name,
                    }
                }
            };
            output.extend(session.recorder.observe(observation, at));
        }
        output.extend(session.recorder.tick(skysight::now_secs()));
        if !self.source.verify_window(&context)? {
            self.stop();
            return Ok(());
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
            if let Some(invocations) = &mut session.invocations {
                let (events, partial) = invocations.drain(|| {
                    store.state().is_ok_and(|current| {
                        current.generation == session.generation
                            && current.revision == session.revision
                            && allowed(&current, &context)
                    })
                });
                session.partial |= partial;
                for (timestamp_ms, content) in events {
                    value_boundary_ms = value_boundary_ms.max(timestamp_ms);
                    let label_cut = content
                        .label
                        .as_ref()
                        .is_some_and(|v| v.chars().count() > 512);
                    let id_cut = content
                        .identifier
                        .as_ref()
                        .is_some_and(|v| v.chars().count() > 256);
                    let partial = session.partial || label_cut || id_cut;
                    // UIA confirms an invocation but not its physical origin.
                    // Do not invent a mouse button, click count or shortcut.
                    let payload = json!({"capture_mode":"interaction_events","secure":false,
                        "role":content.role,
                        "label":content.label.map(|v|session.recorder.clean(&v.chars().take(512).collect::<String>())),
                        "identifier":content.identifier.map(|v|session.recorder.clean(&v.chars().take(256).collect::<String>())),
                        "source_timestamp":timestamp_ms/1000,
                        "observation":"provider_invocation",
                        "capture_incomplete":partial,
                        "truncated":{"role":false,"label":label_cut,"identifier":id_cut,"value":false}});
                    Self::commit(
                        store,
                        &context,
                        &session.generation,
                        session.revision,
                        "ui.invoke",
                        payload,
                        sink,
                    )?;
                }
                let (values, partial) = invocations.drain_values(value_boundary_ms);
                session.partial |= partial;
                for value in values {
                    let at = value.ticket.timestamp();
                    let content = value.content;
                    let Some(raw) = content.value else { continue; };
                    let value_cut = raw.chars().count() > 2048;
                    let label_cut = content.label.as_ref().is_some_and(|s| s.chars().count() > 512);
                    let id_cut = content.identifier.as_ref().is_some_and(|s| s.chars().count() > 256);
                    let clean = crate::memory::clean_observed_value(
                        content.label.as_deref().unwrap_or(""),
                        &raw.chars().take(2048).collect::<String>(),
                        &|text| session.recorder.clean(text),
                    );
                    let payload = json!({"capture_mode":"interaction_events","secure":false,
                        "role":content.role,
                        "label":content.label.map(|s|session.recorder.clean(&s.chars().take(512).collect::<String>())),
                        "identifier":content.identifier.map(|s|session.recorder.clean(&s.chars().take(256).collect::<String>())),
                        "value":clean,"observation":"provider_value_change",
                        "source_timestamp":at/1000,"source_timestamp_ms":at,
                        "capture_incomplete":session.partial || value_cut || label_cut || id_cut,
                        "truncated":{"role":false,"label":label_cut,"identifier":id_cut,"value":value_cut}});
                    // The producer already read under this ticket's policy and
                    // owner barrier. Keep its timestamp rather than minting a
                    // later sampling time. commit_with rechecks pause/deletion.
                    store.commit_with(value.ticket, &context, "ui.value", payload.clone(), || {
                        sink(&context, at, &payload).map_err(|error| Error::new("computerHistoryMemoryWrite", error))
                    })?;
                }
            }
            // Keep the observed field state, not a guessed sequence of typed
            // characters. This also represents paste and replacement faithfully.
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
                    let partial = value_cut || label_cut || id_cut;
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
        if !terminal && session.snapshot_at.elapsed() >= Duration::from_millis(500) {
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
        Ok(())
    }
    fn commit(
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
        let mut identity = WindowsSource::default();
        if identity.identify()?.as_ref() != Some(context) || !identity.verify(context)? {
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
