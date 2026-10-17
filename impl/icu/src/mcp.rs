//! `icu mcp`: a stdio MCP server (JSON-RPC 2.0, one JSON object per line) in
//! front of [`crate::Layer`]. Tool names/descriptions/schemas come from
//! [`crate::tool_defs`]. When the client advertises the `elicitation`
//! capability, an unapproved app raises Codex's `createElicitation` payload as
//! an `elicitation/create` request to the client, the answer is recorded, and
//! the input tool proceeds; otherwise the tool returns the elicitation in its
//! structured error for the caller to resolve through `approve_app`.
use crate::{Layer, ToolError, CODE_APP_APPROVAL_REQUIRED, CODE_CONFIRMATION_REQUIRED, CODE_PREPARED_APPROVAL_REQUIRED};
#[allow(unused_imports)]
use crate::CODE_INVALID_ARGUMENT;
use serde_json::{json, Value};
use std::io::{BufRead, Write};

/// Protocol versions this server speaks; the client's requested version wins when it is one of
/// them, else the newest. Elicitation is a 2025-06-18 feature and is only used when the client
/// declared the capability.
pub const PROTOCOL_VERSIONS: [&str; 3] = ["2025-06-18", "2025-03-26", "2024-11-05"];
pub const SERVER_NAME: &str = "invisible-computer-use";
/// Tools reachable through `dispatch` but deliberately not callable over MCP.
pub const NOT_OVER_MCP: [&str; 6] = ["approve_app", "approve_prepared", "confirm_secure_input", "placement", "stop_session", "resume_session"];

/// The transport the server talks to; stdin/stdout in production, buffers in tests.
pub struct Io<'a> { pub reader: &'a mut dyn BufRead, pub writer: &'a mut dyn Write }

pub struct Server<'a> {
    pub layer: &'a Layer,
    client_elicitation: bool,
    next_id: u64,
    /// Responses to server-initiated requests that arrived while a different one was awaited.
    pending: std::collections::HashMap<u64, Value>,
    hints: crate::memory_hint::HintState,
}

pub(crate) fn tool_result(v: &Value) -> Value {
    // MCP wants `structuredContent` to be an object; a bare list there is rejected by Claude Code, so a list
    // goes under `result` while the text content stays the plain JSON.
    let structured = if v.is_object() { v.clone() } else { json!({"result": v}) };
    json!({"content": [{"type": "text", "text": serde_json::to_string(v).unwrap()}], "structuredContent": structured})
}

pub fn tool_error(e: &ToolError) -> Value {
    json!({"content": [{"type": "text", "text": e.message}], "isError": true, "structuredContent": e.structured})
}

impl<'a> Server<'a> {
    pub fn new(layer: &'a Layer) -> Server<'a> { Server { layer, client_elicitation: false, next_id: 1, pending: Default::default(), hints: Default::default() } }

    fn send(io: &mut Io, msg: &Value) -> std::io::Result<()> {
        writeln!(io.writer, "{}", serde_json::to_string(msg).unwrap())?;
        io.writer.flush()
    }

    /// Ask the client to resolve an approval elicitation; returns the client's result.
    /// Requests that arrive meanwhile are served (they may nest another elicitation);
    /// answers to OTHER pending elicitations are kept for their waiters.
    fn elicit(&mut self, io: &mut Io, elicitation: &Value) -> Option<Value> {
        let id = self.next_id; self.next_id += 1;
        let req = json!({"jsonrpc": "2.0", "id": id, "method": "elicitation/create", "params": {
            "message": elicitation.get("message"),
            "requestedSchema": elicitation.get("requestedSchema").cloned().unwrap_or_else(|| json!({"type": "object", "properties": {"persist": {"type": "string", "enum": elicitation.pointer("/meta/persist").cloned().unwrap_or(json!(["session"]))}}})),
            "_meta": elicitation.get("meta"),
        }});
        Self::send(io, &req).ok()?;
        loop {
            if let Some(answer) = self.pending.remove(&id) { return Some(answer); }
            let mut line = String::new();
            if io.reader.read_line(&mut line).ok()? == 0 { return None; }
            let line = line.trim();
            if line.is_empty() { continue; }
            let msg: Value = match serde_json::from_str(line) {
                Ok(m) => m,
                Err(e) => { Self::send(io, &json!({"jsonrpc": "2.0", "id": null, "error": {"code": -32700, "message": format!("parse error: {e}")}})).ok()?; continue; }
            };
            if msg.get("method").is_some() {
                if let Some(reply) = self.handle(io, &msg) { Self::send(io, &reply).ok()?; }
                continue;
            }
            match msg.get("id").and_then(Value::as_u64) {
                Some(rid) if rid == id => return Some(msg.get("result").cloned().unwrap_or(json!({"action": "cancel"}))),
                Some(rid) => { self.pending.insert(rid, msg.get("result").cloned().unwrap_or(json!({"action": "cancel"}))); }
                None => {}
            }
        }
    }

    /// The tool result, plus the one-line memory prior when this is the first result for an app that memory knows.
    fn with_memory_hint(&mut self, name: &str, args: &Value, v: &Value) -> Value {
        let mut r = tool_result(v);
        if let Some(h) = crate::memory_hint::hint(self.layer, &mut self.hints, name, args, v) {
            // Claude Code hands the model `structuredContent` and drops the text blocks when both are present (measured:
            // study/audit/gaps-memory-4-2026-10-05, round 1, where a hint in a text block was never seen), so it goes in both
            if let Some(c) = r.get_mut("content").and_then(Value::as_array_mut) { c.push(json!({"type": "text", "text": h})); }
            if let Some(s) = r.get_mut("structuredContent").and_then(Value::as_object_mut) { s.insert("memory_prior".into(), json!(h)); }
        }
        r
    }

    /// One tool call, with the elicitation round trip when the client supports it.
    fn call_tool(&mut self, io: &mut Io, name: &str, args: &Value) -> Value {
        if NOT_OVER_MCP.contains(&name) {
            return tool_error(&ToolError::new(crate::CODE_INVALID_ARGUMENT, format!("{name} is not callable over MCP: it is the operator's (`icu call {name}`), not the model's")));
        }
        match self.layer.dispatch(name, args) {
            Ok(v) => self.with_memory_hint(name, args, &v),
            Err(e) if e.code() == Some(CODE_APP_APPROVAL_REQUIRED) && self.client_elicitation => {
                let elicitation = e.structured.get("elicitation").cloned().unwrap_or(Value::Null);
                let app = e.structured.get("app").cloned().unwrap_or(Value::Null);
                let Some(result) = self.elicit(io, &elicitation) else { return tool_error(&e) };
                let answer = json!({
                    "app": app, "display_name": elicitation.pointer("/meta/tool_params_display/0/value"),
                    "action": result.get("action").cloned().unwrap_or(json!("cancel")),
                    "persist": result.pointer("/content/persist").cloned().unwrap_or(json!("session")),
                });
                match self.layer.dispatch("approve_app", &answer) {
                    Ok(_) => match self.layer.dispatch(name, args) { Ok(v) => tool_result(&v), Err(e) => tool_error(&e) },
                    Err(refused) => tool_error(&refused),
                }
            }
            // a password field: the human confirms this one input, then it proceeds
            Err(e) if e.code() == Some(CODE_CONFIRMATION_REQUIRED) && self.client_elicitation => {
                let elicitation = e.structured.get("elicitation").cloned().unwrap_or(Value::Null);
                let Some(result) = self.elicit(io, &elicitation) else { return tool_error(&e) };
                let mut answer = e.structured.get("confirm").cloned().unwrap_or(json!({}));
                answer["action"] = result.get("action").cloned().unwrap_or(json!("cancel"));
                match self.layer.dispatch("confirm_secure_input", &answer) {
                    Ok(_) => match self.layer.dispatch(name, args) { Ok(v) => tool_result(&v), Err(e) => tool_error(&e) },
                    Err(refused) => tool_error(&refused),
                }
            }
            // a prepared action: the human approves this plan (and may edit its text), then it commits
            Err(e) if e.code() == Some(CODE_PREPARED_APPROVAL_REQUIRED) && self.client_elicitation => {
                let elicitation = e.structured.get("elicitation").cloned().unwrap_or(Value::Null);
                let Some(result) = self.elicit(io, &elicitation) else { return tool_error(&e) };
                let mut answer = e.structured.get("confirm").cloned().unwrap_or(json!({}));
                answer["action"] = result.get("action").cloned().unwrap_or(json!("cancel"));
                if let Some(p) = result.pointer("/content/persist") { answer["persist"] = p.clone(); }
                if let Some(t) = result.pointer("/content/text").or_else(|| result.pointer("/content/value")) { answer["text"] = t.clone(); }
                match self.layer.dispatch("approve_prepared", &answer) {
                    Ok(_) => match self.layer.dispatch(name, args) { Ok(v) => tool_result(&v), Err(e) => tool_error(&e) },
                    Err(refused) => tool_error(&refused),
                }
            }
            Err(e) => tool_error(&e),
        }
    }

    /// Handle one request; `None` for notifications.
    pub fn handle(&mut self, io: &mut Io, msg: &Value) -> Option<Value> {
        let id = msg.get("id").cloned();
        let method = msg.get("method").and_then(Value::as_str).unwrap_or("");
        let params = msg.get("params").cloned().unwrap_or(json!({}));
        let result = match method {
            "initialize" => {
                self.client_elicitation = params.pointer("/capabilities/elicitation").is_some();
                let requested = params.get("protocolVersion").and_then(Value::as_str).unwrap_or("");
                let version = PROTOCOL_VERSIONS.iter().find(|v| **v == requested).copied().unwrap_or(PROTOCOL_VERSIONS[0]);
                json!({"protocolVersion": version, "capabilities": {"tools": {}, "resources": {}}, "serverInfo": {"name": SERVER_NAME, "version": env!("CARGO_PKG_VERSION")}, "instructions": crate::mcp_instructions()})
            }
            "ping" => json!({}),
            "tools/list" => json!({"tools": crate::tool_defs()}),
            "resources/list" => json!({"resources": [{"uri": crate::desktop::LINUX_SKILL_URI, "name": "linux-desktop", "title": "Full-desktop verbs", "description": "How to use desktop_screenshot, desktop_click, desktop_drag, desktop_drag_handle, desktop_move, desktop_scroll, desktop_press_key and desktop_type_text.", "mimeType": "text/markdown"}]}),
            "resources/read" => {
                let uri = params.get("uri").and_then(Value::as_str).unwrap_or("");
                if uri != crate::desktop::LINUX_SKILL_URI {
                    return id.map(|id| json!({"jsonrpc": "2.0", "id": id, "error": {"code": -32002, "message": format!("resource not found: {uri}")}}));
                }
                json!({"contents": [{"uri": uri, "mimeType": "text/markdown", "text": crate::desktop::LINUX_SKILL}]})
            }
            "tools/call" => {
                let name = params.get("name").and_then(Value::as_str).unwrap_or("").to_string();
                let args = params.get("arguments").cloned().unwrap_or(json!({}));
                self.call_tool(io, &name, &args)
            }
            m if m.starts_with("notifications/") => return None,
            other => {
                return id.map(|id| json!({"jsonrpc": "2.0", "id": id, "error": {"code": -32601, "message": format!("method not found: {other}")}}));
            }
        };
        id.map(|id| json!({"jsonrpc": "2.0", "id": id, "result": result}))
    }

    /// Serve until the reader closes.
    pub fn serve(&mut self, io: &mut Io) -> std::io::Result<()> {
        loop {
            let mut line = String::new();
            if io.reader.read_line(&mut line)? == 0 { return Ok(()); }
            let line = line.trim();
            if line.is_empty() { continue; }
            let msg: Value = match serde_json::from_str(line) {
                Ok(m) => m,
                Err(e) => { Self::send(io, &json!({"jsonrpc": "2.0", "id": null, "error": {"code": -32700, "message": format!("parse error: {e}")}}))?; continue; }
            };
            if let Some(reply) = self.handle(io, &msg) { Self::send(io, &reply)?; }
        }
    }
}
