//! A memory prior at the moment it matters (spec 03 step 4, measured in study/audit/gaps-memory-4-2026-10-05).
//!
//! The consult-first text lives in the MCP instructions and the tool descriptions, which a model has read and
//! mostly forgotten by the time it opens an app. When the first result for an app the model is about to drive
//! (`list_windows`, `list_apps`, `get_state`, `get_app`, `get_window_state`) arrives and the memory folder has
//! summaries that name that app, the result carries one extra text block: how many entries, their titles, and the
//! exact `memory_grep` that returns them. It is sent once per app per server, and not at all once the model has
//! called a memory tool itself. An app with no entries, an empty or missing folder, and every other tool get
//! nothing, so an irrelevant memory costs no tokens and no nudge.
//!
//! It rides in `structuredContent.memory_prior` (the object a Claude Code model reads; a text block alone was never
//! seen) and in a second text content block for clients that show those. `ICU_MEMORY_HINT=off` sends nothing.
use crate::Layer;
use serde_json::Value;
use std::collections::HashSet;

/// Per-server state: apps already hinted, and whether the model has used memory itself.
#[derive(Default)]
pub struct HintState { pub hinted: HashSet<String>, pub consulted: bool }

fn stem(id: &str) -> String {
    let t = id.trim().trim_end_matches(".app").trim_end_matches(".exe");
    t.rsplit(['.', '/', '\\']).next().unwrap_or(t).to_ascii_lowercase()
}

/// A title is user-derived text and goes into a line the model reads as part of a tool result: one line, no quote or angle
/// bracket that could end the quoting or open a tag, at most 80 characters (gaps-memory-5, the red-team case).
pub(crate) fn title_as_data(title: &str) -> String {
    let s: String = title.chars().filter(|c| !matches!(c, '"' | '<' | '>' | '`')).collect::<String>().split_whitespace().collect::<Vec<_>>().join(" ");
    if s.chars().count() <= 80 { s } else { format!("{}...", s.chars().take(80).collect::<String>()) }
}

/// The app ids a result is about: the window's app for `get_window_state`, the app for `get_app`, and the apps
/// that have an open window for the listings (at most two, in listed order).
fn apps_in(layer: &Layer, tool: &str, args: &Value, result: &Value) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let mut push = |s: Option<&str>| { if let Some(s) = s.filter(|s| !s.is_empty()) { if !out.iter().any(|o| o == s) { out.push(s.to_string()); } } };
    let open = |a: &Value| a.get("windows").and_then(Value::as_array).is_some_and(|w| !w.is_empty());
    match tool {
        "list_windows" => for w in result.as_array().into_iter().flatten() { push(w.get("app").and_then(Value::as_str)); },
        "list_apps" => for a in result.as_array().into_iter().flatten() { if open(a) { push(a.get("id").and_then(Value::as_str)); } },
        "get_state" => for a in result.get("apps").and_then(Value::as_array).into_iter().flatten() { if open(a) { push(a.get("id").and_then(Value::as_str)); } },
        "get_app" => push(result.pointer("/app/id").and_then(Value::as_str)),
        "get_window_state" => {
            if let Ok((pid, _)) = layer.resolve_window(args) {
                if let Ok((bundle, name)) = layer.app_facts(pid) { push(bundle.as_deref().or(name.as_deref())); }
            }
        }
        _ => {}
    }
    out.truncate(2);
    out
}

/// The text block to append to this tool's result, if any.
pub(crate) fn hint(layer: &Layer, state: &mut HintState, tool: &str, args: &Value, result: &Value) -> Option<String> {
    if tool.starts_with("memory_") { state.consulted = true; return None; }
    let mode = std::env::var("ICU_MEMORY_HINT").unwrap_or_default().to_ascii_lowercase();
    if state.consulted || matches!(mode.as_str(), "off" | "0" | "false" | "no") { return None; }
    if !matches!(tool, "list_windows" | "list_apps" | "get_state" | "get_app" | "get_window_state") { return None; }
    let root = &layer.cfg.memory_root;
    if !root.is_dir() { return None; }
    let apps: Vec<String> = apps_in(layer, tool, args, result).into_iter().filter(|a| !state.hinted.contains(a)).collect();
    if apps.is_empty() { return None; }
    let st = skysight::Store::open(root).ok()?;
    let summaries: Vec<skysight::Summary> = st.list().into_iter().filter_map(|n| st.read(&n).ok()).collect();
    let mut lines: Vec<String> = Vec::new();
    for app in &apps {
        let mut hits: Vec<&skysight::Summary> = summaries.iter().filter(|s| s.frontmatter.applications.iter().any(|a| stem(a) == stem(app))).collect();
        if hits.is_empty() { continue; }
        state.hinted.insert(app.clone());
        hits.reverse(); // newest first
        let titles = hits.iter().take(4).map(|s| format!("\"{}\"", title_as_data(&s.frontmatter.title))).collect::<Vec<_>>().join(", ");
        let more = if hits.len() > 4 { format!(" and {} more", hits.len() - 4) } else { String::new() };
        let line = format!("Memory has {} earlier {} about {}: {}{}. If this task leaves a detail open (a name, format, wording, time or recipient), call memory_grep {{\"needle\": \"{}\"}} before you decide it; the goal and the window outrank what it says.",
            hits.len(), if hits.len() == 1 { "entry" } else { "entries" }, app, titles, more, stem(app));
        lines.push(line);
    }
    if lines.is_empty() { None } else { Some(lines.join("\n")) }
}
