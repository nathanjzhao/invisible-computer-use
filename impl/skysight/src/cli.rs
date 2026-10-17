//! The crate's command-line verbs as a library entry point, so the `icu` binary can host
//! them (`icu skysight ...`) and the standalone binary stays a thin shim.
//! CLI over the skysight layer, for the conformance harness and evidence runs.
//!
//!   skysight prompt summarizer|memory                       verbatim prompt text
//!   skysight windows --events FILE --cadence 10min|6h        JSON list of window starts
//!   skysight summarize --events FILE --window START --root DIR [--cadence 10min]   write a 10min summary (reference summarizer), print its path
//!   skysight rollup --window START --root DIR                write the 6h summary from the child 10min summaries, print its path
//!   skysight validate FILE                                   parse + validate a summary document (exit 7 on violation)
//!   skysight list --root DIR | prior --root DIR --before TS [--limit N] | grep --root DIR NEEDLE
//!   skysight phase2-tag TEXT

use crate::*;
use std::io::Write;

fn arg(a: &[String], flag: &str) -> Option<String> { a.iter().position(|x| x == flag).and_then(|i| a.get(i + 1).cloned()) }
fn fail(msg: &str, code: i32) -> ! { eprintln!("[skysight] {msg}"); std::process::exit(code); }
fn root(a: &[String]) -> Store { Store::open(arg(a, "--root").map(std::path::PathBuf::from).unwrap_or_else(Store::default_root)).unwrap_or_else(|e| fail(&e, 1)) }
fn events(a: &[String]) -> Vec<EventRecord> {
    let p = arg(a, "--events").unwrap_or_else(|| fail("--events FILE required", 2));
    parse_events_jsonl(&std::fs::read_to_string(&p).unwrap_or_else(|e| fail(&e.to_string(), 1))).unwrap_or_else(|e| fail(&e, 1))
}
fn window(a: &[String], cadence: Cadence) -> Window {
    let s = arg(a, "--window").unwrap_or_else(|| fail("--window START required", 2));
    let ts = parse_ts(&s).unwrap_or_else(|| fail(&format!("bad timestamp {s:?}"), 2));
    Window { cadence, start: cadence.window_start(ts) }
}

pub fn run(a: Vec<String>) {
    match a.get(1).map(String::as_str) {
        Some("prompt") => {
            let t = match a.get(2).map(String::as_str) { Some("summarizer") => SUMMARIZER_PROMPT, Some("memory") => MEMORY_INSTRUCTIONS, _ => fail("prompt summarizer|memory", 2) };
            std::io::stdout().write_all(t.as_bytes()).unwrap();
        }
        Some("windows") => {
            let cadence = Cadence::from_token(&arg(&a, "--cadence").unwrap_or_else(|| "10min".into())).unwrap_or_else(|| fail("--cadence 10min|6h", 2));
            let ts: Vec<Ts> = events(&a).iter().filter_map(|e| e.timestamp()).collect();
            let ws: Vec<String> = windows_covering(cadence, ts).iter().map(|w| format_ts(w.start)).collect();
            println!("{}", serde_json::to_string(&ws).unwrap());
        }
        Some("summarize") => {
            let st = root(&a);
            let w = window(&a, Cadence::TenMin);
            let ev = events(&a);
            let prior: Vec<Summary> = st.prior(w.start, 3).iter().filter_map(|n| st.read(n).ok()).collect();
            let s = ReferenceSummarizer.summarize_10min(&TenMinInput { window: w, events: &ev, prior: &prior }).unwrap_or_else(|e| fail(&e, 1));
            let name = st.write(w, &s).unwrap_or_else(|e| fail(&e, 1));
            println!("{}", st.path(&name).display());
        }
        Some("rollup") => {
            let st = root(&a);
            let w = window(&a, Cadence::SixHour);
            let kids: Vec<Summary> = st.children(w).iter().map(|n| st.read(n).unwrap_or_else(|e| fail(&e, 1))).collect();
            let s = ReferenceSummarizer.summarize_6h(&SixHourInput { window: w, children: &kids }).unwrap_or_else(|e| fail(&e, 1));
            let name = st.write(w, &s).unwrap_or_else(|e| fail(&e, 1));
            println!("{}", st.path(&name).display());
        }
        Some("validate") => {
            let p = a.get(2).unwrap_or_else(|| fail("validate FILE", 2));
            let text = std::fs::read_to_string(p).unwrap_or_else(|e| fail(&e.to_string(), 1));
            match Summary::parse(&text) {
                Ok(s) => println!("{}", serde_json::to_string_pretty(&s).unwrap()),
                Err(e) => fail(&e, 7),
            }
        }
        Some("list") => {
            let st = root(&a);
            println!("{}", serde_json::to_string_pretty(&st.list().iter().map(|n| n.file_name()).collect::<Vec<_>>()).unwrap());
        }
        Some("prior") => {
            let st = root(&a);
            let before = parse_ts(&arg(&a, "--before").unwrap_or_else(|| fail("--before TS", 2))).unwrap_or_else(|| fail("bad --before", 2));
            let limit = arg(&a, "--limit").and_then(|l| l.parse().ok()).unwrap_or(3);
            println!("{}", serde_json::to_string_pretty(&st.prior(before, limit).iter().map(|n| n.file_name()).collect::<Vec<_>>()).unwrap());
        }
        Some("grep") => {
            let st = root(&a);
            let needle = a.iter().skip(2).filter(|x| !x.starts_with("--")).find(|x| Some(x.as_str()) != arg(&a, "--root").as_deref()).cloned().unwrap_or_else(|| fail("grep --root DIR NEEDLE", 2));
            println!("{}", serde_json::to_string_pretty(&st.grep(&needle).iter().map(|n| n.file_name()).collect::<Vec<_>>()).unwrap());
        }
        Some("phase2-tag") => println!("{}", phase2_tag(a.get(2).map(String::as_str).unwrap_or(""))),
        _ => fail("usage: skysight <prompt|windows|summarize|rollup|validate|list|prior|grep|phase2-tag> ...", 2),
    }
}
