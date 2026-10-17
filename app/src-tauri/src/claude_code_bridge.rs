//! Test harness, not product code: serve the Ollama chat protocol on loopback
//! and answer each request by running local Claude Code through the app's
//! `claude-code` provider. A remote app (a cloud VM with no Claude login)
//! reaches it over a reverse ssh tunnel, so its decisions come from a real
//! model while the login never leaves this machine.
//!
//! ```sh
//! ICU_BRIDGE_PORT=11500 ICU_BRIDGE_LOG=/tmp/bridge.jsonl ICU_BRIDGE_MAX_CALLS=40 \
//!   cargo test --lib claude_code_bridge -- --ignored --nocapture
//! ```
//!
//! Each run has a random token (printed at start, or `ICU_BRIDGE_TOKEN` of at
//! least 32 characters), and only `POST /<token>/api/chat` carrying the app's
//! own decision or memory system prompt is answered, at most `ICU_BRIDGE_MAX_CALLS` times. The
//! system prompt alone is no gate: it is public in this repo. So a relay points
//! at `http://127.0.0.1:11500/<token>`, and anything else that reaches the port
//! gets 403 without spending quota. A connection has 10 s in all to send its
//! whole request (a deadline for the request, not a per-read timeout), so a
//! silent or trickling one cannot hold up the next for longer than that. The
//! request line and headers may take 16 KiB in all and the body 4 MiB; a longer
//! head gets 431 before the token is checked, so nothing buffers without bound.
//! Requests are still served one at a time. Every request appends one JSON line
//! to `ICU_BRIDGE_LOG`, with the token cut from the logged path.

use serde_json::{json, Value};
use std::io::{BufRead, BufReader, Read, Write};

/// 128 random bits as hex, from the OS-seeded keys std gives each `RandomState`.
fn random_token() -> String {
    use std::hash::{BuildHasher, Hasher};
    let half = || std::collections::hash_map::RandomState::new().build_hasher().finish();
    format!("{:016x}{:016x}", half(), half())
}

/// How long a connection may take to send its whole request.
const READ_TIMEOUT_SECS: u64 = 10;

/// The most the request line and headers together may take. Read before the token gate,
/// so any local process reaches it: one header line without a newline was buffered whole
/// (2 GiB in under a second, round-5 R5-X1).
const HEAD_MAX: u64 = 16 * 1024;

/// The connection's reads, each bounded by what is left of one deadline for the
/// whole request: a per-read timeout alone restarts on every byte, so a client
/// sending one byte every few seconds could hold the loop for ever.
struct Deadline<'a> { stream: &'a std::net::TcpStream, until: std::time::Instant }

impl Read for Deadline<'_> {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        let left = self.until.saturating_duration_since(std::time::Instant::now());
        if left.is_zero() {
            return Err(std::io::Error::new(std::io::ErrorKind::TimedOut, format!("request not sent within {READ_TIMEOUT_SECS} s")));
        }
        self.stream.set_read_timeout(Some(left))?;
        (&mut &*self.stream).read(buf)
    }
}

fn respond(stream: &mut std::net::TcpStream, status: &str, body: &Value) {
    let body = body.to_string();
    let _ = write!(stream, "HTTP/1.1 {status}\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}", body.len());
}

/// The request's path and body, or the status and reason to refuse it with.
fn read_request(stream: &std::net::TcpStream, until: std::time::Instant) -> Result<(String, Vec<u8>), (&'static str, String)> {
    let bad = |e: std::io::Error| ("400 Bad Request", e.to_string());
    let too_long = || ("431 Request Header Fields Too Large", format!("request line and headers exceed {} KiB", HEAD_MAX / 1024));
    let mut reader = BufReader::new(Deadline { stream, until });
    let mut head = (&mut reader).take(HEAD_MAX);
    let mut line = String::new();
    head.read_line(&mut line).map_err(bad)?;
    if head.limit() == 0 && !line.ends_with('\n') { return Err(too_long()); }
    let path = line.split_whitespace().nth(1).unwrap_or("").to_string();
    let mut len = 0usize;
    loop {
        let mut h = String::new();
        head.read_line(&mut h).map_err(bad)?;
        if head.limit() == 0 && !h.ends_with('\n') { return Err(too_long()); }
        if h.trim().is_empty() { break; }
        if let Some(v) = h.to_ascii_lowercase().strip_prefix("content-length:") { len = v.trim().parse().unwrap_or(0); }
    }
    if len > 4 * 1024 * 1024 { return Err(("400 Bad Request", "request too large".into())); }
    let mut body = vec![0; len];
    reader.read_exact(&mut body).map_err(bad)?;
    Ok((path, body))
}

/// After refusing a request the client is still sending: stop writing, then read and drop
/// a bounded amount (1 MiB, 1 s), so closing does not reset the connection and discard the
/// refusal before the client reads it.
fn linger(stream: &mut std::net::TcpStream) {
    let _ = stream.shutdown(std::net::Shutdown::Write);
    let _ = stream.set_read_timeout(Some(std::time::Duration::from_secs(1)));
    let until = std::time::Instant::now() + std::time::Duration::from_secs(1);
    let mut buf = [0u8; 16 * 1024];
    let mut left = 1024 * 1024usize;
    while left > 0 && std::time::Instant::now() < until {
        match stream.read(&mut buf) { Ok(0) | Err(_) => break, Ok(n) => left = left.saturating_sub(n) }
    }
}

/// Only the app's exact, known prompts can use the authenticated test relay.
fn request_kind(system: &str) -> Option<&'static str> {
    match system {
        crate::brain::SYSTEM => Some("decision"),
        crate::memory::SECRETS_PROMPT => Some("memory_secrets"),
        crate::memory::EXTRACT_PROMPT => Some("memory_extract"),
        crate::memory::CONSOLIDATE_PROMPT => Some("memory_consolidate"),
        crate::memory::JUDGE_PROMPT => Some("memory_judge"),
        _ => None,
    }
}

/// Answer the app's decision and memory requests on `listener` with `answer`, at most
/// `max_calls` of them; `log` gets one JSON line per request.
fn serve(listener: std::net::TcpListener, token: &str, max_calls: usize,
         answer: impl Fn(&str, &str) -> Result<String, String>, log: Option<&str>) {
    let mut calls = 0usize;
    for stream in listener.incoming() {
        let Ok(mut stream) = stream else { continue };
        let started = std::time::Instant::now();
        let _ = stream.set_write_timeout(Some(std::time::Duration::from_secs(READ_TIMEOUT_SECS)));
        let (path, body) = match read_request(&stream, started + std::time::Duration::from_secs(READ_TIMEOUT_SECS)) {
            Ok(r) => r,
            Err((status, e)) => { respond(&mut stream, status, &json!({"error": e})); if status.starts_with("431") { linger(&mut stream); } continue; }
        };
        let request: Value = serde_json::from_slice(&body).unwrap_or(Value::Null);
        let message = |role: &str| request.get("messages").and_then(Value::as_array)
            .and_then(|m| m.iter().find(|m| m.get("role").and_then(Value::as_str) == Some(role)))
            .and_then(|m| m.get("content")).and_then(Value::as_str).map(str::to_owned);
        let (system, user) = (message("system"), message("user"));
        let kind = system.as_deref().and_then(request_kind);
        let (authorized, path) = match path.strip_prefix('/').and_then(|p| p.split_once('/')) {
            Some((t, rest)) if !token.is_empty() && t == token => (true, format!("/{rest}")),
            _ => (false, if path.contains(token) && !token.is_empty() { path.replace(token, "<token>") } else { path }),
        };
        let mut entry = json!({"path": path, "at_unix_ms": std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_millis() as u64});
        if !authorized || path != "/api/chat" || kind.is_none() || user.is_none() {
            entry["refused"] = json!("not an authorized app request");
            respond(&mut stream, "403 Forbidden", &json!({"error": "this bridge only answers the app's known decision and memory requests"}));
        } else if calls >= max_calls {
            entry["refused"] = json!("call budget spent");
            respond(&mut stream, "429 Too Many Requests", &json!({"error": format!("bridge call budget of {max_calls} spent")}));
        } else {
            calls += 1;
            let out = answer(system.as_deref().unwrap(), user.as_deref().unwrap());
            entry["kind"] = json!(kind.unwrap());
            entry["call"] = json!(calls);
            entry["user_prompt"] = json!(user);
            match &out {
                Ok(text) => { entry["reply"] = json!(text); respond(&mut stream, "200 OK", &json!({"message": {"role": "assistant", "content": text}})); }
                Err(e) => { entry["error"] = json!(e); respond(&mut stream, "502 Bad Gateway", &json!({"error": e})); }
            }
        }
        entry["ms"] = json!(started.elapsed().as_millis() as u64);
        println!("BRIDGE {}", json!({"call": entry.get("call"), "ms": entry["ms"], "ok": entry.get("reply").is_some(), "error": entry.get("error"), "refused": entry.get("refused")}));
        if let Some(path) = log {
            if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(path) { let _ = writeln!(f, "{entry}"); }
        }
    }
}

#[test]
#[ignore = "serves a real model over loopback; spends the Claude Code login's quota"]
fn claude_code_bridge() {
    let port: u16 = std::env::var("ICU_BRIDGE_PORT").ok().and_then(|p| p.parse().ok()).unwrap_or(11500);
    let max_calls: usize = std::env::var("ICU_BRIDGE_MAX_CALLS").ok().and_then(|p| p.parse().ok()).unwrap_or(40);
    let model = std::env::var("ICU_BRAIN_MODEL").unwrap_or_else(|_| "claude-sonnet-5".into());
    let log = std::env::var("ICU_BRIDGE_LOG").ok();
    let token = std::env::var("ICU_BRIDGE_TOKEN").ok().filter(|t| !t.is_empty()).unwrap_or_else(random_token);
    assert!(token.len() >= 32 && token.bytes().all(|b| b.is_ascii_alphanumeric()), "ICU_BRIDGE_TOKEN must be 32 or more letters and digits");
    let listener = std::net::TcpListener::bind(("127.0.0.1", port)).unwrap();
    println!("BRIDGE listening 127.0.0.1:{port} model={model} max_calls={max_calls}");
    println!("BRIDGE relay to http://127.0.0.1:{port}/{token}");
    serve(listener, &token, max_calls, |system, user| crate::brain::claude_code_complete(None, &model, system, user), log.as_deref());
}

#[test]
#[ignore = "serves the real Codex model over loopback using this machine's login"]
fn codex_oauth_bridge(){
    use crate::brain::Brain;
    let port:u16=std::env::var("ICU_BRIDGE_PORT").expect("explicit port").parse().unwrap();
    let max_calls:usize=std::env::var("ICU_BRIDGE_MAX_CALLS").expect("explicit call budget").parse().unwrap();
    let token=std::env::var("ICU_BRIDGE_TOKEN").expect("explicit private token");
    assert!(token.len()>=32&&token.bytes().all(|b|b.is_ascii_alphanumeric()));
    let model=std::env::var("ICU_BRAIN_MODEL").expect("explicit model");
    let brain=crate::brain::CodexOAuth{model:model.clone()};
    let log=std::env::var("ICU_BRIDGE_LOG").ok();
    let listener=std::net::TcpListener::bind(("127.0.0.1",port)).unwrap();
    println!("BRIDGE listening 127.0.0.1:{port} model={model} max_calls={max_calls}");
    serve(listener,&token,max_calls,|system,user|brain.complete(system,user),log.as_deref());
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{Duration, Instant};

    const TOKEN: &str = "0123456789abcdef0123456789abcdef";

    /// A bridge on a free port whose model echoes the turn; returns its address.
    fn start(max_calls: usize) -> std::net::SocketAddr {
        let listener = std::net::TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let addr = listener.local_addr().unwrap();
        std::thread::spawn(move || serve(listener, TOKEN, max_calls, |_, user| Ok(format!("echo {user}")), None));
        addr
    }

    fn post(addr: std::net::SocketAddr, path: &str, system: &str) -> String {
        let body = json!({"model": "m", "messages": [{"role": "system", "content": system}, {"role": "user", "content": "hi"}]}).to_string();
        let mut s = std::net::TcpStream::connect(addr).unwrap();
        s.set_read_timeout(Some(Duration::from_secs(20))).unwrap();
        write!(s, "POST {path} HTTP/1.1\r\nhost: x\r\ncontent-type: application/json\r\ncontent-length: {}\r\n\r\n{body}", body.len()).unwrap();
        let mut out = String::new();
        let _ = s.read_to_string(&mut out);
        out.lines().next().unwrap_or("").to_owned()
    }

    #[test]
    fn only_a_request_with_the_runs_token_is_answered() {
        // Round 3: the app's SYSTEM prompt is public, so it cannot be the gate.
        let addr = start(5);
        assert!(post(addr, "/api/chat", crate::brain::SYSTEM).contains("403"), "no token");
        assert!(post(addr, "/wrong-token/api/chat", crate::brain::SYSTEM).contains("403"), "wrong token");
        assert!(post(addr, &format!("/{TOKEN}/api/chat"), "another prompt").contains("403"), "not the app's prompt");
        assert!(post(addr, &format!("/{TOKEN}/api/chat"), crate::brain::SYSTEM).contains("200"), "the app, with the token");
        let (a, b) = (random_token(), random_token());
        assert!(a.len() == 32 && a != b, "{a} {b}");
    }

    #[test]
    fn memory_requests_preserve_their_prompt_and_share_the_call_budget() {
        let listener = std::net::TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let addr = listener.local_addr().unwrap();
        let (send, received) = std::sync::mpsc::channel();
        std::thread::spawn(move || serve(listener, TOKEN, 5, |system, user| {
            send.send((system.to_owned(), user.to_owned())).unwrap();
            Ok("{}".into())
        }, None));
        let path = format!("/{TOKEN}/api/chat");
        assert!(post(addr, "/wrong-token/api/chat", crate::memory::SECRETS_PROMPT).contains("403"));
        assert!(post(addr, &path, &format!("{} modified", crate::memory::SECRETS_PROMPT)).contains("403"));
        for system in [crate::brain::SYSTEM, crate::memory::SECRETS_PROMPT, crate::memory::EXTRACT_PROMPT,
                       crate::memory::CONSOLIDATE_PROMPT, crate::memory::JUDGE_PROMPT] {
            assert!(post(addr, &path, system).contains("200"));
            assert_eq!(received.recv_timeout(Duration::from_secs(1)).unwrap(), (system.to_owned(), "hi".into()));
        }
        assert!(post(addr, &path, crate::brain::SYSTEM).contains("429"));
        assert!(post(addr, &path, crate::memory::SECRETS_PROMPT).contains("429"));
        assert!(received.try_recv().is_err());
    }

    #[test]
    fn the_token_never_reaches_the_log() {
        // Round 4 (app-r4 F2, mutation A1): the cut was unpinned. The authorized path is
        // logged without its token, and a refused path that holds it has it replaced.
        let log = std::env::temp_dir().join(format!("icu-bridge-log-{}-{:?}.jsonl", std::process::id(), Instant::now()));
        let listener = std::net::TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let addr = listener.local_addr().unwrap();
        let log_path = log.to_str().unwrap().to_owned();
        std::thread::spawn(move || serve(listener, TOKEN, 5, |_, user| Ok(format!("echo {user}")), Some(&log_path)));
        assert!(post(addr, &format!("/{TOKEN}/api/chat"), crate::brain::SYSTEM).contains("200"));
        assert!(post(addr, &format!("/x/{TOKEN}/api/chat"), crate::brain::SYSTEM).contains("403"));
        assert!(post(addr, &format!("/{TOKEN}/api/tags"), crate::brain::SYSTEM).contains("403"));
        let text = std::fs::read_to_string(&log).unwrap();
        let paths: Vec<String> = text.lines().map(|l| serde_json::from_str::<Value>(l).unwrap()["path"].as_str().unwrap().to_owned()).collect();
        assert_eq!(paths, ["/api/chat", "/x/<token>/api/chat", "/api/tags"], "{text}");
        let _ = std::fs::remove_file(&log);
        assert!(!text.contains(TOKEN), "{text}");
    }

    #[test]
    fn a_silent_connection_does_not_block_the_next_request() {
        let addr = start(5);
        let _silent = std::net::TcpStream::connect(addr).unwrap();
        let started = Instant::now();
        assert!(post(addr, &format!("/{TOKEN}/api/chat"), crate::brain::SYSTEM).contains("200"));
        assert!(started.elapsed() < Duration::from_secs(READ_TIMEOUT_SECS + 5), "{:?}", started.elapsed());
    }

    #[test]
    fn a_trickling_connection_does_not_block_the_next_request() {
        // Round-4 review F4: one byte every 7 s never trips a 10 s per-read timeout,
        // and the request behind it waited 51.5 s. Here a byte a second, for 40 s.
        let addr = start(5);
        let mut trickle = std::net::TcpStream::connect(addr).unwrap();
        std::thread::spawn(move || for b in b"POST /x HTTP/1.1\r\nhost: x\r\nx-pad: 0123456789012345678901" {
            if trickle.write_all(&[*b]).is_err() { break }
            std::thread::sleep(Duration::from_secs(1));
        });
        std::thread::sleep(Duration::from_millis(300));
        let started = Instant::now();
        assert!(post(addr, &format!("/{TOKEN}/api/chat"), crate::brain::SYSTEM).contains("200"));
        assert!(started.elapsed() < Duration::from_secs(READ_TIMEOUT_SECS + 5), "{:?}", started.elapsed());
    }

    /// Send `head` and wait for the answer without closing; the status line and how long it took.
    fn send_head(addr: std::net::SocketAddr, head: &[u8]) -> (String, Duration) {
        let mut s = std::net::TcpStream::connect(addr).unwrap();
        s.set_read_timeout(Some(Duration::from_secs(20))).unwrap();
        let started = Instant::now();
        let _ = s.write_all(head);
        let mut out = Vec::new();
        let _ = s.read_to_end(&mut out);
        (String::from_utf8_lossy(&out).lines().next().unwrap_or("").to_owned(), started.elapsed())
    }

    #[test]
    fn a_request_head_past_its_cap_is_refused_at_once() {
        // Round-5 R5-X1: one header line without a newline was buffered whole, before the
        // token gate (2 GiB in 0.8 s). Now the head is capped, and the refusal does not wait
        // for a newline or the deadline.
        let addr = start(5);
        let mut endless = format!("POST /{TOKEN}/api/chat HTTP/1.1\r\nx-pad: ").into_bytes();
        endless.resize(endless.len() + 64 * 1024, b'a');
        let (status, took) = send_head(addr, &endless);
        assert!(status.contains("431") && took < Duration::from_secs(5), "{status} after {took:?}");
        let mut line = b"POST /".to_vec();
        line.resize(64 * 1024, b'a');
        let (status, _) = send_head(addr, &line);
        assert!(status.contains("431"), "an endless request line: {status}");
        let many: String = (0..400).map(|i| format!("x-h{i}: {}\r\n", "b".repeat(40))).collect();
        let (status, _) = send_head(addr, format!("POST /{TOKEN}/api/chat HTTP/1.1\r\n{many}\r\n").as_bytes());
        assert!(status.contains("431"), "many short headers: {status}");
        // A head just under the cap is still read, and the request answered.
        let body = json!({"model": "m", "messages": [{"role": "system", "content": crate::brain::SYSTEM}, {"role": "user", "content": "hi"}]}).to_string();
        let pad = "c".repeat(HEAD_MAX as usize - 200);
        let req = format!("POST /{TOKEN}/api/chat HTTP/1.1\r\nx-pad: {pad}\r\ncontent-length: {}\r\n\r\n{body}", body.len());
        let (status, _) = send_head(addr, req.as_bytes());
        assert!(status.contains("200"), "a head under the cap: {status}");
    }
}
