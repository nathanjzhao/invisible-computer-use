//! Policy-aware sampling. Native adapters never write raw observations to a staging file.
use crate::{
    policy::{Context, Website},
    store::{CaptureMode, Commit, Store},
    Result,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::sync::atomic::{AtomicBool, Ordering};

#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Content {
    pub role: String,
    pub label: Option<String>,
    pub identifier: Option<String>,
    pub value: Option<String>,
}
impl Content {
    fn bounded(mut self, mode: CaptureMode) -> Value {
        let truncated = json!({"role":self.role.chars().count()>128,
            "label":self.label.as_ref().is_some_and(|v|v.chars().count()>512),
            "identifier":self.identifier.as_ref().is_some_and(|v|v.chars().count()>256),
            "value":mode==CaptureMode::Values&&self.value.as_ref().is_some_and(|v|v.chars().count()>2048)});
        self.role = self.role.chars().take(128).collect();
        self.label = self.label.map(|v| v.chars().take(512).collect());
        self.identifier = self.identifier.map(|v| v.chars().take(256).collect());
        self.value = if mode == CaptureMode::Values {
            self.value.map(|v| v.chars().take(2048).collect())
        } else {
            None
        };
        let mut payload = serde_json::to_value(self).expect("content serialization");
        payload["truncated"] = truncated;
        payload["secure"] = json!(false);
        payload["capture_mode"] = json!(mode);
        payload
    }
}
/// Implementations must return None while locked, switched out, or identity is uncertain.
/// `read` must reject secure fields and web content with unknown/private browsing state,
/// before reading labels or values. `verify` rechecks native focus and session state.
pub trait Source {
    fn identify(&mut self) -> Result<Option<Context>>;
    /// Called only after the app rule permits observation. May resolve a browser page.
    fn resolve(&mut self, context: &Context, _browser_capture: bool) -> Result<Option<Context>> {
        Ok(Some(context.clone()))
    }
    fn read(&mut self, context: &Context, mode: CaptureMode) -> Result<Option<Content>>;
    fn verify(&mut self, context: &Context) -> Result<bool>;
}
#[derive(Default)]
pub struct Recorder {
    last: Option<(String, u64, Context, Value)>,
}
impl Recorder {
    pub fn reset(&mut self) {
        self.last = None;
    }
    pub fn sample(
        &mut self,
        store: &Store,
        source: &mut impl Source,
        stop: &AtomicBool,
        sink: impl FnOnce(&Context, u64, &Value) -> Result<()>,
    ) -> Result<Option<Commit>> {
        let Some(observation) = store.observation()? else {
            self.reset();
            return Ok(None);
        };
        if stop.load(Ordering::SeqCst) {
            self.reset();
            return Ok(None);
        }
        let mode = observation.mode;
        if mode == CaptureMode::InteractionEvents {
            return Err(crate::Error::new("computerHistoryEventSourceRequired", "Interaction capture requires the event collector"));
        }
        let Some(context) = source.identify()? else {
            self.reset();
            return Ok(None);
        };
        if !observation.allows_application(&context) {
            self.reset();
            return Ok(None);
        }
        let original = context;
        let Some(context) = source.resolve(&original, observation.browser_capture)? else {
            self.reset();
            return Ok(None);
        };
        if context.application != original.application
            || context.process_id != original.process_id
            || context.window_id != original.window_id
        {
            self.reset();
            return Ok(None);
        }
        // Browser content needs a verified page and private-mode source. Native adapters
        // without that source use Unknown and cannot silently claim full browser coverage.
        if mode != CaptureMode::ApplicationMetadata && matches!(context.website, Website::Unknown) {
            self.reset();
            return Ok(None);
        }
        let Some(ticket) = store.bind(observation, context.clone())? else {
            self.reset();
            return Ok(None);
        };
        let state = store.state()?;
        let timestamp = ticket.timestamp();
        let payload = if mode == CaptureMode::ApplicationMetadata {
            json!({})
        } else {
            let Some(content) = source.read(&context, mode)? else {
                self.reset();
                return Ok(None);
            };
            content.bounded(mode)
        };
        if stop.load(Ordering::SeqCst) || !source.verify(&context)? || stop.load(Ordering::SeqCst) {
            self.reset();
            return Ok(Some(Commit::Stale));
        }
        let key = (
            state.generation,
            state.revision,
            context.clone(),
            payload.clone(),
        );
        if self.last.as_ref() == Some(&key) {
            return Ok(None);
        }
        let kind = if mode == CaptureMode::ApplicationMetadata {
            "app_focus"
        } else {
            "ui.focus"
        };
        let outcome = store.commit_with(ticket, &context, kind, payload.clone(), || {
            sink(&context, timestamp, &payload)
        })?;
        self.last = if outcome == Commit::Written {
            Some(key)
        } else {
            None
        };
        Ok(Some(outcome))
    }
}
