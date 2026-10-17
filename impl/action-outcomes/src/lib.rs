//! Driver failures stay structured until a caller decides whether another
//! observation is safe. This crate knows no delivery or foreground policy.
use serde_json::{json, Value};

/// Preserve the original payload, including fields an older client does not
/// understand. `None` is a transport error; `Some` is an answered tool error.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DriverError {
    pub message: String,
    pub structured: Option<Value>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Effect<'a> {
    Refused,
    Confirmed,
    Partial,
    Unverifiable,
    SuspectedNoop,
    Unknown(&'a str),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Disposition {
    Refused,
    Uncertain,
}

pub const CLI_ENVELOPE_ENV: &str = "CUA_CLI_RESULT_ENVELOPE";
pub const CLI_ENVELOPE_VERSION: u64 = 1;

/// An input fallback may already have dispatched when stdout arrives. Never
/// reinterpret legacy output or retry if the versioned envelope is absent.
pub fn cli_tool_result(stdout: &str) -> Result<Value, &'static str> {
    let envelope: Value = serde_json::from_str(stdout).map_err(|_| "CLI returned no valid result envelope")?;
    if envelope.get("cua_cli_result_envelope").and_then(Value::as_u64) != Some(CLI_ENVELOPE_VERSION) {
        return Err("CLI result envelope is missing or unsupported");
    }
    let result = envelope.get("result").filter(|value| value.is_object()).ok_or("CLI result envelope has no tool result")?;
    if result.get("content").and_then(Value::as_array).is_none()
        || result.get("isError").is_some_and(|value| !value.is_boolean())
    {
        return Err("CLI result envelope contains a malformed tool result");
    }
    Ok(result.clone())
}

/// Legacy native codes that are issued before dispatch. A code without an
/// explicit no-effect marker is sufficient only for this closed set. New or
/// post-dispatch codes fail closed until the driver supplies effect evidence.
const PRE_DISPATCH_CODES: &[&str] = &[
    "ambiguous_capture",
    "background_unavailable",
    "element_not_visible",
    "foreground_unavailable",
    "invalid_argument",
    "invalid_arguments",
    "missing_target",
    "permission_denied",
    "permission_required",
    "point_unmapped",
    "set_value_out_of_range",
    "set_value_unsupported",
    "stale_capture",
    "stale_element_target",
    "stale_element_token",
    "stale_window_target",
    "surface_identity_unproven",
    "system_hotkey_refused",
    "system_menu_refused",
    "type_text_target_not_focused",
    "window_id_not_found",
    "window_minimized",
    "window_target_mismatch",
    "window_target_not_found",
];

impl DriverError {
    /// A top-level field, even malformed, takes precedence over the nested
    /// refusal. Contradictory top-level evidence cannot become a safe retry.
    fn field(&self, name: &str) -> Option<&Value> {
        let payload = self.structured.as_ref()?;
        payload
            .get(name)
            .or_else(|| payload.get("refusal")?.get(name))
    }

    pub fn code(&self) -> Option<&str> {
        self.field("code")?.as_str()
    }
    pub fn status(&self) -> Option<&str> {
        self.field("status")?.as_str()
    }
    pub fn reported_effect(&self) -> Option<&str> {
        self.field("effect")?.as_str()
    }
    pub fn delivery(&self) -> Option<&Value> {
        self.field("delivery")
    }
    pub fn effect(&self) -> Option<Effect<'_>> {
        Some(match self.reported_effect()? {
            "none" | "refused" => Effect::Refused,
            "confirmed" => Effect::Confirmed,
            "partial" => Effect::Partial,
            "unverifiable" => Effect::Unverifiable,
            "suspected_noop" => Effect::SuspectedNoop,
            other => Effect::Unknown(other),
        })
    }

    pub fn disposition(&self) -> Disposition {
        let Some(payload) = self.structured.as_ref() else {
            return Disposition::Uncertain;
        };
        if payload.get("refusal").is_some_and(|value| !value.is_object())
            || payload.get("code").is_some_and(|value| value.as_str().is_none_or(str::is_empty))
        {
            return Disposition::Uncertain;
        }
        // Any evidence that dispatch happened overrides a refusal label. A
        // delivery count can appear in either the current or legacy shape.
        let rows = [Some(payload), payload.get("refusal"), payload.get("detail"), payload.pointer("/refusal/detail")];
        let has_delivery = rows.into_iter().flatten().any(|row| {
            ["action_performed", "action_attempted"].into_iter()
                .any(|key| row.get(key).is_some_and(|v| v.as_bool() != Some(false)))
                || ["delivered_count", "delivered_chars"].into_iter()
                    .any(|key| row.get(key).is_some_and(|v| v.as_u64() != Some(0)))
                || row.get("delivery").is_some_and(|delivery| {
                    !delivery.is_object()
                        || delivery.get("delivered_count").is_none_or(|v| v.as_u64() != Some(0))
                        || delivery.get("mode").is_some_and(|v| !matches!(v.as_str(), Some("background" | "foreground" | "not_applicable")))
                })
        });
        if has_delivery { return Disposition::Uncertain; }
        // Inspect all supported evidence locations, even if a top-level
        // refusal would otherwise hide a nested partial effect or status.
        for row in rows
            .into_iter()
            .flatten()
        {
            if row
                .get("effect")
                .is_some_and(|v| !matches!(v.as_str(), Some("none" | "refused")))
            {
                return Disposition::Uncertain;
            }
            if row
                .get("status")
                .is_some_and(|v| v.as_str() != Some("refused"))
            {
                return Disposition::Uncertain;
            }
        }
        if payload.get("code").is_some()
            && !payload
                .get("code")
                .and_then(Value::as_str)
                .is_some_and(|code| PRE_DISPATCH_CODES.contains(&code))
            && payload.get("effect").is_none()
        {
            return Disposition::Uncertain;
        }
        if self.effect() == Some(Effect::Refused) {
            return Disposition::Refused;
        }
        if self
            .code()
            .is_some_and(|code| PRE_DISPATCH_CODES.contains(&code))
        {
            return Disposition::Refused;
        }
        // status=refused alone does not establish that no native dispatch was
        // attempted, especially when a helper returned an AX error afterwards.
        Disposition::Uncertain
    }

    /// Structured failures sometimes arrive in a successful MCP envelope.
    /// Normal successful dispatch with an unverifiable effect stays a success;
    /// it is not evidence that the intended UI change was confirmed.
    pub fn from_action_payload(payload: &Value) -> Option<Self> {
        let rows = [Some(payload), payload.get("refusal"), payload.get("detail"), payload.pointer("/refusal/detail")];
        let failed = payload.get("code").is_some() || payload.get("refusal").is_some()
            || rows.into_iter().flatten().any(|row| {
                row.get("status").is_some_and(|value| !matches!(value.as_str(), Some("ok" | "success" | "delivered" | "completed")))
                    || row.get("effect").is_some_and(|value| !matches!(value.as_str(), Some("confirmed" | "unverifiable" | "suspected_noop")))
                    || ["delivered_count", "delivered_chars"].into_iter().any(|key| row.get(key).is_some_and(|value| value.as_u64().is_none()))
                    || ["action_performed", "action_attempted"].into_iter().any(|key| row.get(key).is_some_and(|value| !value.is_boolean()))
                    || row.get("delivery").is_some_and(|delivery| !delivery.is_object()
                        || delivery.get("mode").is_some_and(|mode| !matches!(mode.as_str(), Some("background" | "foreground" | "not_applicable" | "unknown")))
                        || delivery.get("delivered_count").is_some_and(|count| count.as_u64().is_none()))
            });
        failed.then(|| Self {
            message: payload
                .get("message")
                .or_else(|| payload.pointer("/refusal/message"))
                .and_then(Value::as_str)
                .unwrap_or("driver did not complete the action")
                .to_owned(),
            structured: Some(payload.clone()),
        })
    }

    pub fn from_tool_result(result: &Value) -> Option<Self> {
        if result.get("isError").and_then(Value::as_bool) != Some(true) {
            return None;
        }
        // Error text is not a structured evidence channel, even when the
        // entire diagnostic happens to be valid JSON. Keep it as text.
        let payload = result
            .get("structuredContent")
            .cloned()
            .unwrap_or_else(|| json!({"text": text_content(result)}));
        let message = text_content(result)
            .or_else(|| payload.get("message").and_then(Value::as_str))
            .or_else(|| payload.pointer("/refusal/message").and_then(Value::as_str))
            .or_else(|| payload.get("code").and_then(Value::as_str))
            .or_else(|| payload.pointer("/refusal/code").and_then(Value::as_str))
            .unwrap_or("tool invocation failed without an error description");
        Some(Self {
            message: message.chars().take(4096).collect(),
            structured: Some(payload),
        })
    }
}

fn text_content(result: &Value) -> Option<&str> {
    result
        .get("content")?
        .as_array()?
        .iter()
        .filter(|item| item.get("type").and_then(Value::as_str) == Some("text"))
        .filter_map(|item| item.get("text").and_then(Value::as_str))
        .find(|text| !text.trim().is_empty())
}

/// Common MCP payload projection. Capture adapters may add image content
/// afterwards; error interpretation never depends on image or message text.
pub fn tool_payload(result: &Value) -> Value {
    result
        .get("structuredContent")
        .cloned()
        .or_else(|| text_content(result).and_then(|text| serde_json::from_str(text).ok()))
        .unwrap_or_else(|| json!({"text": text_content(result)}))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn error(payload: Value) -> DriverError {
        DriverError::from_tool_result(&json!({"isError":true,"structuredContent":payload,
            "content":[{"type":"text","text":"refused (stale_capture): misleading prose"}]}))
        .unwrap()
    }

    #[test]
    fn nested_preflight_refusal_retains_its_code_and_detail() {
        let raw = json!({"status":"refused","refusal":{"code":"stale_element_token","message":"observe again","detail":{"generation":4}}});
        let failure = error(raw.clone());
        assert_eq!(failure.code(), Some("stale_element_token"));
        assert_eq!(failure.disposition(), Disposition::Refused);
        assert_eq!(failure.structured, Some(raw));
    }

    #[test]
    fn partial_uncertain_and_contradictory_evidence_never_becomes_refused() {
        for payload in [
            json!({"effect":"partial","code":"stale_capture","status":"refused","delivery":{"delivered_count":2}}),
            json!({"status":"uncertain","effect":"unverifiable","code":"action_outcome_unknown"}),
            json!({"status":"refused","code":"cannotComplete"}),
            json!({"status":"refused","code":"attributeUnsupported"}),
            json!({"effect":"future_effect","refusal":{"effect":"none","code":"stale_capture"}}),
            json!({"effect":"none","action_performed":true,"refusal":{"code":"stale_capture"}}),
            json!({"effect":"none","delivery":{"delivered_count":1},"code":"stale_capture"}),
            json!({"status":"failed","refusal":{"status":"refused","code":"stale_capture"}}),
            json!({"code":"unknown_new_error","refusal":{"code":"stale_capture"}}),
            json!({"code":"unknown_new_error","refusal":{"code":"stale_capture","effect":"none"}}),
            json!({"effect":"none","refusal":{"effect":"partial","code":"stale_capture"}}),
            json!({"effect":null,"refusal":{"effect":"none","code":"stale_capture"}}),
        ] {
            assert_eq!(
                error(payload.clone()).disposition(),
                Disposition::Uncertain,
                "{payload}"
            );
        }
    }

    #[test]
    fn unstructured_error_prose_is_never_delivery_evidence() {
        for message in [
            "refused (stale_capture): AX action returned -25204",
            r#"{"code":"stale_capture","effect":"none"}"#,
        ] {
            let failure = DriverError::from_tool_result(&json!({"isError":true,
                "content":[{"type":"text","text":message}]}))
            .unwrap();
            assert_eq!(failure.code(), None);
            assert_eq!(failure.disposition(), Disposition::Uncertain);
        }
    }

    #[test]
    fn a_new_code_with_explicit_no_effect_is_a_refusal() {
        let failure = error(json!({"code":"cancelled","effect":"none"}));
        assert_eq!(failure.code(), Some("cancelled"));
        assert_eq!(failure.effect(), Some(Effect::Refused));
        assert_eq!(failure.disposition(), Disposition::Refused);
    }

    #[test]
    fn successful_unverified_dispatch_is_not_promoted_to_failure_or_confirmation() {
        assert!(DriverError::from_action_payload(
            &json!({"effect":"unverifiable","verified":false,"route":"accessibility"})
        )
        .is_none());
        let partial = DriverError::from_action_payload(
            &json!({"effect":"partial","delivery":{"delivered_count":3}}),
        )
        .unwrap();
        assert_eq!(partial.effect(), Some(Effect::Partial));
        assert_eq!(partial.disposition(), Disposition::Uncertain);
    }

    #[test]
    fn successful_linux_accessibility_dispatch_can_have_unknown_delivery_mode() {
        // The public driver contract explicitly includes Unknown. Linux's
        // legacy AT-SPI SetValue success carries it because it did not measure
        // delivery. The caller still observes the resulting window normally.
        let payload = json!({"effect":"unverifiable","route":"accessibility",
            "delivery":{"mode":"unknown"}});
        assert!(DriverError::from_action_payload(&payload).is_none());
        assert_eq!(tool_payload(&json!({"content":[],"structuredContent":payload})), payload);
        let failed = DriverError::from_tool_result(&json!({"isError":true,
            "content":[],"structuredContent":payload})).unwrap();
        assert_eq!(failed.disposition(), Disposition::Uncertain,
            "An actual failed dispatch with unknown delivery must never become a retry");
        assert!(DriverError::from_action_payload(&json!({"effect":"unverifiable",
            "delivery":{"mode":"unrecognized_future_mode"}})).is_some());
    }

    #[test]
    fn malformed_or_nested_delivery_evidence_never_allows_a_refusal_retry() {
        for evidence in [
            json!({"action_attempted":true}),
            json!({"action_performed":"false"}),
            json!({"delivered_chars":1}),
            json!({"delivered_count":null}),
            json!({"delivery":null}),
            json!({"delivery":{"mode":"background"}}),
            json!({"delivery":{"mode":"unknown","delivered_count":0}}),
            json!({"delivery":{"mode":"background","delivered_count":"0"}}),
            json!({"delivery":{"mode":"background","delivered_count":1}}),
            json!({"effect":"partial"}),
            json!({"status":"uncertain"}),
        ] {
            for location in ["", "/refusal", "/detail", "/refusal/detail"] {
                let mut payload = json!({"code":"stale_capture","effect":"none"});
                match location {
                    "" => payload.as_object_mut().unwrap().extend(evidence.as_object().unwrap().clone()),
                    "/refusal" => payload["refusal"] = evidence.clone(),
                    "/detail" => payload["detail"] = evidence.clone(),
                    _ => payload["refusal"] = json!({"detail":evidence}),
                }
                assert_eq!(error(payload.clone()).disposition(), Disposition::Uncertain, "{payload}");
            }
        }
        let failure = error(json!({"effect":"unverifiable",
            "delivery":{"mode":"unknown","delivered_count":0}}));
        assert_eq!(failure.disposition(), Disposition::Uncertain);
        assert_eq!(error(json!({"status":"refused"})).disposition(), Disposition::Uncertain);
    }

    #[test]
    fn successful_envelope_cannot_hide_malformed_or_nested_failure_evidence() {
        for payload in [
            json!({"effect":"unverifiable","delivered_count":"0"}),
            json!({"effect":"confirmed","delivered_chars":null}),
            json!({"effect":"unverifiable","action_attempted":"false"}),
            json!({"effect":"confirmed","detail":{"effect":"partial"}}),
            json!({"effect":"unverifiable","detail":{"status":"uncertain"}}),
        ] {
            let failure = DriverError::from_action_payload(&payload).unwrap();
            assert_eq!(failure.disposition(), Disposition::Uncertain, "{payload}");
            assert_eq!(failure.structured, Some(payload));
        }
    }

    #[test]
    fn cli_envelope_preserves_tool_result_without_guessing_legacy_shapes() {
        let result = json!({"isError":true,"content":[{"type":"text","text":"native failure"}],
            "structuredContent":{"effect":"partial","delivery":{"delivered_count":2}},
            "future_metadata":{"retained":true}});
        let stdout = json!({"cua_cli_result_envelope":1,"result":result}).to_string();
        assert_eq!(cli_tool_result(&stdout).unwrap(), result);
        for unsupported in [
            result.clone(),
            json!({"cua_cli_result_envelope":2,"result":result}),
            json!({"cua_cli_result_envelope":"1","result":result}),
            json!({"cua_cli_result_envelope":1,"result":{"content":[],"isError":"true"}}),
            json!({"cua_cli_result_envelope":1,"result":{"structuredContent":{"ok":true}}}),
            json!({"code":"stale_capture","effect":"none"}),
        ] {
            assert!(cli_tool_result(&unsupported.to_string()).is_err(), "{unsupported}");
        }
        assert!(cli_tool_result("set_value refused: stale_capture").is_err());
    }
}
