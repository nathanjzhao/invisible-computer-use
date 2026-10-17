use action_outcomes::Disposition;
use icu::{cua::normalize_result, ToolError, CODE_DRIVER_ERROR};
use serde_json::json;

#[test]
fn native_uncertainty_survives_mcp_normalization_and_icu_taxonomy() {
    let raw = json!({"status":"uncertain","effect":"unverifiable","verified":false,
        "code":"action_outcome_unknown","delivery":{"mode":"unknown"},"action_attempted":true});
    let driver = normalize_result(&json!({"isError":true,"structuredContent":raw,
        "content":[{"type":"text","text":"refused (stale_capture): misleading description"}]}))
    .unwrap_err();
    assert_eq!(driver.code(), Some("action_outcome_unknown"));
    assert_eq!(driver.disposition(), Disposition::Uncertain);
    assert_eq!(driver.structured.as_ref(), Some(&raw));
    let mapped = ToolError::from_cua(driver);
    assert_eq!(mapped.code(), Some(CODE_DRIVER_ERROR));
    assert_eq!(mapped.structured["driver_code"], "action_outcome_unknown");
    for key in [
        "status",
        "effect",
        "verified",
        "delivery",
        "action_attempted",
    ] {
        assert_eq!(mapped.structured[key], raw[key], "lost {key}");
    }
}

#[test]
fn nested_refusal_code_and_evidence_survive_icu_mapping() {
    let raw = json!({"status":"refused","refusal":{"code":"stale_capture","message":"window resized","detail":{"generation":2}}});
    let driver = normalize_result(&json!({"isError":true,"structuredContent":raw})).unwrap_err();
    assert_eq!(driver.disposition(), Disposition::Refused);
    let mapped = ToolError::from_cua(driver);
    assert_eq!(mapped.structured["driver_code"], "stale_capture");
    assert_eq!(mapped.structured["refusal"], raw["refusal"]);
    assert_eq!(mapped.structured["status"], "refused");
}

#[test]
fn a_plain_native_error_never_becomes_no_effect_from_its_words() {
    let driver = normalize_result(&json!({"isError":true,
        "content":[{"type":"text","text":"refused (stale_capture): AXPress returned -25204"}]}))
    .unwrap_err();
    assert_eq!(driver.disposition(), Disposition::Uncertain);
    let mapped = ToolError::from_cua(driver);
    assert_eq!(mapped.code(), Some(CODE_DRIVER_ERROR));
    assert!(mapped.structured.get("effect").is_none());
    assert!(mapped.structured.get("driver_code").is_none());
}
