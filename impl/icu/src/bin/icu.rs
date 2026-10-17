//! invisible-computer-use, one binary.
//!
//!   icu mcp                          stdio MCP server (the entry point a project bundles)
//!   icu call <tool> [json]           one tool call through the same code path (exit 0/2/3/4/5/6/7, see exit_code)
//!   icu sky-pipe [--socket PATH]     the Codex client's length-prefixed JSON-RPC pipe (ping + request) over this layer (unix socket; Windows named pipe)
//!   icu tools                        the tool names (window2 names first, verbatim)
//!   icu capabilities [--text]        readiness: driver, permissions, host, features, typed errors, mode (same shape as the MCP tool)
//!   icu events [--window ID] [--interval MS] [--count N]   stream ax/focus/permission change events
//!   icu doctor                       driver health, patch status, packs, paths
//!   icu call stop_session|resume_session   operator-only (not over MCP): every tool refuses with userStoppedSession meanwhile
//!   icu mcp-config                   the client config snippet for this install
//!   icu memory serve [--interval MS] [--once] [--capture-only]   ambient capture (app focus only, no titles/text) + 10min/6h/phase2 scheduler (spec 03)
//!   icu <window2|confirmations|skysight|app-instructions> ...   the crates' own dev verbs
//!
//! Configuration is `ICU_*` environment (see `Config::from_env`): ICU_DRIVER, ICU_SOCKET,
//! ICU_HOME, ICU_STATE_DIR, ICU_MEMORY_ROOT, ICU_APPROVALS_SESSION, ICU_APPROVALS_ALWAYS,
//! ICU_DENIED_APPS, ICU_FORBIDDEN_APPS, ICU_REQUIRE_APP_APPROVAL, ICU_ALLOW_ALWAYS, ICU_RISK_LEVEL.
use icu::mcp::{Io, Server};
use icu::{Config, Layer};
use serde_json::{json, Value};

fn fail(msg: &str, code: i32) -> ! { eprintln!("[icu] {msg}"); std::process::exit(code); }

/// `icu call` exit codes: 0 ok, 2 bad arguments, 4 window/app not open, 5 accessibility degraded,
/// 6 refused by policy/approval, 7 screen locked, 3 any other refusal.
fn exit_code(e: &icu::ToolError) -> i32 {
    match e.code() {
        Some(icu::CODE_INVALID_ARGUMENT) => 2,
        Some(icu::CODE_RUNNING_APPLICATION_NOT_FOUND) => 4,
        Some(icu::CODE_ACCESSIBILITY_ERROR) | Some(icu::CODE_STALE_ELEMENT) => 5,
        Some(icu::CODE_APP_NOT_ALLOWED) | Some(icu::CODE_APP_APPROVAL_REQUIRED) => 6,
        Some(icu::CODE_SCREEN_LOCKED) => 7,
        _ => 3,
    }
}

fn connect() -> Layer {
    let cfg = Config::from_env();
    std::fs::create_dir_all(&cfg.state_dir).ok();
    Layer::connect(cfg).unwrap_or_else(|e| {
        println!("{}", serde_json::to_string_pretty(&e.structured).unwrap());
        fail(&e.message, exit_code(&e))
    })
}

fn main() {
    let a: Vec<String> = std::env::args().collect();
    match a.get(1).map(String::as_str) {
        Some("mcp") => {
            // one approval session per server start (Codex: per conversation) unless pinned
            if std::env::var("ICU_SESSION").map(|v| v.is_empty()).unwrap_or(true) {
                let t = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
                std::env::set_var("ICU_SESSION", format!("mcp-{t}-{}", std::process::id()));
            }
            let cfg = Config::from_env();
            let layer = Layer::connect(cfg.clone()).unwrap_or_else(|e| {
                Layer::with_cua(cfg, Box::new(icu::cua::UnavailableCua(icu::cua::CuaError {
                    message: e.message, structured: None,
                })))
            });
            let stdin = std::io::stdin();
            let stdout = std::io::stdout();
            let mut reader = stdin.lock();
            let mut writer = stdout.lock();
            let mut io = Io { reader: &mut reader, writer: &mut writer };
            Server::new(&layer).serve(&mut io).unwrap_or_else(|e| fail(&e.to_string(), 1));
        }
        Some("call") => {
            let tool = a.get(2).unwrap_or_else(|| fail("usage: icu call <tool> [json]", 2));
            let args: Value = a.get(3).map(|s| serde_json::from_str(s).unwrap_or_else(|e| fail(&format!("bad json: {e}"), 2))).unwrap_or(json!({}));
            let layer = connect();
            layer.set_one_shot();
            match layer.dispatch(tool, &args) {
                Ok(v) => println!("{}", serde_json::to_string_pretty(&v).unwrap()),
                Err(e) => { println!("{}", serde_json::to_string_pretty(&e.structured).unwrap()); eprintln!("[icu] {}", e.message); std::process::exit(exit_code(&e)); }
            }
        }
        Some("sky-pipe") => {
            // the Codex client's length-prefixed JSON-RPC pipe (spec 13) over this layer; a unix socket here, a named pipe below
            #[cfg(unix)]
            {
                let mut path: Option<std::path::PathBuf> = std::env::var("SKY_CUA_SERVICE_NATIVE_PIPE_PATH").ok().filter(|v| !v.trim().is_empty()).map(Into::into);
                let mut i = 2;
                while i < a.len() { if a[i] == "--socket" { path = a.get(i + 1).map(Into::into); i += 2; } else { i += 1; } }
                if std::env::var("ICU_SESSION").map(|v| v.is_empty()).unwrap_or(true) { std::env::set_var("ICU_SESSION", format!("sky-{}", std::process::id())); }
                let path = path.unwrap_or_else(|| {
                    let base = std::env::var("ICU_HOME").map(std::path::PathBuf::from).unwrap_or_else(|_| std::path::PathBuf::from(std::env::var("HOME").unwrap_or_default()).join(".icu"));
                    base.join("computeruse.sock")
                });
                let pipe = std::sync::Arc::new(icu::skypipe::SkyPipe::new(connect()));
                eprintln!("[icu] sky-pipe: serving {} on {}", icu::skypipe::API_VERSION, path.display());
                icu::skypipe::serve(pipe, &path).unwrap_or_else(|e| fail(&e.to_string(), 1));
            }
            #[cfg(windows)]
            {
                // the same protocol over a named pipe: `--socket NAME` or `\\.\pipe\NAME`
                let mut raw: Option<String> = std::env::var("SKY_CUA_SERVICE_NATIVE_PIPE_PATH").ok().filter(|v| !v.trim().is_empty());
                let mut i = 2;
                while i < a.len() { if a[i] == "--socket" { raw = a.get(i + 1).cloned(); i += 2; } else { i += 1; } }
                let path = match raw { Some(r) => icu::winpipe::pipe_path(&r).unwrap_or_else(|e| fail(&e, 2)), None => icu::winpipe::default_pipe_name(&icu::winpipe::user_name()) };
                if std::env::var("ICU_SESSION").map(|v| v.is_empty()).unwrap_or(true) { std::env::set_var("ICU_SESSION", format!("sky-{}", std::process::id())); }
                let pipe = std::sync::Arc::new(icu::skypipe::SkyPipe::new(connect()));
                eprintln!("[icu] sky-pipe: serving {} on {path}", icu::skypipe::API_VERSION);
                icu::winpipe::serve_pipe(pipe, &path).unwrap_or_else(|e| fail(&e.to_string(), 1));
            }
            #[cfg(not(any(unix, windows)))]
            fail("sky-pipe is not available on this platform", 2);
        }
        Some("tools") => { for n in icu::tool_names() { println!("{n}"); } }
        Some("capabilities") => {
            // total: a missing driver is a readiness value, not a crash
            let cfg = Config::from_env();
            let r = match Layer::connect(cfg.clone()) {
                Ok(layer) => layer.readiness(),
                Err(e) => icu::readiness::assess(None, None, Some(e.message), &cfg.mode, skysight::private::create_dir(&cfg.memory_root).is_ok()),
            };
            if a.iter().any(|x| x == "--text") { print!("{}", r.render()); } else { println!("{}", serde_json::to_string_pretty(&r.to_json()).unwrap()); }
        }
        Some("events") => {
            let layer = connect();
            let mut i = 2; let mut window: Option<u64> = None; let mut interval = 1000u64; let mut count = u64::MAX;
            while i < a.len() { match a[i].as_str() { "--window" => { window = a.get(i+1).and_then(|s| s.parse().ok()); i += 2; } "--interval" => { interval = a.get(i+1).and_then(|s| s.parse().ok()).unwrap_or(1000); i += 2; } "--count" => { count = a.get(i+1).and_then(|s| s.parse().ok()).unwrap_or(u64::MAX); i += 2; } _ => i += 1 } }
            let mut cursor = icu::exec::EventCursor::default();
            let mut emitted = 0u64;
            // prime the window pid map so ax events resolve
            let _ = layer.dispatch("list_windows", &json!({}));
            loop {
                let (events, next) = layer.poll_events(window, &cursor);
                for e in &events { println!("{}", serde_json::to_string(e).unwrap()); emitted += 1; }
                use std::io::Write; std::io::stdout().flush().ok();
                cursor = next;
                if emitted >= count { break; }
                std::thread::sleep(std::time::Duration::from_millis(interval));
            }
        }
        Some("doctor") => {
            let layer = connect();
            match layer.doctor() { Ok(v) => println!("{}", serde_json::to_string_pretty(&v).unwrap()), Err(e) => fail(&e.message, 1) }
        }
        Some("mcp-config") => {
            let exe = std::env::current_exe().map(|p| p.display().to_string()).unwrap_or_else(|_| "icu".into());
            let cfg = Config::from_env();
            let mut env = serde_json::Map::new();
            env.insert("ICU_DRIVER".into(), json!(cfg.driver));
            if let Some(s) = &cfg.socket { env.insert("ICU_SOCKET".into(), json!(s)); }
            println!("{}", serde_json::to_string_pretty(&json!({"mcpServers": {"invisible-computer-use": {"command": exe, "args": ["mcp"], "env": env}}})).unwrap());
        }
        Some("memory") if a.get(2).map(String::as_str) == Some("serve") => {
            // ambient capture loop (spec 03): poll the frontmost app into the event stream,
            // expire records past the retention window, and on each pass run the cadence
            // scheduler (closed 10min windows, closed 6h windows, then phase2) unless
            // --capture-only. Foreground + fifo-friendly; Ctrl-C to stop.
            let layer = connect();
            let mut i = 3; let mut interval = 5000u64; let mut once = false; let mut summarize = true;
            while i < a.len() { match a[i].as_str() { "--interval" => { interval = a.get(i+1).and_then(|s| s.parse().ok()).unwrap_or(5000); i += 2; } "--once" => { once = true; i += 1; } "--capture-only" => { summarize = false; i += 1; } _ => i += 1 } }
            let events = layer.events_path();
            eprintln!("[icu] memory serve: capturing app focus into {} every {interval}ms{}", events.display(), if summarize { ", summarizing closed windows" } else { "" });
            let mut sched = skysight::schedule::Scheduler::new();
            loop {
                match layer.capture_step(&events) { Ok(true) => { if let Ok(Some(e)) = std::fs::read_to_string(&events).map(|s| s.lines().last().map(String::from)) { println!("{e}"); } } Ok(false) => {} Err(e) => eprintln!("[icu] capture: {}", e.message) }
                if summarize {
                    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0);
                    match layer.memory_tick(&mut sched, now) {
                        Ok(t) if t["written"].as_array().is_some_and(|w| !w.is_empty()) || t["refused"].as_array().is_some_and(|r| !r.is_empty()) => eprintln!("[icu] summarize: {t}"),
                        Ok(_) => {}
                        Err(e) => eprintln!("[icu] summarize: {}", e.message),
                    }
                }
                use std::io::Write; std::io::stdout().flush().ok();
                if once { break; }
                std::thread::sleep(std::time::Duration::from_millis(interval));
            }
        }
        Some("window2") => window2::cli::run(a[1..].to_vec()),
        Some("confirmations") => confirmations::cli::run(a[1..].to_vec()),
        Some("skysight") | Some("memory") => skysight::cli::run(a[1..].to_vec()),
        Some("app-instructions") | Some("pack") => app_instructions::cli::run(a[1..].to_vec()),
        Some("--version") | Some("-V") => println!("icu {}", env!("CARGO_PKG_VERSION")),
        _ => fail("usage: icu <mcp|call|tools|doctor|mcp-config|window2|confirmations|skysight|app-instructions> ...", 2),
    }
}
