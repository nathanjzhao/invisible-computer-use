//! Model, prompt and run metadata on recorded events (specs/00-COVERAGE.md:169).
//!
//! cua's per-turn `action.json` carries what the driver did (tool, arguments, result,
//! `action_truth`) but not which model asked, for which prompt, in which run. An eval or a replay
//! diff needs that on every recorded turn, so the patched driver's `start_recording` accepts a
//! `run_metadata` object and stamps it into every `turn-NNNNN/action.json` and `evidence.json`.
//! This module builds that object for the layer's `record_start`:
//!
//! - `run` on the call (`{model, prompt, run_id, eval, ...}`), over
//! - `ICU_RUN_METADATA` (a JSON object a host sets once for the `icu mcp` it spawns),
//! - plus `prompt_sha256` when a `prompt` is given, so a diff can compare prompts without storing
//!   them, and the layer's own `icu` provenance (version, approval session, mode).
//!
//! Nothing given means nothing sent: a recording started without metadata is byte-identical to
//! one from before. The driver bounds the object (16 KiB, 4 levels) and writes it in plain text.
use serde_json::{json, Map, Value};

pub struct Provenance<'a> { pub session: &'a str, pub mode: &'a str }

/// The object to send as `run_metadata`, or None when neither the call nor the environment gave any.
pub fn run_metadata(call: Option<&Value>, env_default: Option<&str>, prov: &Provenance) -> Result<Option<Value>, String> {
    let mut merged = Map::new();
    if let Some(raw) = env_default.map(str::trim).filter(|s| !s.is_empty()) {
        match serde_json::from_str::<Value>(raw) {
            Ok(Value::Object(m)) => merged.extend(m),
            _ => return Err("ICU_RUN_METADATA must be a JSON object".into()),
        }
    }
    match call {
        None | Some(Value::Null) => {}
        Some(Value::Object(m)) => merged.extend(m.clone()),
        Some(_) => return Err("`run` must be a JSON object ({model, prompt, run_id, eval, ...})".into()),
    }
    if merged.is_empty() { return Ok(None); }
    if !merged.contains_key("prompt_sha256") {
        if let Some(prompt) = merged.get("prompt").and_then(Value::as_str) {
            merged.insert("prompt_sha256".into(), json!(crate::predicates::sha256_hex_of(prompt.as_bytes())));
        }
    }
    merged.entry("icu").or_insert_with(|| json!({"version": env!("CARGO_PKG_VERSION"), "session": prov.session, "mode": prov.mode}));
    Ok(Some(Value::Object(merged)))
}

#[cfg(test)]
mod tests {
    use super::*;
    const P: Provenance = Provenance { session: "s1", mode: "attended" };

    #[test]
    fn nothing_given_sends_nothing() {
        assert_eq!(run_metadata(None, None, &P).unwrap(), None);
        assert_eq!(run_metadata(Some(&Value::Null), Some("  "), &P).unwrap(), None);
        assert_eq!(run_metadata(Some(&json!({})), None, &P).unwrap(), None, "an empty object is not metadata");
    }

    #[test]
    fn model_prompt_and_run_id_ride_with_the_prompt_hash_and_provenance() {
        let m = run_metadata(Some(&json!({"model": "gpt-5.5", "prompt": "abc", "run_id": "r-7", "eval": {"suite": "osworld", "task": 12}})), None, &P).unwrap().unwrap();
        assert_eq!((m["model"].as_str(), m["run_id"].as_str(), m["eval"]["task"].as_i64()), (Some("gpt-5.5"), Some("r-7"), Some(12)));
        assert_eq!(m["prompt"], "abc", "the prompt is stored as given");
        assert_eq!(m["prompt_sha256"], "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad");
        assert_eq!(m["icu"]["session"], "s1");
        assert_eq!(m["icu"]["mode"], "attended");
    }

    #[test]
    fn the_environment_default_is_overridden_key_by_key_by_the_call() {
        let m = run_metadata(Some(&json!({"model": "call-model"})), Some(r#"{"model": "env-model", "run_id": "env-run"}"#), &P).unwrap().unwrap();
        assert_eq!((m["model"].as_str(), m["run_id"].as_str()), (Some("call-model"), Some("env-run")));
        let only_env = run_metadata(None, Some(r#"{"run_id": "r"}"#), &P).unwrap().unwrap();
        assert_eq!(only_env["run_id"], "r");
    }

    #[test]
    fn malformed_metadata_is_an_error_not_a_silent_drop() {
        assert!(run_metadata(Some(&json!("model")), None, &P).unwrap_err().contains("JSON object"));
        assert!(run_metadata(None, Some("[1]"), &P).unwrap_err().contains("ICU_RUN_METADATA"));
        assert!(run_metadata(None, Some("{not json"), &P).is_err());
    }

    #[test]
    fn an_explicit_prompt_hash_is_kept() {
        let m = run_metadata(Some(&json!({"prompt": "abc", "prompt_sha256": "mine"})), None, &P).unwrap().unwrap();
        assert_eq!(m["prompt_sha256"], "mine");
    }
}
