//! Two-phase prepare / approve / commit for irreversible actions (specs/15-native-app-tools.md:36).
//!
//! Codex's Messages tool never sends in one call: `PrepareSendRequest` resolves the recipients and
//! the final body and returns a `PreparedSend` without side effects; the user sees that plan, may
//! edit it, and approves it with a persistence scope; `CommitSendRequest` then delivers and
//! answers with an outcome enum, not a boolean (sent, outcome uncertain, rate limited, plan
//! unavailable). A cancelled approval is final and said so in-band ("Do not ask the user to
//! approve access or retry"). The Messages tools are out of scope for a generic driver; the pattern
//! is not, and is what this module applies to any input tool a model wants to run with a human in
//! the loop (a paste-and-submit, a form submit, a destructive click):
//!
//! 1. `prepare_action {tool, arguments}` validates the call the way the real one would and returns
//!    the plan: the app, the window, the resolved target (the element's label, not just its id) and
//!    the exact text or key. It performs nothing: no input tool reaches the driver.
//! 2. The human answers. Over MCP the server raises an elicitation when the model commits; from a
//!    shell the operator runs `icu call approve_prepared`. The answer carries accept / decline /
//!    cancel, a persistence scope (once, session, always) and, for text actions, an optional edited
//!    text. `approve_prepared` is not an MCP tool: a model that could call it would approve itself.
//! 3. `commit_action {prepared_id}` runs the prepared call through the ordinary input path (app
//!    approval, hard deny, secure-field confirmation all still apply), at most once, and returns
//!    `{outcome}`: `committed`, `outcome_uncertain` (the driver could not confirm delivery, or the
//!    deadline passed), or a refusal: `planUnavailable` (unknown, expired, already committed, the
//!    target no longer exists) or `approvalCanceled`.
//!
//! A plan is bound to what the model saw: if its element is gone or relabelled by commit time the
//! commit is `planUnavailable` (`target_changed`), never a click on whatever took its place.
//! The scopes are INFERRED from Codex's `PERSISTENCE_{ONCE,SESSION,ALWAYS}` (TURN has no analogue
//! here); the store is keyed by tool and app the way `MessagesSendApprovalStore` keys by operation
//! and destination.
use super::*;

pub const PREPARED_TTL_SECS: u64 = 600;
pub const CANCELED_MESSAGE: &str = "The action was not performed because its approval was canceled. Do not ask the user to approve it again or retry.";

fn now_secs() -> u64 { std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0) }

/// Tools whose plan carries text the human may edit, and the argument that holds it.
fn editable_field(tool: &str) -> Option<&'static str> {
    match tool { "type_text" | "paste" => Some("text"), "set_value" => Some("value"), _ => None }
}

fn preview(text: &str) -> String {
    let t: String = text.chars().take(200).collect();
    if text.chars().count() > 200 { format!("{t}\u{2026}") } else { t }
}

fn plan_unavailable(reason: &str, message: impl Into<String>) -> ToolError {
    let mut e = ToolError::new(CODE_PLAN_UNAVAILABLE, message);
    e.structured["reason"] = json!(reason);
    e
}

impl Layer {
    pub(crate) fn prepared_path(&self, id: &str) -> PathBuf { self.cfg.state_dir.join("prepared").join(format!("{id}.json")) }

    fn load_prepared(&self, id: &str) -> Result<Value, ToolError> {
        // ids are our own hex: refuse anything else so the argument can never walk the filesystem
        if id.is_empty() || !id.bytes().all(|b| b.is_ascii_hexdigit()) { return Err(plan_unavailable("unknown", format!("{id:?} is not a prepared action"))); }
        let p: Value = std::fs::read(self.prepared_path(id)).ok().and_then(|b| serde_json::from_slice(&b).ok())
            .ok_or_else(|| plan_unavailable("unknown", "no such prepared action (it was never prepared, or is gone)"))?;
        Ok(p)
    }

    fn save_prepared(&self, id: &str, p: &Value) -> Result<(), ToolError> {
        let path = self.prepared_path(id);
        std::fs::create_dir_all(path.parent().unwrap()).map_err(|e| ToolError::new("storeError", e.to_string()))?;
        std::fs::write(&path, serde_json::to_vec(p).unwrap()).map_err(|e| ToolError::new("storeError", e.to_string()))
    }

    /// Where the "approve all of this kind" decisions are kept: per session, and (when offered) always.
    fn prepared_scope_path(&self, always: bool) -> PathBuf {
        if always { self.cfg.approvals_always.with_file_name("PreparedActionApprovals.json") }
        else { self.cfg.approvals_session.with_extension("prepared.json") }
    }

    fn prepared_scope_allows(&self, key: &str) -> bool {
        [false, true].iter().any(|always| {
            std::fs::read(self.prepared_scope_path(*always)).ok().and_then(|b| serde_json::from_slice::<Value>(&b).ok())
                .and_then(|v| v.get("keys").and_then(Value::as_array).map(|k| k.iter().any(|x| x.as_str() == Some(key)))).unwrap_or(false)
        })
    }

    fn prepared_scope_add(&self, key: &str, always: bool) -> Result<(), ToolError> {
        let path = self.prepared_scope_path(always);
        let mut keys: Vec<String> = std::fs::read(&path).ok().and_then(|b| serde_json::from_slice::<Value>(&b).ok())
            .and_then(|v| v.get("keys").and_then(Value::as_array).map(|k| k.iter().filter_map(|x| x.as_str().map(str::to_string)).collect())).unwrap_or_default();
        if !keys.iter().any(|k| k == key) { keys.push(key.to_string()); }
        std::fs::create_dir_all(path.parent().unwrap()).map_err(|e| ToolError::new("storeError", e.to_string()))?;
        std::fs::write(&path, serde_json::to_vec(&json!({"keys": keys})).unwrap()).map_err(|e| ToolError::new("storeError", e.to_string()))
    }

    /// Phase 1: resolve and plan, no side effects.
    pub fn prepare_action(&self, args: &Value) -> Result<Value, ToolError> {
        let tool = args.get("tool").and_then(Value::as_str).filter(|t| INPUT_TOOLS.contains(t))
            .ok_or_else(|| ToolError::new(CODE_INVALID_ARGUMENT, format!("`tool` must be one of: {}", INPUT_TOOLS.join(", "))))?;
        let a = args.get("arguments").filter(|a| a.is_object()).ok_or_else(|| ToolError::new(CODE_INVALID_ARGUMENT, "`arguments` (the tool's own arguments, with its `window`) is required"))?;
        // the refusals that would stop the real call stop the plan too
        self.hard_deny(tool, a)?;
        self.pre_dispatch(tool)?;
        let (pid, id) = self.resolve_window(a)?;
        let (bundle, name) = self.app_facts(pid)?;
        let bundle = bundle.unwrap_or_else(|| format!("pid:{pid}"));
        self.check_app_policy(&bundle)?;
        let num = |k: &str| a.get(k).and_then(Value::as_f64);
        let element = a.get("element_index").and_then(Value::as_u64);
        let missing = |what: &str| ToolError::new(CODE_INVALID_ARGUMENT, format!("{tool} needs {what}"));
        match tool {
            "click" if element.is_none() && (num("x").is_none() || num("y").is_none()) => return Err(missing("element_index or x and y")),
            "press_key" if a.get("key").and_then(Value::as_str).is_none() => return Err(missing("`key`")),
            "type_text" | "paste" if a.get("text").and_then(Value::as_str).is_none() => return Err(missing("`text`")),
            "select_text" if a.get("text").and_then(Value::as_str).is_none() => return Err(missing("`text`")),
            "set_value" if element.is_none() || a.get("value").and_then(Value::as_str).is_none() => return Err(missing("`element_index` and `value`")),
            "perform_secondary_action" if element.is_none() || a.get("action").and_then(Value::as_str).is_none() => return Err(missing("`element_index` and `action`")),
            "scroll" if num("x").is_none() || num("y").is_none() => return Err(missing("`x` and `y`")),
            "drag" if ["from_x", "from_y", "to_x", "to_y"].iter().any(|k| num(k).is_none()) => return Err(missing("from_x, from_y, to_x, to_y")),
            _ => {}
        }
        // the element must be one the latest state shows (a stale id is a plan that cannot be made)
        let (label, role, secure) = match element {
            Some(stable) => {
                self.element_target(id, a).map_err(|e| plan_unavailable("target_missing", e.message))?;
                let t = self.targets_for(id);
                let (role, label) = t.as_ref().and_then(|t| t.labels.get(&stable).cloned()).unwrap_or_default();
                (Some(label).filter(|l| !l.is_empty()), Some(role).filter(|r| !r.is_empty()), t.as_ref().is_some_and(|t| t.secure.contains_key(&stable)))
            }
            None => (None, None, matches!(tool, "type_text" | "paste") && self.targets_for(id).is_some_and(|t| t.focused_secure.is_some())),
        };
        let field = editable_field(tool);
        let shown = field.and_then(|f| a.get(f)).and_then(Value::as_str).map(|t| if secure { "\u{2022}\u{2022}\u{2022}\u{2022}".to_string() } else { preview(t) });
        let prepared_id = crate::predicates::sha256_hex_of(format!("{}:{}:{}:{}", std::process::id(), now_secs(), id, serde_json::to_string(a).unwrap_or_default()).as_bytes())[..24].to_string()
            + &format!("{:x}", std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.subsec_nanos()).unwrap_or(0));
        let app = name.clone().unwrap_or_else(|| bundle.clone());
        let mut plan = json!({
            "prepared_id": prepared_id, "tool": tool, "app": {"id": bundle, "display_name": app},
            "window": {"id": id}, "target": {"element_index": element, "label": label, "role": role,
                "x": num("x").or(num("from_x")), "y": num("y").or(num("from_y"))},
            "editable": field, "text": shown, "sensitive": secure,
            "expires_unix": now_secs() + PREPARED_TTL_SECS,
            "approval": {"required": !self.prepared_scope_allows(&format!("{tool}|{bundle}")), "scopes": if self.cfg.allow_persistent_approval { json!(["once", "session", "always"]) } else { json!(["once", "session"]) }},
        });
        let stored = json!({"plan": plan, "arguments": a, "status": "pending", "label": label, "edited": null});
        self.save_prepared(&prepared_id, &stored)?;
        plan["next"] = json!("the user approves it, then commit_action {prepared_id}; nothing has been performed yet");
        Ok(plan)
    }

    pub(crate) fn targets_for(&self, id: u64) -> Option<Targets> {
        let mut t = self.targets.lock().unwrap();
        if !t.contains_key(&id) {
            if let Some(saved) = std::fs::read(self.targets_path(id)).ok().and_then(|b| serde_json::from_slice::<Targets>(&b).ok()) { t.insert(id, saved); }
        }
        t.get(&id).cloned()
    }

    /// The human's answer to a prepared action (the MCP client's elicitation, or the operator's
    /// `icu call approve_prepared`). Not an MCP tool. `accept` approves this plan (optionally with
    /// edited text) under a scope; anything else cancels it for good.
    pub fn approve_prepared(&self, args: &Value) -> Result<Value, ToolError> {
        let id = args.get("prepared_id").and_then(Value::as_str).ok_or_else(|| ToolError::new(CODE_INVALID_ARGUMENT, "`prepared_id` is required"))?;
        let mut p = self.load_prepared(id)?;
        if p["status"] != "pending" { return Err(plan_unavailable("not_pending", format!("the prepared action is already {}", p["status"].as_str().unwrap_or("settled")))); }
        let action = args.get("action").and_then(Value::as_str).unwrap_or("accept");
        if action != "accept" {
            p["status"] = json!("canceled");
            self.save_prepared(id, &p)?;
            return Err(ToolError::new(CODE_APPROVAL_CANCELED, CANCELED_MESSAGE));
        }
        let persist = args.get("persist").and_then(Value::as_str).unwrap_or("once");
        let key = format!("{}|{}", p["plan"]["tool"].as_str().unwrap_or(""), p["plan"]["app"]["id"].as_str().unwrap_or(""));
        match persist {
            "once" => {}
            "session" => self.prepared_scope_add(&key, false)?,
            "always" if self.cfg.allow_persistent_approval => self.prepared_scope_add(&key, true)?,
            "always" => return Err(ToolError::new(CODE_INVALID_ARGUMENT, "persistent approval is not offered by this configuration")),
            other => return Err(ToolError::new(CODE_INVALID_ARGUMENT, format!("persist must be once, session or always, got {other:?}"))),
        }
        if let Some(text) = args.get("text").and_then(Value::as_str) {
            if p["plan"]["editable"].is_null() { return Err(ToolError::new(CODE_INVALID_ARGUMENT, "this action has no editable text")); }
            p["edited"] = json!(text);
        }
        p["status"] = json!("approved");
        self.save_prepared(id, &p)?;
        Ok(json!({"prepared_id": id, "approved": true, "persist": persist, "edited": !p["edited"].is_null()}))
    }

    /// Phase 2: commit a prepared action, at most once.
    pub fn commit_action(&self, args: &Value) -> Result<Value, ToolError> {
        let id = args.get("prepared_id").and_then(Value::as_str).ok_or_else(|| ToolError::new(CODE_INVALID_ARGUMENT, "`prepared_id` is required"))?;
        let mut p = self.load_prepared(id)?;
        match p["status"].as_str() {
            Some("consumed") => return Err(plan_unavailable("already_committed", "this prepared action was already committed")),
            Some("canceled") => return Err(ToolError::new(CODE_APPROVAL_CANCELED, CANCELED_MESSAGE)),
            _ => {}
        }
        if p["plan"]["expires_unix"].as_u64().unwrap_or(0) < now_secs() {
            p["status"] = json!("consumed");
            self.save_prepared(id, &p)?;
            return Err(plan_unavailable("expired", "the prepared action expired; prepare it again"));
        }
        let tool = p["plan"]["tool"].as_str().unwrap_or("").to_string();
        let bundle = p["plan"]["app"]["id"].as_str().unwrap_or("").to_string();
        let key = format!("{tool}|{bundle}");
        if p["status"] != "approved" && !self.prepared_scope_allows(&key) {
            let app = p["plan"]["app"]["display_name"].as_str().unwrap_or(&bundle).to_string();
            let what = match (p["plan"]["target"]["label"].as_str(), p["plan"]["text"].as_str()) {
                (Some(l), Some(t)) => format!("{tool} \"{t}\" into \"{l}\""),
                (Some(l), None) => format!("{tool} \"{l}\""),
                (None, Some(t)) => format!("{tool} \"{t}\""),
                (None, None) => tool.clone(),
            };
            let message = format!("Let Computer Use {what} in {app}?");
            let mut e = ToolError::new(CODE_PREPARED_APPROVAL_REQUIRED, format!("{message} This action was prepared and needs the user's approval before it is committed: raise the elicitation, or the operator runs `icu call approve_prepared`, then commit again."));
            let persist = if self.cfg.allow_persistent_approval { json!(["once", "session", "always"]) } else { json!(["once", "session"]) };
            let mut props = json!({"persist": {"type": "string", "enum": persist}});
            if let Some(field) = p["plan"]["editable"].as_str() { props[field] = json!({"type": "string", "description": "Edit the text before it is sent."}); }
            e.structured["elicitation"] = json!({
                "message": message,
                "requestedSchema": {"type": "object", "properties": props},
                "meta": {"codex_approval_kind": "mcp_tool_call", "connector_id": "computer-use", "connector_name": "Computer Use", "riskLevel": self.cfg.risk_level,
                         "tool_name": "commit_action", "tool_params": {"prepared_id": id}, "persist": persist,
                         "tool_params_display": [{"name": "app", "display_name": "App", "value": app}, {"name": "action", "display_name": "Action", "value": what}]},
            });
            e.structured["confirm"] = json!({"prepared_id": id});
            e.structured["plan"] = p["plan"].clone();
            return Err(e);
        }
        // the plan is bound to what the model saw: the element must still be there under its label
        let mut call = p["arguments"].clone();
        if let Some(stable) = call.get("element_index").and_then(Value::as_u64) {
            let (window, now) = (p["plan"]["window"]["id"].as_u64().unwrap_or(0), self.targets_for(p["plan"]["window"]["id"].as_u64().unwrap_or(0)));
            let still = now.as_ref().and_then(|t| t.labels.get(&stable)).map(|(_, l)| l.as_str());
            let planned = p["label"].as_str().unwrap_or("");
            if still.is_none() || still.unwrap_or("") != planned {
                return Err(plan_unavailable("target_changed", format!("element {stable} of window {window} is no longer the \"{planned}\" the plan was made for; prepare the action again from a fresh get_window_state")));
            }
        }
        if let (Some(field), Some(text)) = (editable_field(&tool), p["edited"].as_str()) { call[field] = json!(text); }
        // at most once: spent before the driver is asked, whatever the driver says
        p["status"] = json!("consumed");
        self.save_prepared(id, &p)?;
        let result = match self.input(&tool, &call) {
            Ok(r) => r,
            Err(e) => {
                // a refusal before the driver was asked (the app is not approved, a password field
                // wants its confirmation, the session is stopped) performed nothing, so the plan
                // stays committable once that is fixed; anything the driver saw stays spent
                let before_dispatch = matches!(e.code(), Some(CODE_APP_APPROVAL_REQUIRED | CODE_CONFIRMATION_REQUIRED | CODE_APP_NOT_ALLOWED | CODE_HAND_OFF_REQUIRED
                    | CODE_STALE_ELEMENT | CODE_INVALID_ARGUMENT | CODE_USER_STOPPED_SESSION | CODE_PERMISSIONS_NOT_GRANTED | CODE_PERMISSIONS_PENDING | CODE_INCOMPATIBLE_CLIENT_VERSION));
                if before_dispatch { p["status"] = json!("approved"); self.save_prepared(id, &p)?; }
                return Err(e);
            }
        };
        let uncertain = result.get("_deadline_exceeded").and_then(Value::as_bool) == Some(true) || result.get("effect").and_then(Value::as_str) == Some("unverifiable");
        Ok(json!({"outcome": if uncertain { "outcome_uncertain" } else { "committed" }, "prepared_id": id, "tool": tool, "edited": !p["edited"].is_null(), "result": result}))
    }
}
