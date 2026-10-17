//! Action approval — the Codex/Sky parity slice for `specs/14-approval-confirmations.md`.
//!
//! Three pieces, each pinned to a shipped artefact:
//!
//! 1. **The Computer Use Confirmations Policy** (model-facing). Two verbatim
//!    copies ship: the Windows packaging (`policy/confirmations-policy.md`, which
//!    is the golden `conformance/golden/confirmations-policy.md` ==
//!    `codex-corpus/windows/confirmations.md`) and the macOS guardian section
//!    (`policy/confirmations-policy-macos.md`, lines 133-198 of
//!    `codex-corpus/mac/ComputerUse/computer-use-node-repl-guardian.md`). The
//!    four modes and the numbered categories are the same in both; the
//!    machine-readable taxonomy below is checked against the golden text by the
//!    conformance suite (an oracle independent of this table).
//! 2. **The per-app approval elicitation** — the exact `createElicitation` shape
//!    from `sky_js/src/targets/mac/computer-use-policy.js` (`withComputerUsePolicy`):
//!    message `Allow Computer Use to use "<displayName>"?`, meta keys, the
//!    `accept/cancel/decline -> accepted/canceled/declined` mapping, the
//!    `session`/`always` persistence read from `_meta.persist`, and the two
//!    org/safety refusal messages.
//! 3. **The on-disk allowlists** exactly as observed on this machine (spec 14):
//!    per-session `~/.codex/computer-use/sessions/<id>.toml` =
//!    `[apps]\nallowed = ["<bid>"]\n`, and the global
//!    `.../ComputerUseAppApprovals.json` = `{"approvedBundleIdentifiers":[...]}`
//!    (compact, no trailing newline).
//!
//! INFERRED (not recoverable) and labelled: the TOML separator for a multi-entry
//! `allowed` list (`", "`; only single-entry files were observed) and that the
//! session file is created on first approval (Codex writes it per session id).
//! Anything else here is the pinned text or code, not a reading of it.
//!
//! Not built (spec 14 addendum, separately packaged): the Windows
//! "Non-negotiable Windows Automation Safety" hard-deny list; the audio
//! elicitation (spec 16 is a non-goal).

pub mod cli;

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};

/// The golden/Windows policy text, verbatim.
pub const POLICY_WINDOWS: &str = include_str!("../policy/confirmations-policy.md");
/// The macOS guardian policy section, verbatim.
pub const POLICY_MACOS: &str = include_str!("../policy/confirmations-policy-macos.md");

/// The Non-negotiable Windows Automation Safety hard-deny list (spec 14 addendum),
/// verbatim from the Windows guidance corpus. This is a hard allow/deny layer ABOVE
/// the four confirmation modes: "These denies are mandatory. Confirmation policy
/// applies only to allowed-but-confirmed actions and cannot replace these denies."
pub const HARD_DENY_WINDOWS: &str = include_str!("../policy/windows-hard-deny.md");

/// Whether a `press_key` / `hotkey` token is the Windows/Meta key, which the hard-deny
/// list forbids: "Never call `press_key` with `Meta`, `Windows`, `Win`, `WIN+...`,
/// `Windows+...`, `WINDOWS+...`, `Meta+...`, `Cmd`, `Command`, `Super`, or `OS` key names."
/// Also catches the X keysym and Win32 spellings of that key (`Super_L`, `Meta_R`,
/// `LWin`, `RWin`). Case-insensitive; this is safety, so it over-matches that one key
/// rather than risk letting a Windows-key chord through.
fn is_windows_meta_key_token(token: &str) -> bool {
    let t = token.trim().to_ascii_lowercase();
    const EXACT: &[&str] = &["meta", "windows", "win", "cmd", "command", "super", "os", "lwin", "rwin"];
    EXACT.contains(&t.as_str())
        || t.starts_with("super_")
        || t.starts_with("meta_")
        || t.starts_with("windows_")
        || t.starts_with("win_")
}

/// Hard-deny check for a `press_key` / `hotkey` key spec (a `+`, space, or comma
/// separated chord). Returns the non-negotiable deny reason if any token is the
/// Windows/Meta key, or `None` if the chord is allowed. Enforced above the
/// confirmation modes; a confirmed action cannot override it.
pub fn press_key_hard_denied(spec: &str) -> Option<&'static str> {
    spec.split(['+', ' ', ','])
        .any(is_windows_meta_key_token)
        .then_some("Windows automation safety: the Windows/Meta key is non-negotiably denied (spec 14 hard-deny); it cannot be confirmed past.")
}

/// Apply the Windows key rule to the target driver's reported platform.
/// Missing or unrecognized platform evidence cannot relax the rule. Callers
/// must obtain this value from the driver, not from action arguments.
pub fn press_key_hard_denied_on_platform(spec: &str, platform: Option<&str>) -> Option<&'static str> {
    if matches!(platform, Some("darwin" | "linux")) { None }
    else { press_key_hard_denied(spec) }
}

/// The four confirmation modes, titled exactly as the policy headings.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Mode {
    HandOffRequired,
    AlwaysConfirm,
    PreApprovalWorks,
    NoConfirmationNeeded,
}

impl Mode {
    /// Verbatim heading text (after the `N) `).
    pub fn title(self) -> &'static str {
        match self {
            Mode::HandOffRequired => "Hand-Off Required (User Must Do It)",
            Mode::AlwaysConfirm => "Always Confirm at Action-Time (Even If Pre-Approved)",
            Mode::PreApprovalWorks => "Pre-Approval Works (Otherwise Treat as \u{201c}Always Confirm\u{201d})",
            Mode::NoConfirmationNeeded => "No Confirmation Needed (Always Allowed)",
        }
    }
    /// The heading number in the policy (1-4).
    pub fn number(self) -> u8 {
        match self {
            Mode::HandOffRequired => 1,
            Mode::AlwaysConfirm => 2,
            Mode::PreApprovalWorks => 3,
            Mode::NoConfirmationNeeded => 4,
        }
    }
    pub const ALL: [Mode; 4] = [Mode::HandOffRequired, Mode::AlwaysConfirm, Mode::PreApprovalWorks, Mode::NoConfirmationNeeded];
}

/// Category tag (the bracketed number in the policy, e.g. `2.4`) -> mode.
/// Ranges/lists in the text (`[2.1, 2.2, 2.5, 2.6]`, `[8.3–8.5]`) are expanded.
pub const TAXONOMY: &[(&str, Mode)] = &[
    ("2.4", Mode::HandOffRequired), ("15", Mode::HandOffRequired),
    ("1", Mode::AlwaysConfirm), ("2.1", Mode::AlwaysConfirm), ("2.2", Mode::AlwaysConfirm),
    ("2.5", Mode::AlwaysConfirm), ("2.6", Mode::AlwaysConfirm), ("4", Mode::AlwaysConfirm),
    ("8.3", Mode::AlwaysConfirm), ("8.4", Mode::AlwaysConfirm), ("8.5", Mode::AlwaysConfirm),
    ("9", Mode::AlwaysConfirm), ("10", Mode::AlwaysConfirm), ("11", Mode::AlwaysConfirm),
    ("13", Mode::AlwaysConfirm), ("17", Mode::AlwaysConfirm),
    ("2.3", Mode::PreApprovalWorks), ("2.7", Mode::PreApprovalWorks), ("3.3", Mode::PreApprovalWorks),
    ("5.1", Mode::PreApprovalWorks), ("6", Mode::PreApprovalWorks), ("12", Mode::PreApprovalWorks),
    ("14", Mode::PreApprovalWorks),
    ("3.1", Mode::NoConfirmationNeeded), ("3.2", Mode::NoConfirmationNeeded), ("7", Mode::NoConfirmationNeeded),
];

/// Mode for a category tag. Unknown tags are "Any action outside this taxonomy"
/// -> No Confirmation Needed (policy section 4).
pub fn mode_for(tag: &str) -> Mode {
    TAXONOMY.iter().find(|(t, _)| *t == tag).map(|(_, m)| *m).unwrap_or(Mode::NoConfirmationNeeded)
}

/// Who authored the instruction that (claims to) permit an action (policy
/// "Types of Instruction").
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum InstructionSource {
    /// Typed by the user in the prompt: valid intent even if high-risk.
    UserAuthored,
    /// Pasted/quoted/uploaded/website content: never permission by itself.
    ThirdPartyContent,
}

impl InstructionSource {
    pub fn grants_permission(self) -> bool { matches!(self, InstructionSource::UserAuthored) }
}

/// What the agent must do before the action.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Decision {
    /// Ask the user to take over or find an alternative.
    HandOff,
    /// Blocking confirmation immediately before the action.
    Confirm,
    /// Proceed without confirming.
    Proceed,
}

/// The mode transition table. `pre_approval` is the source of an explicit
/// permission given in the INITIAL prompt, if any: it only counts for
/// Pre-Approval-Works categories and only when user-authored.
///
/// CALLER OBLIGATION: passing `Some(UserAuthored)` asserts the caller has already
/// judged the permission explicit and specific under the policy's qualifiers —
/// [14] must name the specific data AND destination, the [2.3/2.7] login nuance
/// ("go to xyz.com" implies login there, a redirect elsewhere does not), and
/// Hygiene's "vague asks are not blanket pre-approval". Those are natural-language
/// judgments this function does not make.
pub fn decide(tag: &str, pre_approval: Option<InstructionSource>) -> Decision {
    match mode_for(tag) {
        Mode::HandOffRequired => Decision::HandOff,
        Mode::AlwaysConfirm => Decision::Confirm,
        Mode::PreApprovalWorks => match pre_approval {
            Some(s) if s.grants_permission() => Decision::Proceed,
            _ => Decision::Confirm,
        },
        Mode::NoConfirmationNeeded => Decision::Proceed,
    }
}

// ---- per-app approval elicitation (computer-use-policy.js) --------------------

/// `getAppPolicy(app).decision`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AppPolicyDecision { Allowed, Denied, Forbidden }

/// `getAppPolicy(app).target`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct AppTarget {
    #[serde(rename = "bundleIdentifier")]
    pub bundle_identifier: String,
    #[serde(rename = "displayName")]
    pub display_name: String,
    /// `riskLevel` in the elicitation meta.
    pub risk: String,
    #[serde(rename = "warningSubtitle", skip_serializing_if = "Option::is_none")]
    pub warning_subtitle: Option<String>,
}

/// Verbatim refusal messages for the org/safety decisions.
pub fn app_policy_refusal(decision: AppPolicyDecision, bundle_identifier: &str) -> Option<String> {
    match decision {
        AppPolicyDecision::Allowed => None,
        AppPolicyDecision::Denied => Some(format!("Computer Use is blocked from using the app '{bundle_identifier}' by your organization's policy.")),
        AppPolicyDecision::Forbidden => Some(format!("Computer Use is not allowed to use the app '{bundle_identifier}' for safety reasons.")),
    }
}

/// The elicitation message.
pub fn elicitation_message(display_name: &str) -> String {
    format!("Allow Computer Use to use \"{display_name}\"?")
}

/// The exact `createElicitation({message, meta})` payload (`withComputerUsePolicy`).
pub fn elicitation(target: &AppTarget, allow_persistent_approval: bool, tool_name: &str, tool_call_id: Option<&str>) -> Value {
    let mut meta = serde_json::Map::new();
    meta.insert("codex_approval_kind".into(), json!("mcp_tool_call"));
    meta.insert("connector_id".into(), json!("computer-use"));
    meta.insert("connector_name".into(), json!("Computer Use"));
    meta.insert("persist".into(), if allow_persistent_approval { json!(["session", "always"]) } else { json!(["session"]) });
    meta.insert("riskLevel".into(), json!(target.risk));
    if let Some(s) = &target.warning_subtitle { meta.insert("subtitle".into(), json!(s)); }
    if let Some(id) = tool_call_id { meta.insert("tool_call_id".into(), json!(id)); }
    meta.insert("tool_name".into(), json!(tool_name));
    meta.insert("tool_params".into(), json!({"app": target.bundle_identifier}));
    meta.insert("tool_params_display".into(), json!([{"name": "app", "display_name": "App", "value": target.display_name}]));
    json!({"message": elicitation_message(&target.display_name), "meta": Value::Object(meta)})
}

/// Persistence scope of an accepted approval (`_meta.persist`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Persist { Session, Always }

/// Telemetry `approvalResult` for an elicitation action (`accept/cancel/decline`
/// -> `accepted/canceled/declined`; anything else -> None).
pub fn approval_result(action: &str) -> Option<&'static str> {
    match action {
        "accept" => Some("accepted"),
        "cancel" => Some("canceled"),
        "decline" => Some("declined"),
        _ => None,
    }
}

/// `computer-use-policy.js`: a result whose `content.source` is
/// `"computer-use-persisted-state"` was auto-accepted from an existing approval;
/// Codex records no telemetry and no new approval for it. Callers must check this
/// before [`approval_result`] / [`AppApprovalStore::approve`] (our layer also
/// short-circuits with [`AppApprovalStore::is_allowed`] before eliciting).
pub fn is_persisted_state(result: &Value) -> bool {
    result.get("content").and_then(|c| c.get("source")).and_then(Value::as_str) == Some("computer-use-persisted-state")
}

/// Resolve the elicitation result: only `accept` approves; the persistence is
/// `always` iff `_meta.persist == "always"`, else `session`. Any other action is
/// the verbatim "not approved" error.
pub fn resolve_elicitation(result: &Value, display_name: &str) -> Result<Persist, String> {
    let action = result.get("action").and_then(Value::as_str).unwrap_or("");
    if action != "accept" {
        return Err(format!("Computer Use was not approved to use {display_name}"));
    }
    let persist = result.get("_meta").and_then(|m| m.get("persist")).and_then(Value::as_str);
    Ok(if persist == Some("always") { Persist::Always } else { Persist::Session })
}

// ---- on-disk allowlists ----------------------------------------------------

/// The two stores: per-session TOML + global JSON.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AppApprovalStore {
    pub session_path: PathBuf,
    pub always_path: PathBuf,
}

fn toml_string(s: &str) -> String {
    // bundle ids are plain; escape the two characters TOML basic strings need
    format!("\"{}\"", s.replace('\\', "\\\\").replace('"', "\\\""))
}

/// Render the session file: `[apps]\nallowed = ["a", "b"]\n`.
pub fn render_session_toml(allowed: &[String]) -> String {
    let items: Vec<String> = allowed.iter().map(|s| toml_string(s)).collect();
    format!("[apps]\nallowed = [{}]\n", items.join(", "))
}

/// Parse the session file's `allowed` list. An empty/whitespace file is an empty
/// list; anything that is not the single-line shape Codex writes
/// (`[apps]\nallowed = ["a", "b"]`) is an error, so a store is never silently
/// treated as empty and then overwritten (review finding).
pub fn parse_session_toml(text: &str) -> Result<Vec<String>, String> {
    if text.trim().is_empty() { return Ok(Vec::new()); }
    let mut out = Vec::new();
    let mut lines = text.lines().map(str::trim).filter(|l| !l.is_empty() && !l.starts_with('#'));
    if lines.next() != Some("[apps]") { return Err("session store: expected `[apps]` header".into()); }
    let Some(line) = lines.next() else { return Err("session store: missing `allowed = [...]`".into()) };
    if lines.next().is_some() { return Err("session store: unexpected extra lines".into()); }
    let Some(rest) = line.strip_prefix("allowed") else { return Err("session store: expected `allowed = [...]`".into()) };
    let rest = rest.trim_start();
    let Some(rest) = rest.strip_prefix('=') else { return Err("session store: expected `=`".into()) };
    let rest = rest.trim();
    let Some(inner) = rest.strip_prefix('[').and_then(|r| r.strip_suffix(']')) else {
        return Err("session store: `allowed` is not a single-line array".into());
    };
    // comma-separated TOML basic strings only (what Codex writes)
    let mut chars = inner.chars().peekable();
    loop {
        while matches!(chars.peek(), Some(c) if c.is_whitespace() || *c == ',') { chars.next(); }
        match chars.next() {
            None => break,
            Some('"') => {
                let mut cur = String::new();
                let mut esc = false;
                let mut closed = false;
                for c in chars.by_ref() {
                    if esc { cur.push(c); esc = false; }
                    else if c == '\\' { esc = true; }
                    else if c == '"' { closed = true; break; }
                    else { cur.push(c); }
                }
                if !closed { return Err("session store: unterminated string".into()); }
                out.push(cur);
            }
            Some(c) => return Err(format!("session store: unsupported token {c:?} (only basic strings are written)")),
        }
    }
    Ok(out)
}

/// Render the global file: `{"approvedBundleIdentifiers":[...]}` (compact, no newline).
pub fn render_always_json(approved: &[String]) -> String {
    serde_json::to_string(&json!({"approvedBundleIdentifiers": approved})).expect("serializable")
}

/// Parse the global file. Empty file -> empty list; malformed -> error.
pub fn parse_always_json(text: &str) -> Result<Vec<String>, String> {
    if text.trim().is_empty() { return Ok(Vec::new()); }
    let v: Value = serde_json::from_str(text).map_err(|e| format!("global store: {e}"))?;
    let arr = v.get("approvedBundleIdentifiers").and_then(Value::as_array)
        .ok_or_else(|| "global store: missing `approvedBundleIdentifiers` array".to_string())?;
    arr.iter().map(|x| x.as_str().map(str::to_string).ok_or_else(|| "global store: non-string entry".to_string())).collect()
}

impl AppApprovalStore {
    pub fn new(session_path: impl Into<PathBuf>, always_path: impl Into<PathBuf>) -> Self {
        AppApprovalStore { session_path: session_path.into(), always_path: always_path.into() }
    }

    fn read(p: &Path) -> String { std::fs::read_to_string(p).unwrap_or_default() }

    /// The session list; a malformed file is an error (never read as empty).
    pub fn session_allowed(&self) -> Result<Vec<String>, String> { parse_session_toml(&Self::read(&self.session_path)) }
    /// The global list; a malformed file is an error (never read as empty).
    pub fn always_allowed(&self) -> Result<Vec<String>, String> { parse_always_json(&Self::read(&self.always_path)) }

    /// Allowed if the bundle id is in either store (exact match). A malformed
    /// store grants nothing (fail closed).
    pub fn is_allowed(&self, bundle_identifier: &str) -> bool {
        self.session_allowed().map(|l| l.iter().any(|b| b == bundle_identifier)).unwrap_or(false)
            || self.always_allowed().map(|l| l.iter().any(|b| b == bundle_identifier)).unwrap_or(false)
    }

    /// Record an accepted approval in the store for its scope (idempotent,
    /// preserves order of earlier entries). Refuses to write over a store it
    /// cannot parse, so existing approvals are never dropped.
    pub fn approve(&self, bundle_identifier: &str, persist: Persist) -> Result<(), String> {
        let (path, text) = match persist {
            Persist::Session => {
                let mut cur = self.session_allowed()?;
                if !cur.iter().any(|b| b == bundle_identifier) { cur.push(bundle_identifier.to_string()); }
                (&self.session_path, render_session_toml(&cur))
            }
            Persist::Always => {
                let mut cur = self.always_allowed()?;
                if !cur.iter().any(|b| b == bundle_identifier) { cur.push(bundle_identifier.to_string()); }
                (&self.always_path, render_always_json(&cur))
            }
        };
        if let Some(dir) = path.parent() { std::fs::create_dir_all(dir).map_err(|e| e.to_string())?; }
        std::fs::write(path, text).map_err(|e| e.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn taxonomy_covers_every_tag_once_and_unknown_is_outside_taxonomy() {
        let mut tags: Vec<&str> = TAXONOMY.iter().map(|(t, _)| *t).collect();
        let n = tags.len();
        tags.sort(); tags.dedup();
        assert_eq!(tags.len(), n, "duplicate tag");
        assert_eq!(n, 26);
        for m in Mode::ALL { assert!(TAXONOMY.iter().any(|(_, mm)| *mm == m), "{m:?} has no categories"); }
        assert_eq!(mode_for("99"), Mode::NoConfirmationNeeded);
        assert_eq!(mode_for(""), Mode::NoConfirmationNeeded);
        assert_eq!(mode_for("2"), Mode::NoConfirmationNeeded, "no prefix matching of tags");
    }

    #[test]
    fn mode_transitions_for_every_tag_and_every_pre_approval_source() {
        let sources = [None, Some(InstructionSource::UserAuthored), Some(InstructionSource::ThirdPartyContent)];
        for (tag, mode) in TAXONOMY {
            for pa in sources {
                let d = decide(tag, pa);
                let expected = match (mode, pa) {
                    (Mode::HandOffRequired, _) => Decision::HandOff,
                    (Mode::AlwaysConfirm, _) => Decision::Confirm,
                    (Mode::PreApprovalWorks, Some(InstructionSource::UserAuthored)) => Decision::Proceed,
                    (Mode::PreApprovalWorks, _) => Decision::Confirm,
                    (Mode::NoConfirmationNeeded, _) => Decision::Proceed,
                };
                assert_eq!(d, expected, "tag {tag} mode {mode:?} pre_approval {pa:?}");
            }
        }
        // the two rules the policy states explicitly
        assert_eq!(decide("11", Some(InstructionSource::UserAuthored)), Decision::Confirm, "Always Confirm even if pre-approved");
        assert_eq!(decide("14", Some(InstructionSource::ThirdPartyContent)), Decision::Confirm, "third-party content is never permission");
        assert_eq!(decide("2.4", Some(InstructionSource::UserAuthored)), Decision::HandOff);
        assert!(!InstructionSource::ThirdPartyContent.grants_permission() && InstructionSource::UserAuthored.grants_permission());
    }

    #[test]
    fn mode_titles_are_the_policy_headings_in_both_texts() {
        for m in Mode::ALL {
            let heading = format!("{}) {}", m.number(), m.title());
            // line-exact: a substring match would also accept a deeper heading level
            assert!(POLICY_WINDOWS.lines().any(|l| l == format!("#### {heading}")), "windows text lacks {heading}");
            assert!(POLICY_MACOS.lines().any(|l| l == format!("### {heading}")), "macos text lacks {heading}");
        }
    }

    #[test]
    fn elicitation_shape_matches_policy_js() {
        let t = AppTarget { bundle_identifier: "com.apple.Safari".into(), display_name: "Safari".into(), risk: "medium".into(), warning_subtitle: None };
        let e = elicitation(&t, true, "click", Some("call_1"));
        assert_eq!(e["message"], "Allow Computer Use to use \"Safari\"?");
        let m = e["meta"].as_object().unwrap();
        assert_eq!(m["codex_approval_kind"], "mcp_tool_call");
        assert_eq!(m["connector_id"], "computer-use");
        assert_eq!(m["connector_name"], "Computer Use");
        assert_eq!(m["persist"], json!(["session", "always"]));
        assert_eq!(m["riskLevel"], "medium");
        assert_eq!(m["tool_call_id"], "call_1");
        assert_eq!(m["tool_name"], "click");
        assert_eq!(m["tool_params"], json!({"app": "com.apple.Safari"}));
        assert_eq!(m["tool_params_display"], json!([{"name": "app", "display_name": "App", "value": "Safari"}]));
        assert!(!m.contains_key("subtitle"));
        let mut keys: Vec<&String> = m.keys().collect(); keys.sort();
        assert_eq!(keys, ["codex_approval_kind", "connector_id", "connector_name", "persist", "riskLevel", "tool_call_id", "tool_name", "tool_params", "tool_params_display"]);
        // non-persistent + subtitle + no call id
        let t2 = AppTarget { warning_subtitle: Some("Careful".into()), ..t.clone() };
        let e2 = elicitation(&t2, false, "type_text", None);
        assert_eq!(e2["meta"]["persist"], json!(["session"]));
        assert_eq!(e2["meta"]["subtitle"], "Careful");
        assert!(e2["meta"].get("tool_call_id").is_none());
    }

    #[test]
    fn elicitation_resolution_and_result_mapping() {
        assert_eq!(resolve_elicitation(&json!({"action": "accept", "_meta": {"persist": "always"}}), "Safari"), Ok(Persist::Always));
        assert_eq!(resolve_elicitation(&json!({"action": "accept", "_meta": {"persist": "session"}}), "Safari"), Ok(Persist::Session));
        assert_eq!(resolve_elicitation(&json!({"action": "accept"}), "Safari"), Ok(Persist::Session), "no persist meta -> session");
        for a in ["decline", "cancel", "", "ACCEPT"] {
            assert_eq!(resolve_elicitation(&json!({"action": a}), "Safari"), Err("Computer Use was not approved to use Safari".to_string()), "{a}");
        }
        assert_eq!(approval_result("accept"), Some("accepted"));
        assert_eq!(approval_result("cancel"), Some("canceled"));
        assert_eq!(approval_result("decline"), Some("declined"));
        assert_eq!(approval_result("other"), None);
        assert_eq!(app_policy_refusal(AppPolicyDecision::Allowed, "x"), None);
        assert_eq!(app_policy_refusal(AppPolicyDecision::Denied, "x").unwrap(), "Computer Use is blocked from using the app 'x' by your organization's policy.");
        assert_eq!(app_policy_refusal(AppPolicyDecision::Forbidden, "x").unwrap(), "Computer Use is not allowed to use the app 'x' for safety reasons.");
    }

    #[test]
    fn on_disk_formats_are_the_observed_bytes() {
        assert_eq!(render_session_toml(&["live.taiku.app".into()]), "[apps]\nallowed = [\"live.taiku.app\"]\n");
        assert_eq!(render_always_json(&["org.mozilla.firefox".into(), "com.apple.mail".into(), "com.google.Chrome".into()]),
                   "{\"approvedBundleIdentifiers\":[\"org.mozilla.firefox\",\"com.apple.mail\",\"com.google.Chrome\"]}");
        assert_eq!(parse_session_toml("[apps]\nallowed = [\"com.apple.Safari\"]\n").unwrap(), vec!["com.apple.Safari"]);
        assert_eq!(parse_session_toml("[apps]\nallowed = [\"a\", \"b\"]\n").unwrap(), vec!["a", "b"]);
        assert_eq!(parse_session_toml("[apps]\nallowed = []\n").unwrap(), Vec::<String>::new());
        assert_eq!(parse_session_toml("").unwrap(), Vec::<String>::new());
        assert_eq!(parse_always_json("{\"approvedBundleIdentifiers\":[\"a\"]}").unwrap(), vec!["a"]);
        assert_eq!(parse_always_json("").unwrap(), Vec::<String>::new());
        // anything we do not write is an error, never an empty list
        for bad in ["garbage", "[apps]\nallowed = [\n  \"a\",\n]\n", "[apps]\nallowed = ['a']\n", "[apps]\nallowed = [\"a\"\n", "[other]\nallowed = [\"a\"]\n", "[apps]\nallowed = [\"a\"]\nextra = 1\n"] {
            assert!(parse_session_toml(bad).is_err(), "{bad:?}");
        }
        for bad in ["nope", "{}", "{\"approvedBundleIdentifiers\":\"a\"}", "{\"approvedBundleIdentifiers\":[1]}", "{\"approvedBundleIdentifiers\":[\"a\"]"] {
            assert!(parse_always_json(bad).is_err(), "{bad:?}");
        }
        // round trip for arbitrary ids incl. characters that need escaping
        for ids in [vec![], vec!["a".to_string()], vec!["x.y".to_string(), "with \"quote\"".to_string(), "back\\slash".to_string()]] {
            assert_eq!(parse_session_toml(&render_session_toml(&ids)).unwrap(), ids);
            assert_eq!(parse_always_json(&render_always_json(&ids)).unwrap(), ids);
        }
    }

    #[test]
    fn store_roundtrip_session_vs_always_scopes() {
        let dir = std::env::temp_dir().join(format!("confirmations-test-{}-{}", std::process::id(),
            std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()));
        let s = AppApprovalStore::new(dir.join("sessions/abc.toml"), dir.join("ComputerUseAppApprovals.json"));
        assert!(!s.is_allowed("com.apple.Safari"));
        s.approve("com.apple.Safari", Persist::Session).unwrap();
        assert!(s.is_allowed("com.apple.Safari"));
        assert_eq!(std::fs::read_to_string(&s.session_path).unwrap(), "[apps]\nallowed = [\"com.apple.Safari\"]\n");
        assert!(!s.always_path.exists(), "a session approval must not touch the global store");
        s.approve("org.mozilla.firefox", Persist::Always).unwrap();
        assert_eq!(std::fs::read_to_string(&s.always_path).unwrap(), "{\"approvedBundleIdentifiers\":[\"org.mozilla.firefox\"]}");
        assert_eq!(s.session_allowed().unwrap(), vec!["com.apple.Safari"]);
        assert!(s.is_allowed("org.mozilla.firefox"));
        // idempotent + order preserved + exact match only
        s.approve("com.apple.Safari", Persist::Session).unwrap();
        s.approve("com.apple.mail", Persist::Session).unwrap();
        assert_eq!(s.session_allowed().unwrap(), vec!["com.apple.Safari", "com.apple.mail"]);
        assert!(!s.is_allowed("com.apple.safari") && !s.is_allowed("com.apple"));
        // a NEW session (different session file) does not inherit session approvals but does see `always`
        let s2 = AppApprovalStore::new(dir.join("sessions/def.toml"), dir.join("ComputerUseAppApprovals.json"));
        assert!(!s2.is_allowed("com.apple.Safari"));
        assert!(s2.is_allowed("org.mozilla.firefox"));
    }

    #[test]
    fn malformed_store_is_never_overwritten_and_grants_nothing() {
        let dir = std::env::temp_dir().join(format!("confirmations-bad-{}-{}", std::process::id(),
            std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()));
        std::fs::create_dir_all(&dir).unwrap();
        let s = AppApprovalStore::new(dir.join("s.toml"), dir.join("a.json"));
        let bad_toml = "[apps]\nallowed = [\n  \"com.apple.Safari\",\n]\n";
        let bad_json = "{\"approvedBundleIdentifiers\":[\"org.mozilla.firefox\"";
        std::fs::write(&s.session_path, bad_toml).unwrap();
        std::fs::write(&s.always_path, bad_json).unwrap();
        assert!(!s.is_allowed("com.apple.Safari") && !s.is_allowed("org.mozilla.firefox"), "fail closed");
        assert!(s.approve("com.apple.mail", Persist::Session).is_err());
        assert!(s.approve("com.apple.mail", Persist::Always).is_err());
        assert_eq!(std::fs::read_to_string(&s.session_path).unwrap(), bad_toml, "must not clobber");
        assert_eq!(std::fs::read_to_string(&s.always_path).unwrap(), bad_json, "must not clobber");
    }

    #[test]
    fn persisted_state_results_are_recognised() {
        let persisted = json!({"action": "accept", "content": {"source": "computer-use-persisted-state"}});
        assert!(is_persisted_state(&persisted));
        assert!(!is_persisted_state(&json!({"action": "accept", "_meta": {"persist": "always"}})));
        assert!(!is_persisted_state(&json!({"action": "accept", "content": {"source": "other"}})));
        // it still resolves as an acceptance (the JS only skips telemetry/recording)
        assert_eq!(resolve_elicitation(&persisted, "Safari"), Ok(Persist::Session));
    }

    #[test]
    fn windows_hard_deny_list_is_verbatim_and_mandatory() {
        assert!(HARD_DENY_WINDOWS.starts_with("## Non-negotiable Windows Automation Safety"));
        assert!(HARD_DENY_WINDOWS.contains("These denies are mandatory. Confirmation policy applies only to allowed-but-confirmed actions and cannot replace these denies."));
        // the mechanically enforced rule is present verbatim
        assert!(HARD_DENY_WINDOWS.contains("Never call `press_key` with `Meta`, `Windows`, `Win`, `WIN+...`, `Windows+...`, `WINDOWS+...`, `Meta+...`, `Cmd`, `Command`, `Super`, or `OS` key names."));
        // the other non-negotiable categories are carried
        for line in ["Do not automate password manager apps", "Windows security or anti-malware apps", "Do not use the Windows Run dialog.", "Do not submit age verification."] {
            assert!(HARD_DENY_WINDOWS.contains(line), "hard-deny list is missing: {line}");
        }
    }

    #[test]
    fn press_key_hard_deny_refuses_the_windows_meta_key_only() {
        // every spelling of the Windows/Meta key is denied (chords, keysyms, Win32)
        for denied in ["Meta", "Windows", "Win", "Win+R", "Windows+D", "WINDOWS+E", "Meta+space", "Cmd", "Command", "Super", "OS", "Super_L", "Meta_R", "LWin", "RWin", "ctrl+win+left"] {
            assert!(press_key_hard_denied(denied).is_some(), "must deny the Windows/Meta chord: {denied:?}");
        }
        // ordinary chords are allowed (the deny is scoped to that one key)
        for allowed in ["a", "Return", "space", "Tab", "Control_L+a", "Control_L+Shift_L+period", "KP_0", "ctrl+c", "alt+F4", "swindler", "superb"] {
            assert!(press_key_hard_denied(allowed).is_none(), "must allow the ordinary chord: {allowed:?}");
        }
    }
}
