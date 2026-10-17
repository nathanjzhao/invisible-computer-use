//! Stdio MCP transport. Host replies are routed separately from model tool calls.
use crate::{
    Error, Result,
    consent::Scope,
    contract,
    database::Chat,
    engine::{Engine, Host, Output, ReadDecision, SendDecision},
};
use serde_json::{Value, json};
use std::{
    collections::HashMap,
    io::{BufRead, BufReader, Read, Write},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
        mpsc::{self, Receiver, SyncSender},
    },
    thread,
    time::{Duration, Instant},
};

const MAX_LINE: usize = 1024 * 1024;
const MAX_OUTPUT: usize = 32 * 1024 * 1024;
#[derive(Clone)]
pub struct Options {
    pub approval_timeout: Duration,
}
impl Default for Options {
    fn default() -> Self {
        Self {
            approval_timeout: Duration::from_secs(300),
        }
    }
}
struct Routing {
    calls: HashMap<String, Arc<AtomicBool>>,
    replies: HashMap<String, SyncSender<Value>>,
    elicitation: bool,
    turn_lifecycle: bool,
    turn_label: Option<String>,
}
struct Shared {
    output: Mutex<Box<dyn Write + Send>>,
    routing: Mutex<Routing>,
    alive: AtomicBool,
}
impl Shared {
    fn emit(&self, message: Value) -> Result<()> {
        let bytes = serde_json::to_vec(&message).map_err(|_| transport_error())?;
        if bytes.len() > MAX_OUTPUT {
            return Err(Error::new(
                "messagesResponseTooLarge",
                "Messages response is too large; request a smaller page",
            ));
        }
        let result = (|| {
            let mut output = self
                .output
                .lock()
                .map_err(|_| std::io::Error::other("output lock"))?;
            output.write_all(&bytes)?;
            output.write_all(b"\n")?;
            output.flush()
        })();
        if result.is_err() {
            self.close();
            return Err(transport_error());
        }
        Ok(())
    }
    fn close(&self) {
        self.alive.store(false, Ordering::SeqCst);
        if let Ok(mut r) = self.routing.lock() {
            for cancel in r.calls.values() {
                cancel.store(true, Ordering::SeqCst);
            }
            r.replies.clear();
        }
    }
    fn finish(&self, id: Value, message: Value) -> Result<()> {
        self.routing
            .lock()
            .map_err(|_| transport_error())?
            .calls
            .remove(&id.to_string());
        match self.emit(message) {
            Err(error) if error.code == "messagesResponseTooLarge" => {
                self.emit(call_response(id, Err(error)))
            }
            other => other,
        }
    }
}
struct Incoming {
    message: Value,
    cancel: Arc<AtomicBool>,
}
fn id(value: &Value) -> bool {
    value.is_string() || value.is_i64() || value.is_u64()
}
fn rpc_error(id: Value, code: i64, message: &str) -> Value {
    json!({"jsonrpc":"2.0","id":id,"error":{"code":code,"message":message}})
}
fn transport_error() -> Error {
    Error::new(
        "messagesHostDisconnected",
        "The Messages host disconnected; no pending approval was accepted",
    )
}
fn call_response(id: Value, result: Result<Output>) -> Value {
    let result = match result {
        Ok(Output::Json(data)) => {
            json!({"content":[{"type":"text","text":data.to_string()}],"structuredContent":data,"isError":false})
        }
        Ok(Output::Image { mime_type, data }) => {
            json!({"content":[{"type":"image","mimeType":mime_type,"data":data}],"isError":false})
        }
        Err(error) => {
            json!({"content":[{"type":"text","text":error.message}],"structuredContent":{"message":error.message},"isError":true,"_meta":{"icu.error_code":error.code}})
        }
    };
    json!({"jsonrpc":"2.0","id":id,"result":result})
}
fn frame(reader: &mut impl BufRead) -> std::io::Result<Option<Vec<u8>>> {
    let mut line = Vec::new();
    loop {
        let bytes = reader.fill_buf()?;
        if bytes.is_empty() {
            return if line.is_empty() {
                Ok(None)
            } else {
                Err(std::io::Error::new(
                    std::io::ErrorKind::UnexpectedEof,
                    "incomplete MCP frame",
                ))
            };
        }
        let n = bytes
            .iter()
            .position(|b| *b == b'\n')
            .map(|n| n + 1)
            .unwrap_or(bytes.len());
        if line.len() + n > MAX_LINE {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "MCP frame exceeds 1 MiB",
            ));
        }
        let complete = bytes[n - 1] == b'\n';
        line.extend_from_slice(&bytes[..n]);
        reader.consume(n);
        if complete {
            return Ok(Some(line));
        }
    }
}
fn read_loop(reader: impl Read, shared: Arc<Shared>, sender: SyncSender<Incoming>) {
    let mut reader = BufReader::new(reader);
    loop {
        let line = match frame(&mut reader) {
            Ok(Some(line)) => line,
            Ok(None) => break,
            Err(_) => {
                let _ = shared.emit(rpc_error(
                    Value::Null,
                    -32700,
                    "Invalid or oversized MCP frame",
                ));
                break;
            }
        };
        let message: Value = match serde_json::from_slice(&line) {
            Ok(v) => v,
            Err(_) => {
                let _ = shared.emit(rpc_error(Value::Null, -32700, "Invalid JSON"));
                continue;
            }
        };
        if message["jsonrpc"] != "2.0" || !message.is_object() {
            let _ = shared.emit(rpc_error(Value::Null, -32600, "Invalid JSON-RPC request"));
            continue;
        }
        if message.get("method").is_none() {
            if let Some(value) = message.get("id").filter(|v| id(v)) {
                let pending = shared
                    .routing
                    .lock()
                    .ok()
                    .and_then(|mut r| r.replies.remove(&value.to_string()));
                if let Some(reply) = pending {
                    let _ = reply.try_send(message);
                }
            }
            continue;
        }
        let Some(method) = message["method"].as_str() else {
            let _ = shared.emit(rpc_error(Value::Null, -32600, "Invalid request method"));
            continue;
        };
        if method == "notifications/cancelled" && message.get("id").is_none() {
            if let Some(request) = message["params"].get("requestId").filter(|v| id(v))
                && let Ok(r) = shared.routing.lock()
                && let Some(cancel) = r.calls.get(&request.to_string())
            {
                cancel.store(true, Ordering::SeqCst);
            }
            continue;
        }
        if matches!(
            method,
            "notifications/icu/turn_started" | "notifications/icu/turn_ended"
        ) && message.get("id").is_none()
        {
            let mut r = match shared.routing.lock() {
                Ok(r) => r,
                Err(_) => break,
            };
            if !r.turn_lifecycle {
                continue;
            }
            if method.ends_with("turn_started") {
                let Some(label) = message["params"]["turnId"]
                    .as_str()
                    .filter(|s| !s.is_empty() && s.len() <= 200)
                else {
                    continue;
                };
                if r.turn_label.as_deref() == Some(label) {
                    continue;
                }
                r.turn_label = Some(label.into());
            }
            for cancel in r.calls.values() {
                cancel.store(true, Ordering::SeqCst);
            }
        }
        let cancel = Arc::new(AtomicBool::new(false));
        if let Some(request) = message.get("id") {
            if !id(request) {
                let _ = shared.emit(rpc_error(Value::Null, -32600, "Invalid request id"));
                continue;
            }
            let duplicate = {
                let mut r = match shared.routing.lock() {
                    Ok(r) => r,
                    Err(_) => break,
                };
                let key = request.to_string();
                if let std::collections::hash_map::Entry::Vacant(e) = r.calls.entry(key) {
                    e.insert(cancel.clone());
                    false
                } else {
                    true
                }
            };
            if duplicate {
                let _ = shared.emit(rpc_error(
                    Value::Null,
                    -32600,
                    "Duplicate outstanding request id",
                ));
                continue;
            }
        }
        if let Err(error) = sender.try_send(Incoming { message, cancel }) {
            let incoming = match error {
                mpsc::TrySendError::Full(incoming) => incoming,
                mpsc::TrySendError::Disconnected(_) => {
                    shared.close();
                    break;
                }
            };
            if let Some(request) = incoming.message.get("id") {
                let _ = shared.finish(
                    request.clone(),
                    rpc_error(request.clone(), -32000, "Messages request queue is full"),
                );
            } else {
                shared.close();
                break;
            }
        }
    }
    shared.close();
}

struct ElicitationHost {
    shared: Arc<Shared>,
    options: Options,
}
struct PendingReply {
    shared: Arc<Shared>,
    id: Value,
}
impl Drop for PendingReply {
    fn drop(&mut self) {
        if let Ok(mut r) = self.shared.routing.lock() {
            r.replies.remove(&self.id.to_string());
        }
    }
}
impl ElicitationHost {
    fn ask(&self, params: Value, cancel: &AtomicBool) -> Result<Value> {
        if !self.shared.alive.load(Ordering::SeqCst) {
            return Err(transport_error());
        }
        let mut bytes = [0; 24];
        getrandom::fill(&mut bytes).map_err(|_| transport_error())?;
        let id = json!(format!(
            "messages-approval-{}",
            bytes.iter().map(|b| format!("{b:02x}")).collect::<String>()
        ));
        let (sender, receiver) = mpsc::sync_channel(1);
        {
            let mut routing = self.shared.routing.lock().map_err(|_| transport_error())?;
            if !routing.elicitation {
                return Err(Error::new(
                    "messagesApprovalUnavailable",
                    "This MCP client does not support approval forms. No message contents were returned.",
                ));
            }
            routing.replies.insert(id.to_string(), sender);
        }
        let _pending = PendingReply {
            shared: self.shared.clone(),
            id: id.clone(),
        };
        self.shared
            .emit(json!({"jsonrpc":"2.0","id":id,"method":"elicitation/create","params":params}))?;
        let start = Instant::now();
        loop {
            if cancel.load(Ordering::SeqCst) || start.elapsed() >= self.options.approval_timeout {
                let _=self.shared.emit(json!({"jsonrpc":"2.0","method":"notifications/cancelled","params":{"requestId":id,"reason":"The Messages request ended before approval"}}));
                return Err(Error::new(
                    if cancel.load(Ordering::SeqCst) {
                        "messagesRequestCanceled"
                    } else {
                        "messagesApprovalCanceled"
                    },
                    "The pending Messages approval ended. Do not retry it during this turn.",
                ));
            }
            if !self.shared.alive.load(Ordering::SeqCst) {
                return Err(transport_error());
            }
            match receiver.recv_timeout(Duration::from_millis(20)) {
                Ok(reply) => {
                    if cancel.load(Ordering::SeqCst) {
                        return Err(Error::new(
                            "messagesRequestCanceled",
                            "Messages request was canceled before the host approval arrived",
                        ));
                    }
                    if reply.get("error").is_some() {
                        return Err(Error::new(
                            "messagesApprovalUnavailable",
                            "The MCP host could not show the Messages approval",
                        ));
                    }
                    return reply.get("result").cloned().ok_or_else(|| {
                        Error::new(
                            "messagesApprovalInvalid",
                            "The host returned an invalid approval response",
                        )
                    });
                }
                Err(mpsc::RecvTimeoutError::Timeout) => {}
                Err(mpsc::RecvTimeoutError::Disconnected) => return Err(transport_error()),
            }
        }
    }
}
impl Host for ElicitationHost {
    fn review_send(
        &mut self,
        preview: &crate::send::Preview,
        cancel: &AtomicBool,
    ) -> Result<SendDecision> {
        let known = preview.target.consent().is_some();
        let turns = self
            .shared
            .routing
            .lock()
            .map_err(|_| transport_error())?
            .turn_lifecycle;
        let scopes = if !known {
            vec!["once"]
        } else if turns {
            vec!["once", "turn", "session", "always"]
        } else {
            vec!["once", "session", "always"]
        };
        let files = preview
            .attachments
            .iter()
            .map(|f| {
                format!(
                    "{} [resolved: {}] ({} bytes; SHA-256 {})",
                    f.path.display(),
                    f.resolved_path.display(),
                    f.size,
                    f.sha256
                )
            })
            .collect::<Vec<_>>()
            .join("\n");
        let message = format!(
            "Review this send to {}.\n{}{}",
            preview.target.recipients().join(", "),
            if files.is_empty() {
                ""
            } else {
                "Attachments:\n"
            },
            files
        );
        let schema = json!({"type":"object","additionalProperties":false,"required":["text","scope"],"properties":{"text":{"type":"string","title":"Message","default":preview.text.clone().unwrap_or_default(),"minLength":if preview.attachments.is_empty(){1}else{0},"maxLength":10000},"scope":{"type":"string","title":"Allow sending","enum":scopes,"default":"once"}}});
        let reply = self.ask(json!({"message":message,"requestedSchema":schema}), cancel)?;
        match reply["action"].as_str() {
            Some("accept") => {
                let content = &reply["content"];
                contract::validate(content, &schema).map_err(|_| {
                    Error::new(
                        "messagesApprovalInvalid",
                        "The edited send approval did not match this request",
                    )
                })?;
                let scope = match content["scope"].as_str() {
                    Some("once") => Scope::Once,
                    Some("turn") => Scope::Turn,
                    Some("session") => Scope::Session,
                    Some("always") => Scope::Always,
                    _ => return Err(Error::new("messagesApprovalInvalid", "Unknown send scope")),
                };
                Ok(SendDecision::Accept {
                    scope,
                    text: content["text"].as_str().unwrap().into(),
                })
            }
            Some("decline") => Ok(SendDecision::Decline),
            Some("cancel") => Ok(SendDecision::Cancel),
            _ => Err(Error::new(
                "messagesApprovalInvalid",
                "The host returned no send decision",
            )),
        }
    }
    fn review_read(&mut self, chats: &[Chat], cancel: &AtomicBool) -> Result<ReadDecision> {
        let turns = self
            .shared
            .routing
            .lock()
            .map_err(|_| transport_error())?
            .turn_lifecycle;
        let scopes = if turns {
            vec!["once", "turn", "session", "always"]
        } else {
            vec!["once", "session", "always"]
        };
        let mut properties = serde_json::Map::new();
        let mut required = vec![json!("scope")];
        properties.insert("scope".into(),json!({"type":"string","title":"Allow access for","description":"Once allows this request. Session lasts until this plugin connection closes. Always remembers permission for the selected chats and their current members.","enum":scopes,"default":"once"}));
        for (i, chat) in chats.iter().enumerate() {
            let key = format!("chat_{}", i + 1);
            required.push(json!(key));
            properties.insert(key,json!({"type":"boolean","title":chat.name.as_deref().filter(|s|!s.is_empty()).unwrap_or(&chat.chat_guid),"description":format!("Participants: {}. Service: {}.",if chat.participants.is_empty(){"not available".into()}else{chat.participants.join(", ")},chat.service.as_deref().unwrap_or("unknown")),"default":true}));
        }
        let schema = json!({"type":"object","properties":properties,"required":required,"additionalProperties":false});
        let result=self.ask(json!({"message":"Allow this request to read message contents from the selected chats? Unselected chats will be omitted.","requestedSchema":schema}),cancel)?;
        match result["action"].as_str() {
            Some("cancel") => Ok(ReadDecision::Cancel),
            Some("decline") => Ok(ReadDecision::Decline),
            Some("accept") => {
                let content = &result["content"];
                contract::validate(content, &schema).map_err(|_| {
                    Error::new(
                        "messagesApprovalInvalid",
                        "The host's approval fields did not match this request",
                    )
                })?;
                let scope = match content["scope"].as_str() {
                    Some("once") => Scope::Once,
                    Some("turn") if turns => Scope::Turn,
                    Some("session") => Scope::Session,
                    Some("always") => Scope::Always,
                    _ => {
                        return Err(Error::new(
                            "messagesApprovalInvalid",
                            "Unsupported approval scope",
                        ));
                    }
                };
                Ok(ReadDecision::Accept {
                    chat_guids: chats
                        .iter()
                        .enumerate()
                        .filter(|(i, _)| content[format!("chat_{}", i + 1)] == true)
                        .map(|(_, c)| c.chat_guid.clone())
                        .collect(),
                    scope,
                })
            }
            _ => Err(Error::new(
                "messagesApprovalInvalid",
                "The host returned no valid approval decision",
            )),
        }
    }
}

pub fn serve<R: Read + Send + 'static, W: Write + Send + 'static>(
    reader: R,
    writer: W,
    mut engine: Engine,
    options: Options,
) -> Result<()> {
    let shared = Arc::new(Shared {
        output: Mutex::new(Box::new(writer)),
        routing: Mutex::new(Routing {
            calls: HashMap::new(),
            replies: HashMap::new(),
            elicitation: false,
            turn_lifecycle: false,
            turn_label: None,
        }),
        alive: AtomicBool::new(true),
    });
    let (sender, receiver) = mpsc::sync_channel::<Incoming>(32);
    let input_shared = shared.clone();
    thread::Builder::new()
        .name("messages-mcp-input".into())
        .spawn(move || read_loop(reader, input_shared, sender))
        .map_err(|_| transport_error())?;
    let mut initialized = false;
    let mut ready = false;
    let mut host = ElicitationHost {
        shared: shared.clone(),
        options,
    };
    let result = dispatch(
        &receiver,
        &shared,
        &mut engine,
        &mut host,
        &mut initialized,
        &mut ready,
    );
    shared.close();
    result
}
fn dispatch(
    receiver: &Receiver<Incoming>,
    shared: &Arc<Shared>,
    engine: &mut Engine,
    host: &mut ElicitationHost,
    initialized: &mut bool,
    ready: &mut bool,
) -> Result<()> {
    while let Ok(incoming) = receiver.recv() {
        if !shared.alive.load(Ordering::SeqCst) {
            break;
        }
        let message = incoming.message;
        let method = message["method"].as_str().unwrap_or("");
        let Some(id) = message.get("id").cloned() else {
            match method {
                "notifications/initialized" if *initialized => *ready = true,
                "notifications/icu/turn_started" if *ready => engine.next_turn()?,
                "notifications/icu/turn_ended" if *ready => engine.end_turn(),
                _ => {}
            }
            continue;
        };
        let response = match method {
            "ping" => json!({"jsonrpc":"2.0","id":id,"result":{}}),
            "initialize" if !*initialized => {
                let params = &message["params"];
                if !params["protocolVersion"].is_string()
                    || !params["capabilities"].is_object()
                    || !params["clientInfo"]["name"].is_string()
                {
                    rpc_error(id.clone(), -32602, "Invalid MCP initialization")
                } else {
                    let version = if params["protocolVersion"] == "2024-11-05" {
                        "2024-11-05"
                    } else {
                        "2025-06-18"
                    };
                    let elicitation = params["capabilities"]["elicitation"]
                        .as_object()
                        .is_some_and(|v| v.is_empty() || v.contains_key("form"));
                    let mut routing = shared.routing.lock().map_err(|_| transport_error())?;
                    routing.elicitation = elicitation;
                    routing.turn_lifecycle =
                        params["capabilities"]["experimental"]["icuTurnLifecycle"] == true;
                    *initialized = true;
                    json!({"jsonrpc":"2.0","id":id,"result":{"protocolVersion":version,"serverInfo":{"name":"Copilot Messages","version":env!("CARGO_PKG_VERSION")},"capabilities":{"tools":{"listChanged":false},"experimental":{"icuTurnLifecycle":true}}}})
                }
            }
            _ if !*ready => rpc_error(id.clone(), -32002, "MCP initialization has not completed"),
            "tools/list" => json!({"jsonrpc":"2.0","id":id,"result":{"tools":contract::tools()}}),
            "tools/call" => {
                let name = message["params"]["name"].as_str();
                if let Some(name) =
                    name.filter(|name| contract::tools().iter().any(|t| t["name"] == *name))
                {
                    let args = message["params"]
                        .get("arguments")
                        .cloned()
                        .unwrap_or(json!({}));
                    let mut result = engine.call(name, args, host, incoming.cancel.clone());
                    if incoming.cancel.load(Ordering::SeqCst) && name != "send_message" {
                        result = Err(Error::new(
                            "messagesRequestCanceled",
                            "Messages request was canceled before its response was released",
                        ));
                    }
                    call_response(id.clone(), result)
                } else {
                    rpc_error(id.clone(), -32602, "Unknown Messages tool")
                }
            }
            _ => rpc_error(id.clone(), -32601, "Unknown method"),
        };
        shared.finish(id, response)?;
    }
    Ok(())
}
