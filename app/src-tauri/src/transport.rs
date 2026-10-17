//! Direct daemon framing with one deadline, a frame cap, and explicit replay safety.
use serde_json::Value;
use crate::action_failure::ActionFailure;
use std::fmt;

#[derive(Debug)]
pub enum Error {
    Unavailable(String), // No action bytes were sent; CLI fallback is permitted.
    Failed(String),      // Preflight refused or delivery is uncertain. No replay.
    Tool(ActionFailure), // The native call answered; preserve its delivery evidence.
}
impl Error {
    pub fn before_send(&self) -> bool { matches!(self, Self::Unavailable(_)) }
}
impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self { Self::Unavailable(s) | Self::Failed(s) => f.write_str(s), Self::Tool(error) => error.fmt(f) }
    }
}

/// Fallback is permitted only before action bytes are sent, and never after a
/// compatibility refusal, even though that refusal also precedes the action.
pub fn deliver<E: Into<ActionFailure>>(direct: Result<Value, Error>, fallback: impl FnOnce() -> Result<Value, E>) -> Result<Value, ActionFailure> {
    match direct {
        Ok(value) => Ok(value),
        Err(error) if error.before_send() => fallback().map_err(Into::into),
        Err(Error::Tool(error)) => Err(error),
        Err(error) => Err(ActionFailure::from(error.to_string())),
    }
}

/// A completed daemon exchange can contain a failed MCP tool invocation. Keep
/// that distinction before callers discard the envelope's `isError` field.
fn check_tool_result(result: &Value) -> Result<(), Error> {
    match action_outcomes::DriverError::from_tool_result(result) {
        Some(error) => Err(Error::Tool(ActionFailure::Driver(error))),
        None => Ok(()),
    }
}

#[cfg(test)]
mod delivery_tests {
    use super::*;
    #[cfg(unix)]
    #[test]
    fn passive_status_checks_have_a_short_total_deadline() {
        for name in ["check_permissions","health_report"] {
            assert_eq!(request_budget(&serde_json::json!({"method":"call","name":name})),std::time::Duration::from_millis(1500));
        }
        assert_eq!(request_budget(&serde_json::json!({"method":"call","name":"get_window_state"})),std::time::Duration::from_secs(12));
    }

    #[test]
    fn a_tool_error_keeps_the_drivers_typed_code() {
        let error = |result: Value| check_tool_result(&result).unwrap_err().to_string();
        assert_eq!(error(serde_json::json!({"isError":true,"content":[{"type":"text","text":"value 150 is above the maximum 100"}],
            "structuredContent":{"code":"set_value_out_of_range"}})), "daemon tool error (set_value_out_of_range): value 150 is above the maximum 100");
        assert_eq!(error(serde_json::json!({"isError":true,"content":[{"type":"text","text":"Task error: gone"}]})), "daemon tool error: Task error: gone");
        // A code the text already names is not repeated.
        assert_eq!(error(serde_json::json!({"isError":true,"content":[{"type":"text","text":"refused (stale_capture): resized"}],
            "structuredContent":{"code":"stale_capture"}})), "daemon tool error: refused (stale_capture): resized");
        assert_eq!(error(serde_json::json!({"isError":true,"structuredContent":{"code":"stale_capture"}})), "daemon tool error: stale_capture");
        // A code merely mentioned later in the text is still named.
        assert_eq!(error(serde_json::json!({"isError":true,"content":[{"type":"text","text":"the stale_capture check failed"}],
            "structuredContent":{"code":"stale_capture"}})), "daemon tool error (stale_capture): the stale_capture check failed");
    }
    #[test]
    fn a_tool_error_keeps_the_drivers_effect() {
        let error = |result: Value| check_tool_result(&result).unwrap_err().to_string();
        // macOS type_text: 5 of 11 characters landed.
        assert_eq!(error(serde_json::json!({"isError":true,"content":[{"type":"text","text":"type_text incomplete: delivered 5 of 11 character(s)"}],
            "structuredContent":{"code":"type_text_incomplete","effect":"partial","delivered_chars":5}})),
            "daemon tool error (type_text_incomplete; effect partial): type_text incomplete: delivered 5 of 11 character(s)");
        // Linux folds its code into the text; the effect is still carried.
        assert_eq!(error(serde_json::json!({"isError":true,"content":[{"type":"text","text":"foreground_input_unavailable (text_interrupted): X connection reset"}],
            "structuredContent":{"code":"foreground_input_unavailable","reason":"text_interrupted","effect":"partial","delivery":{"mode":"foreground","delivered_count":3}}})),
            "daemon tool error (effect partial): foreground_input_unavailable (text_interrupted): X connection reset");
        assert_eq!(error(serde_json::json!({"isError":true,"content":[{"type":"text","text":"refused (stale_capture): resized"}],
            "structuredContent":{"code":"stale_capture","effect":"refused"}})), "daemon tool error (effect refused): refused (stale_capture): resized");
    }
    #[test]
    fn unavailable_transport_reaches_cli_but_remote_errors_never_replay() {
        let calls = std::cell::Cell::new(0);
        let fallback = || -> Result<Value, String> { calls.set(calls.get() + 1); Ok(serde_json::json!({"delivered":true})) };
        assert!(deliver(Err(Error::Unavailable("unix transport unavailable".into())), fallback).is_ok());
        assert_eq!(calls.get(), 1);
        assert!(deliver(Err(Error::Failed("session cleanup failed after input".into())), fallback).is_err());
        assert_eq!(calls.get(), 1);
    }

    #[test]
    fn typed_partial_outcome_survives_delivery_without_cli_replay() {
        let payload = serde_json::json!({"code":"action_outcome_unknown","status":"uncertain",
            "effect":"partial","delivery":{"mode":"background","delivered_count":3}});
        let reply = serde_json::json!({"isError":true,"structuredContent":payload,
            "content":[{"type":"text","text":"refused (stale_capture): misleading prose"}]});
        let error = deliver::<ActionFailure>(check_tool_result(&reply).map(|()| reply), || panic!("answered action must never replay")).unwrap_err();
        assert_eq!(error.code(), Some("action_outcome_unknown"));
        assert_eq!(error.effect(), Some("partial"));
        assert!(!error.is_refused());
        assert_eq!(error.evidence()["status"], "uncertain");
        assert_eq!(error.evidence()["delivery"]["delivered_count"], 3);
        let ActionFailure::Driver(driver) = error else { panic!("lost typed native answer") };
        assert_eq!(driver.structured, Some(payload));
    }
}

#[cfg(unix)]
pub fn roundtrip(path: &str, request: &Value) -> Result<Value, Error> {
    let result = roundtrip_tool_result(path, request)?;
    check_tool_result(&result)?;
    Ok(result)
}

/// Same exchange, but a completed tool call that failed comes back as its
/// result, so a read-only caller can read the tool's typed refusal from
/// `structuredContent` instead of a flattened error string.
#[cfg(unix)]
pub fn roundtrip_tool_result(path: &str, request: &Value) -> Result<Value, Error> {
    let stream = connect(path).map_err(|e| Error::Unavailable(format!("daemon connect: {e}")))?;
    session_exchange(stream, request, request_budget(request), 32 * 1024 * 1024)
}

#[cfg(unix)]
fn request_budget(request: &Value) -> std::time::Duration {
    // Permission-banner polling is passive and must not stall the app behind
    // a long capture/input budget when the selected driver is unresponsive.
    if request.get("method").and_then(Value::as_str) == Some("call")
        && matches!(request.get("name").and_then(Value::as_str), Some("check_permissions" | "health_report")) {
        return std::time::Duration::from_millis(1500);
    }
    // Linux input can wait for native Remote Desktop consent (20 s), bounded
    // cleanup/readiness, then delivery acknowledgement. Let the driver return
    // its typed outcome instead of abandoning a still-pending consent request.
    // Read-only polling keeps its short budget. No failure authorizes replay.
    #[cfg(target_os = "linux")]
    if request.get("method").and_then(Value::as_str) == Some("call")
        && matches!(request.get("name").and_then(Value::as_str),
            Some("click" | "type_text" | "press_key" | "scroll" | "drag"
                | "mouse_button_down" | "mouse_button_up" | "mouse_drag"
                | "desktop_click" | "desktop_type_text" | "desktop_press_key"
                | "desktop_scroll" | "desktop_drag" | "desktop_drag_handle"))
    {
        return std::time::Duration::from_secs(70);
    }
    let _ = request;
    std::time::Duration::from_secs(12)
}

#[cfg(unix)]
fn session_exchange(mut stream: std::os::unix::net::UnixStream, request: &Value, budget: std::time::Duration, limit: usize) -> Result<Value, Error> {
    let deadline = std::time::Instant::now() + budget;
    let probe_deadline = deadline.min(std::time::Instant::now() + std::time::Duration::from_millis(1500));
    let metadata = exchange(&mut stream, &serde_json::json!({"method":"metadata","client_kind":"cli"}), probe_deadline, limit)?;
    validate_metadata(&metadata)?;
    // The handshake and action share the connection and total deadline. A
    // failed handshake never falls through to action dispatch or CLI replay.
    exchange(&mut stream, request, deadline, limit)
}

#[derive(serde::Deserialize)]
struct Metadata {
    driver_version: String,
    contract_version: String,
    tools_list_schema_version: String,
    capability_version: String,
    mcp_protocol_version: String,
    pid: u32,
    embedded: bool,
    host_bundle_id: Option<String>,
}

fn validate_metadata(value: &Value) -> Result<(), Error> {
    use cua_driver_contract::{CONTRACT_VERSION, TOOLS_LIST_SCHEMA_VERSION, CAPABILITY_VERSION, MCP_PROTOCOL_VERSION};
    let meta: Metadata = serde_json::from_value(value.clone())
        .map_err(|e| Error::Failed(format!("invalid daemon metadata: {e}")))?;
    if meta.driver_version.is_empty() || meta.pid == 0 {
        return Err(Error::Failed("invalid daemon process identity".into()));
    }
    // These fields are decoded to enforce the same metadata shape as the SDK.
    let _host = (meta.embedded, meta.host_bundle_id);
    for (name, actual, expected) in [
        ("contract", meta.contract_version.as_str(), CONTRACT_VERSION),
        ("tool schema", meta.tools_list_schema_version.as_str(), TOOLS_LIST_SCHEMA_VERSION),
        ("capability", meta.capability_version.as_str(), CAPABILITY_VERSION),
        ("MCP protocol", meta.mcp_protocol_version.as_str(), MCP_PROTOCOL_VERSION),
    ] {
        if actual != expected {
            return Err(Error::Failed(format!("incompatible daemon {name}: {actual}; expected {expected}")));
        }
    }
    Ok(())
}

#[cfg(unix)]
pub fn connect(path: &str) -> std::io::Result<std::os::unix::net::UnixStream> {
    let socket = socket2::Socket::new(socket2::Domain::UNIX, socket2::Type::STREAM, None)?;
    socket.connect_timeout(&socket2::SockAddr::unix(path)?, std::time::Duration::from_secs(1))?;
    let fd: std::os::fd::OwnedFd = socket.into();
    Ok(fd.into())
}

#[cfg(not(unix))]
pub fn roundtrip(_path: &str, _request: &Value) -> Result<Value, Error> {
    Err(Error::Unavailable("direct socket transport unavailable on this platform".into()))
}

#[cfg(not(unix))]
pub fn roundtrip_tool_result(_path: &str, _request: &Value) -> Result<Value, Error> {
    Err(Error::Unavailable("direct socket transport unavailable on this platform".into()))
}

#[cfg(unix)]
fn exchange(stream: &mut std::os::unix::net::UnixStream, request: &Value, deadline: std::time::Instant, limit: usize) -> Result<Value, Error> {
    use std::io::{Read, Write};
    use std::time::Instant;
    let remaining = || deadline.checked_duration_since(Instant::now())
        .filter(|d| !d.is_zero()).ok_or_else(|| Error::Failed("daemon deadline exceeded; action outcome may be unknown".into()));
    let fail = |e| Error::Failed(format!("daemon transport: {e}"));
    let mut line = serde_json::to_vec(request).map_err(|e| Error::Unavailable(format!("daemon encode: {e}")))?;
    line.push(b'\n');
    if line.len() > limit { return Err(Error::Unavailable("daemon request exceeds frame limit".into())); }
    let mut written = 0;
    while written < line.len() {
        stream.set_write_timeout(Some(remaining()?)).map_err(fail)?;
        match stream.write(&line[written..]) {
            Ok(0) => return Err(Error::Failed("daemon write closed".into())),
            Ok(n) => written += n,
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(e) => return Err(fail(e)),
        }
    }
    let mut buf = Vec::new();
    let mut chunk = [0; 64 * 1024];
    loop {
        match stream.set_read_timeout(Some(remaining()?)) {
            // Darwin rejects SO_RCVTIMEO with EINVAL once the peer has closed.
            // Its buffered bytes and EOF still arrive without blocking.
            Err(e) if e.kind() == std::io::ErrorKind::InvalidInput && cfg!(target_os = "macos") => {}
            other => other.map_err(fail)?,
        }
        let n = match stream.read(&mut chunk) {
            Ok(0) => return Err(Error::Failed("daemon closed before a complete frame".into())),
            Ok(n) => n,
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(e) => return Err(fail(e)),
        };
        let end = chunk[..n].iter().position(|b| *b == b'\n');
        let take = end.unwrap_or(n);
        if buf.len() + take > limit { return Err(Error::Failed("daemon response exceeds frame limit".into())); }
        buf.extend_from_slice(&chunk[..take]);
        if end.is_some() { break; }
    }
    let response: Value = serde_json::from_slice(&buf).map_err(|e| Error::Failed(format!("daemon parse: {e}")))?;
    if response.get("ok").and_then(Value::as_bool) != Some(true) {
        return Err(Error::Failed(response.get("error").and_then(Value::as_str).unwrap_or("daemon !ok").into()));
    }
    let result = response.get("result").ok_or_else(|| Error::Failed("daemon: no result".into()))?;
    Ok(result.clone())
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::{io::{Read, Write}, os::unix::net::UnixStream, time::{Duration, Instant}};
    #[cfg(target_os = "linux")]
    #[test]
    fn linux_input_budget_outlives_portal_readiness_and_delivery() {
        let input = serde_json::json!({"method":"call", "name":"press_key"});
        let read = serde_json::json!({"method":"call", "name":"get_window_state"});
        assert!(request_budget(&input) > Duration::from_secs(60));
        assert_eq!(request_budget(&read), Duration::from_secs(12));
        assert_eq!(request_budget(&serde_json::json!({"method":"metadata", "name":"press_key"})), Duration::from_secs(12));
    }
    fn run_server(f: impl FnOnce(UnixStream) + Send + 'static, budget: Duration, limit: usize) -> Result<Value, Error> {
        let (mut client, mut server) = UnixStream::pair().unwrap();
        let thread = std::thread::spawn(move || {
            let mut b = [0];
            while server.read_exact(&mut b).is_ok() && b[0] != b'\n' {}
            f(server);
        });
        let result = exchange(&mut client, &serde_json::json!({"method":"call"}), Instant::now() + budget, limit);
        drop(client);
        thread.join().unwrap();
        result
    }
    #[test]
    fn drip_feed_cannot_extend_the_deadline() {
        let start = Instant::now();
        let result = run_server(|mut s| loop {
            if s.write_all(b" ").is_err() { break; }
            std::thread::sleep(Duration::from_millis(10));
        }, Duration::from_millis(100), 1024);
        assert!(result.is_err());
        assert!(!result.unwrap_err().before_send());
        assert!(start.elapsed() < Duration::from_secs(1));
    }
    #[test]
    fn oversized_frames_are_rejected() {
        let err = run_server(|mut s| { let _ = s.write_all(&[b'x'; 512]); }, Duration::from_secs(1), 128).unwrap_err();
        assert!(err.to_string().contains("frame limit"));
    }
    #[test]
    fn fragmented_response_is_accepted() {
        let result = run_server(|mut s| {
            for b in b"{\"ok\":true,\"result\":{\"value\":42}}\n" { s.write_all(&[*b]).unwrap(); }
        }, Duration::from_secs(1), 1024).unwrap();
        assert_eq!(result["value"], 42);
    }
    #[test]
    fn remote_error_mentioning_session_never_authorizes_replay() {
        let err = run_server(|mut s| {
            s.write_all(b"{\"ok\":false,\"error\":\"session cleanup failed after input\"}\n").unwrap();
        }, Duration::from_secs(1), 1024).unwrap_err();
        assert!(!err.before_send());
    }
}

#[cfg(all(test, unix))]
mod compatibility_tests {
    use super::*;
    use std::{io::{BufRead, BufReader, Write}, os::unix::net::UnixStream, time::Duration};

    fn metadata() -> Value {
        serde_json::json!({
            "driver_version":"0.24.0", "pid":42, "embedded":true,
            "contract_version":cua_driver_contract::CONTRACT_VERSION,
            "tools_list_schema_version":cua_driver_contract::TOOLS_LIST_SCHEMA_VERSION,
            "capability_version":cua_driver_contract::CAPABILITY_VERSION,
            "mcp_protocol_version":cua_driver_contract::MCP_PROTOCOL_VERSION,
        })
    }
    #[test]
    fn captured_native_driver_metadata_matches_the_app_contract() {
        let capture: Value = serde_json::from_str(include_str!("../../tests/fixtures/driver-metadata.json")).unwrap();
        validate_metadata(&capture["metadata"]).unwrap();
    }
    #[test]
    #[ignore = "requires an explicitly supplied test daemon; metadata and discovery only"]
    fn native_daemon_handshake_and_discovery() {
        let socket = std::env::var("ICU_TEST_PROTOCOL_SOCKET").expect("set the private test socket");
        let result = roundtrip(&socket, &serde_json::json!({"method":"list","client_kind":"cli"})).unwrap();
        let tools = result.get("tools").and_then(Value::as_array).expect("native tool list");
        assert!(tools.iter().any(|tool| tool["name"] == "click"));
        eprintln!("native tool count: {}", tools.len());
    }
    fn serve(meta: Value) -> (Result<Value, Error>, Vec<String>) {
        let (client, mut server) = UnixStream::pair().unwrap();
        let thread = std::thread::spawn(move || {
            server.set_read_timeout(Some(Duration::from_secs(2))).unwrap();
            let mut reader = BufReader::new(server.try_clone().unwrap());
            let mut methods = Vec::new();
            loop {
                let mut line = String::new();
                if reader.read_line(&mut line).unwrap_or(0) == 0 { break; }
                let request: Value = serde_json::from_str(&line).unwrap();
                let method = request["method"].as_str().unwrap().to_owned();
                methods.push(method.clone());
                let result = if method == "metadata" { meta.clone() } else { serde_json::json!({"accepted":true}) };
                writeln!(server, "{}", serde_json::json!({"ok":true,"result":result})).unwrap();
                // The real daemon keeps the connection open for subsequent
                // protocol requests. EOF from the client ends this fixture.
            }
            methods
        });
        let result = session_exchange(client, &serde_json::json!({"method":"call","name":"click","args":{}}), Duration::from_secs(1), 4096);
        (result, thread.join().unwrap())
    }
    #[test]
    fn incompatible_contracts_refuse_before_sending_an_action() {
        for key in ["contract_version", "tools_list_schema_version", "capability_version", "mcp_protocol_version"] {
            let mut wrong = metadata(); wrong[key] = serde_json::json!("incompatible");
            let (result, methods) = serve(wrong);
            assert!(result.is_err(), "{key}: action unexpectedly accepted");
            assert_eq!(methods, ["metadata"], "{key}: mutation reached an incompatible daemon");
        }
    }
    #[test]
    fn compatible_daemon_receives_exactly_one_action_after_metadata() {
        let (result, methods) = serve(metadata());
        assert_eq!(result.unwrap()["accepted"], true);
        assert_eq!(methods, ["metadata", "call"]);
    }
    #[test]
    fn missing_contract_metadata_never_reaches_action_dispatch() {
        let (result, methods) = serve(serde_json::json!({}));
        assert!(result.is_err());
        assert_eq!(methods, ["metadata"]);
    }
    #[test]
    fn malformed_identity_metadata_never_reaches_action_dispatch() {
        for key in ["driver_version", "pid", "embedded"] {
            let mut wrong = metadata(); wrong[key] = Value::Null;
            let (result, methods) = serve(wrong);
            assert!(result.is_err(), "{key}");
            assert_eq!(methods, ["metadata"]);
        }
    }
    #[test]
    fn handshake_and_action_share_one_deadline() {
        let (client, mut server) = UnixStream::pair().unwrap();
        let thread = std::thread::spawn(move || {
            let mut reader = BufReader::new(server.try_clone().unwrap());
            for result in [metadata(), serde_json::json!({"accepted":true})] {
                let mut line = String::new();
                if reader.read_line(&mut line).unwrap_or(0) == 0 { break; }
                std::thread::sleep(Duration::from_millis(250));
                if writeln!(server, "{}", serde_json::json!({"ok":true,"result":result})).is_err() { break; }
            }
        });
        let result = session_exchange(client, &serde_json::json!({"method":"call","name":"click","args":{}}), Duration::from_millis(400), 4096);
        assert!(result.is_err(), "a renewed per-request deadline would incorrectly accept this response");
        assert!(!result.unwrap_err().before_send());
        thread.join().unwrap();
    }
}
