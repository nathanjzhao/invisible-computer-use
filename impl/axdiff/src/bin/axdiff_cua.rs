//! Drive axdiff over two real cua-driver get_window_state snapshots, rendered the way
//! Sky renders them (`axdiff::sky`).
//! usage: axdiff-cua <turn1.json> <turn2.json> [--app NAME] [--cumulative] [--budget N] [--disable-diff]
use axdiff::sky::{self, SkyOptions};
use axdiff::*;
use std::fs;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 3 {
        eprintln!("usage: axdiff-cua <turn1.json> <turn2.json> [--app NAME] [--cumulative] [--budget N] [--disable-diff]");
        std::process::exit(2);
    }
    let d1: serde_json::Value = serde_json::from_str(&fs::read_to_string(&args[1]).unwrap()).unwrap();
    let d2: serde_json::Value = serde_json::from_str(&fs::read_to_string(&args[2]).unwrap()).unwrap();
    let mut opts = SkyOptions { app_name: d2.get("app_name").and_then(|v| v.as_str()).unwrap_or("").into(), ..SkyOptions::default() };
    let mut cumulative = false;
    let mut i = 3;
    while i < args.len() {
        match args[i].as_str() {
            "--app" => { opts.app_name = args[i + 1].clone(); i += 2; }
            "--cumulative" => { cumulative = true; i += 1; }
            "--disable-diff" => { opts.disable_diff = true; i += 1; }
            "--budget" => { opts.line_budget = args[i + 1].parse().unwrap(); i += 2; }
            _ => { i += 1; }
        }
    }
    let f1 = sky::forest_from_gws(&d1);
    let f2 = sky::forest_from_gws(&d2);
    let r1 = Revision::initial_from(&f1, 0);
    let r2 = r1.next(&f2);
    opts.window_title = sky::window_title(&d2, &r2);
    let out = if cumulative { sky::render_cumulative(&r2, &r1, &opts) } else { sky::render_diff(&r2, true, &opts) };
    let tag = match &out {
        DiffOutput::Full(_) => "FULL",
        DiffOutput::Diff(_) => "DIFF",
        DiffOutput::NoChange(_) => "NO_CHANGE",
        DiffOutput::FullFallback(_) => "FULL_FALLBACK(budget exceeded)",
    };
    eprintln!("[axdiff-cua] turn1 nodes(top)={} turn2 nodes(top)={} -> {}", f1.len(), f2.len(), tag);
    println!("{}", out.text());
}
