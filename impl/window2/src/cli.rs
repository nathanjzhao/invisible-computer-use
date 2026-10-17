//! The crate's command-line verbs as a library entry point, so the `icu` binary can host
//! them (`icu window2 ...`) and the standalone binary stays a thin shim.
//! window2 observation facade CLI over cua-driver (+ the axdiff/app-instructions layer).
//!
//!   window2 get_window_state --pid P --window W [--no-screenshot] [--text] [--state DIR] [--driver PATH]
//!       -> golden WindowState JSON on stdout (window2 defaults: screenshot on, text off)
//!   window2 list_windows                 -> Window[] JSON
//!   window2 get_window --id N            -> Window JSON (exit 4 if not open)
//!   window2 list_apps                    -> ListAppsApp[] JSON
//!   window2 activate_window --id N       -> bring the window to the front and wait until it is on its
//!                                           display's current Space; prints Window + _placement (exit 4 if not open, 6 if still off-Space)
//!   window2 placement --id N             -> _placement diagnostic for an open window
//!   window2 map --gws FILE --tree FILE [--selected FILE] [--app ID] [--no-screenshot] [--text]
//!       -> deterministic: build WindowState from a saved cua document (conformance fixture path)

use axdiff::layer::{bundle_id_for_pid, fetch_gws, observe, LayerConfig};
use serde_json::Value;
use std::path::PathBuf;
use std::process::Command;
use crate::*;

fn call(driver: &str, tool: &str) -> Result<Value, String> {
    let out = Command::new(driver).args(["call", tool, "{}"]).output().map_err(|e| format!("failed to run {driver}: {e}"))?;
    serde_json::from_slice(&out.stdout).map_err(|_| format!("{tool} returned non-JSON"))
}

fn fail(msg: &str, code: i32) -> ! {
    eprintln!("[window2] {msg}");
    std::process::exit(code);
}

pub fn run(a: Vec<String>) {
    let cmd = a.get(1).cloned().unwrap_or_default();
    let mut pid = String::new();
    let mut window = String::new();
    let mut id: Option<u64> = None;
    let mut req = StateRequest::default();
    let mut cfg = LayerConfig::default();
    let mut gws_file: Option<PathBuf> = None;
    let mut tree_file: Option<PathBuf> = None;
    let mut selected_file: Option<PathBuf> = None;
    let mut app_id_override: Option<String> = None;
    let mut lw_file: Option<PathBuf> = None; // `placement --lw FILE`: diagnose from a saved list_windows document
    let mut lineage: Option<String> = None; // `map --lineage KEY`: step one lineage across documents
    let mut i = 2;
    let need = |i: usize, a: &[String]| -> String {
        a.get(i + 1).cloned().unwrap_or_else(|| fail(&format!("{} requires a value", a[i]), 2))
    };
    while i < a.len() {
        match a[i].as_str() {
            "--reset" => { cfg.reset = true; i += 1; }
            "--pid" => { pid = need(i, &a); i += 2; }
            "--window" => { window = need(i, &a); i += 2; }
            "--id" => { id = need(i, &a).parse().ok(); i += 2; }
            "--no-screenshot" => { req.include_screenshot = false; i += 1; }
            "--text" => { req.include_text = true; i += 1; }
            "--state" => { cfg.state_dir = PathBuf::from(need(i, &a)); i += 2; }
            "--driver" => { cfg.driver = need(i, &a); i += 2; }
            "--cumulative" => { cfg.cumulative = true; i += 1; }
            "--disable-diff" => { cfg.opts.disable_diff = true; i += 1; }
            "--gws" => { gws_file = Some(PathBuf::from(need(i, &a))); i += 2; }
            "--tree" => { tree_file = Some(PathBuf::from(need(i, &a))); i += 2; }
            "--selected" => { selected_file = Some(PathBuf::from(need(i, &a))); i += 2; }
            "--app" => { app_id_override = Some(need(i, &a)); i += 2; }
            "--lw" => { lw_file = Some(PathBuf::from(need(i, &a))); i += 2; }
            "--lineage" => { lineage = Some(need(i, &a)); i += 2; }
            other => fail(&format!("unknown argument {other:?}"), 2),
        }
    }

    let emit = |v: &dyn erased::Ser| {
        println!("{}", v.to_json());
    };

    match cmd.as_str() {
        "get_window_state" => {
            if pid.is_empty() || window.is_empty() { fail("usage: window2 get_window_state --pid P --window W [--no-screenshot] [--text]", 2); }
            if !req.include_screenshot && !req.include_text { fail(ERR_BOTH_FALSE, 2); }
            let doc = fetch_gws(&cfg.driver, &pid, &window, req.include_screenshot).unwrap_or_else(|e| fail(&e, 1));
            let bundle_id = bundle_id_for_pid(&cfg.driver, &pid);
            let app_id = bundle_id.clone()
                .or_else(|| doc.get("app_name").and_then(Value::as_str).filter(|s| !s.is_empty()).map(str::to_string))
                .unwrap_or_else(|| pid.clone());
            let tree = if req.include_text {
                let o = observe(&doc, &pid, &window, bundle_id, &cfg);
                if let Some(r) = &o.degraded {
                    // cua refused to ground the tree: surface it, do not fabricate a state —
                    // and say WHY when the window is simply off its display's current Space
                    let hint = call(&cfg.driver, "list_windows").ok()
                        .and_then(|lw| placement_for(&lw, window.parse().unwrap_or(0)))
                        .and_then(|p| p.hint())
                        .map(|h| format!(" | placement: {h}"))
                        .unwrap_or_default();
                    fail(&format!("degraded: {r}{hint}"), 5);
                }
                eprintln!("[window2] pid={pid} window={window} -> {} {} key={} rev={}/{}", o.tag, o.instr_tag, o.key, o.revision.0, o.revision.1);
                Some(TreeInput::from(&o))
            } else { None };
            let st = window_state_from_cua(&doc, &app_id, req, tree.as_ref()).unwrap_or_else(|e| fail(&e, 1));
            emit(&st);
        }
        "activate_window" | "placement" => {
            let id = id.unwrap_or_else(|| fail(&format!("usage: window2 {cmd} --id N"), 2));
            let (la, lw) = match (&lw_file, cmd.as_str()) {
                (Some(f), "placement") => {
                    let lw: Value = serde_json::from_slice(&std::fs::read(f).unwrap_or_else(|e| fail(&e.to_string(), 1))).unwrap_or_else(|e| fail(&e.to_string(), 1));
                    (serde_json::json!({"apps": []}), lw)
                }
                _ => (call(&cfg.driver, "list_apps").unwrap_or_else(|e| fail(&e, 1)), call(&cfg.driver, "list_windows").unwrap_or_else(|e| fail(&e, 1))),
            };
            let resolve = app_id_resolver(&la);
            let ws = windows_from_cua(&lw, &resolve);
            let Some((pid, win)) = ws.iter().find(|(_, w)| w.id == id).cloned() else { fail(&format!("window {id} is not open"), 4) };
            let mut placement = placement_for(&lw, id);
            if cmd == "activate_window" {
                // window2 `activate_window`: the escape hatch that brings an open window to the
                // foreground; on macOS that also switches its display to the window's Space
                let req = format!("{{\"pid\":{pid},\"window_id\":{id}}}");
                let _ = Command::new(&cfg.driver).args(["call", "bring_to_front", &req]).output();
                for _ in 0..8 {
                    std::thread::sleep(std::time::Duration::from_millis(400));
                    placement = call(&cfg.driver, "list_windows").ok().and_then(|l| placement_for(&l, id));
                    if placement.as_ref().is_some_and(Placement::observable) { break; }
                }
            }
            let out = serde_json::json!({ "app": win.app, "id": win.id, "title": win.title, "_placement": placement,
                                          "_hint": placement.as_ref().and_then(Placement::hint) });
            println!("{}", serde_json::to_string_pretty(&out).unwrap());
            if cmd == "activate_window" && !placement.as_ref().is_some_and(Placement::observable) {
                fail("window is still not on its display's current Space after activate_window", 6);
            }
        }
        "list_windows" | "get_window" | "list_apps" => {
            let la = call(&cfg.driver, "list_apps").unwrap_or_else(|e| fail(&e, 1));
            let lw = call(&cfg.driver, "list_windows").unwrap_or_else(|e| fail(&e, 1));
            let resolve = app_id_resolver(&la);
            let ws = windows_from_cua(&lw, &resolve);
            match cmd.as_str() {
                "list_windows" => emit(&ws.iter().map(|(_, w)| w.clone()).collect::<Vec<_>>()),
                "get_window" => {
                    let id = id.unwrap_or_else(|| fail("usage: window2 get_window --id N", 2));
                    match get_window(&ws, id) {
                        Some(w) => emit(&w),
                        None => fail(&format!("window {id} is not open"), 4),
                    }
                }
                _ => emit(&apps_from_cua(&la, &ws)),
            }
        }
        "map" => {
            if !req.include_screenshot && !req.include_text { fail(ERR_BOTH_FALSE, 2); }
            let gws_file = gws_file.unwrap_or_else(|| fail("usage: window2 map --gws FILE [--tree FILE] [--selected FILE] [--app ID] [--lineage KEY] [--disable-diff] [--no-screenshot] [--text]", 2));
            let doc: Value = serde_json::from_slice(&std::fs::read(&gws_file).unwrap_or_else(|e| fail(&e.to_string(), 1)))
                .unwrap_or_else(|e| fail(&e.to_string(), 1));
            let tree = if req.include_text {
                match tree_file {
                    Some(t) => {
                        let text = std::fs::read_to_string(&t).unwrap_or_else(|e| fail(&e.to_string(), 1));
                        let selected: Vec<String> = selected_file.map(|p| std::fs::read_to_string(p).unwrap_or_else(|e| fail(&e.to_string(), 1)))
                            .map(|s| s.lines().map(str::to_string).collect()).unwrap_or_default();
                        Some(TreeInput { tree: text, selected_elements: selected, revision: None, focused_element: None })
                    }
                    None => {
                        // no pre-rendered tree: step the layer on the saved document (a fresh
                        // lineage per file unless --lineage names one to continue)
                        let key = lineage.clone().unwrap_or_else(|| gws_file.display().to_string().replace('/', "_"));
                        let o = observe(&doc, "fixture", &key, app_id_override.clone(), &cfg);
                        Some(TreeInput::from(&o))
                    }
                }
            } else { None };
            let app_id = app_id_override
                .or_else(|| doc.get("app_name").and_then(Value::as_str).filter(|s| !s.trim().is_empty()).map(str::to_string))
                .unwrap_or_else(|| fail("no app id: pass --app or a document with a non-empty app_name", 2));
            let st = window_state_from_cua(&doc, &app_id, req, tree.as_ref()).unwrap_or_else(|e| fail(&e, 1));
            emit(&st);
        }
        _ => fail("usage: window2 <get_window_state|list_windows|get_window|list_apps|map> ...", 2),
    }
}

mod erased {
    pub trait Ser { fn to_json(&self) -> String; }
    impl<T: serde::Serialize> Ser for T {
        fn to_json(&self) -> String { serde_json::to_string_pretty(self).expect("serializable") }
    }
}
