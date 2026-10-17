//! Typed action failures. UI strings are a presentation, never retry evidence.
use action_outcomes::{Disposition, DriverError};
use serde_json::{json, Value};
use std::fmt;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ActionFailure {
    Driver(DriverError),
    LocalRefusal { code: String, message: String },
    Unknown(String),
}

impl ActionFailure {
    pub fn refused(code: &str, message: impl Into<String>) -> Self {
        Self::LocalRefusal {
            code: code.into(),
            message: message.into(),
        }
    }
    pub fn code(&self) -> Option<&str> {
        match self {
            Self::Driver(e) => e.code(),
            Self::LocalRefusal { code, .. } => Some(code),
            _ => None,
        }
    }
    pub fn effect(&self) -> Option<&str> {
        match self {
            Self::Driver(e) => e.reported_effect(),
            Self::LocalRefusal { .. } => Some("none"),
            _ => None,
        }
    }
    pub fn message(&self) -> &str {
        match self {
            Self::Driver(e) => {
                // Presentation only: remove a redundant prefix when it names
                // the already-typed code. This never supplies code/effect facts.
                e.code().and_then(|code| e.message.strip_prefix(&format!("refused ({code}): ")))
                    .unwrap_or(&e.message)
            }
            Self::LocalRefusal { message, .. } | Self::Unknown(message) => message,
        }
    }
    pub fn is_refused(&self) -> bool {
        match self {
            Self::Driver(e) => e.disposition() == Disposition::Refused,
            Self::LocalRefusal { .. } => true,
            _ => false,
        }
    }
    /// Forward known delivery fields to the event consumer. The complete
    /// diagnostic payload stays on DriverError and is not copied into events.
    pub fn evidence(&self) -> Value {
        let mut result = json!({"origin":match self { Self::Driver(_) => "driver", Self::LocalRefusal { .. } => "local", Self::Unknown(_) => "unknown" }});
        if let Some(code) = self.code() {
            result["code"] = json!(code);
        }
        if let Some(effect) = self.effect() {
            result["effect"] = json!(effect);
        }
        if let Self::Driver(error) = self {
            if let Some(status) = error.status() {
                result["status"] = json!(status);
            }
            if let Some(delivery) = error.delivery() {
                let mut fields = json!({});
                for key in ["mode", "delivered_count"] {
                    if let Some(value) = delivery.get(key) { fields[key] = value.clone(); }
                }
                result["delivery"] = fields;
            }
            for key in [
                "delivered_chars",
                "delivered_count",
                "action_performed",
                "action_attempted",
                "verified",
            ] {
                if let Some(value) = error.structured.as_ref().and_then(|s| s.get(key)) {
                    result[key] = value.clone();
                }
            }
        }
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn event_delivery_omits_unrelated_fields_and_retains_raw_driver_error() {
        let raw = json!({"effect":"partial","delivery":{"mode":"background","delivered_count":2,"text":"private input"}});
        let error = ActionFailure::Driver(DriverError { message:"native error".into(), structured:Some(raw.clone()) });
        assert_eq!(error.evidence()["delivery"], json!({"mode":"background","delivered_count":2}));
        let ActionFailure::Driver(driver) = error else { unreachable!() };
        assert_eq!(driver.structured, Some(raw));
    }
}

impl From<String> for ActionFailure {
    fn from(message: String) -> Self {
        Self::Unknown(message)
    }
}
impl From<&str> for ActionFailure {
    fn from(message: &str) -> Self {
        Self::Unknown(message.into())
    }
}
impl From<DriverError> for ActionFailure {
    fn from(error: DriverError) -> Self {
        Self::Driver(error)
    }
}

impl fmt::Display for ActionFailure {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unknown(message) | Self::LocalRefusal { message, .. } => f.write_str(message),
            Self::Driver(error) => {
                let message = &error.message;
                let code = error.code().filter(|c| {
                    !(message.starts_with(&format!("refused ({c})"))
                        || message.starts_with(&format!("{c} ("))
                        || message.starts_with(&format!("{c}:"))
                        || message == c)
                });
                let tag = match (code, error.reported_effect()) {
                    (Some(code), Some(effect)) => format!(" ({code}; effect {effect})"),
                    (Some(code), None) => format!(" ({code})"),
                    (None, Some(effect)) => format!(" (effect {effect})"),
                    (None, None) => String::new(),
                };
                write!(f, "daemon tool error{tag}: {message}")
            }
        }
    }
}
