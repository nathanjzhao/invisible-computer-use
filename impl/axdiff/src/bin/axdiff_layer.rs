//! Stateful AX-diff + per-app-pack layer over cua-driver (CLI over `axdiff::layer`).
//!
//! Wraps cua-driver's `get_window_state`: keeps a per-(pid,window) revision
//! store on disk so that the FIRST observation of a window returns the full
//! tree and each SUBSEQUENT observation returns a diff, with stable element ids
//! carried across turns — the Codex/Sky behaviour, layered on top of cua-driver
//! without forking its internals. The per-app instruction pack (spec 04) is
//! prefixed on the app's first successful observation in the session (= state dir).
//!
//! usage:
//!   axdiff-layer --pid P --window W [--cumulative] [--disable-diff] [--reset]
//!                [--state DIR] [--driver PATH] [--app NAME] [--budget N]
use axdiff::layer::{bundle_id_for_pid, fetch_gws, observe, LayerConfig};
use std::path::PathBuf;

fn main() {
    let mut pid = String::new();
    let mut window = String::new();
    let mut cfg = LayerConfig::default();

    let a: Vec<String> = std::env::args().collect();
    let mut i = 1;
    while i < a.len() {
        match a[i].as_str() {
            "--pid" => { pid = a[i + 1].clone(); i += 2; }
            "--window" => { window = a[i + 1].clone(); i += 2; }
            "--cumulative" => { cfg.cumulative = true; i += 1; }
            "--disable-diff" => { cfg.opts.disable_diff = true; i += 1; }
            "--reset" => { cfg.reset = true; i += 1; }
            "--budget" => { cfg.opts.line_budget = a[i + 1].parse().unwrap(); i += 2; }
            "--app" => { cfg.opts.app_name = a[i + 1].clone(); i += 2; }
            "--state" => { cfg.state_dir = PathBuf::from(&a[i + 1]); i += 2; }
            "--driver" => { cfg.driver = a[i + 1].clone(); i += 2; }
            _ => { i += 1; }
        }
    }
    if pid.is_empty() || window.is_empty() {
        eprintln!("usage: axdiff-layer --pid P --window W [--cumulative] [--disable-diff] [--reset] [--state DIR] [--driver PATH] [--app NAME]");
        std::process::exit(2);
    }

    let doc = match fetch_gws(&cfg.driver, &pid, &window, false) {
        Ok(d) => d,
        Err(e) => { eprintln!("[axdiff-layer] {e}"); std::process::exit(1); }
    };
    let bundle_id = bundle_id_for_pid(&cfg.driver, &pid);
    let o = observe(&doc, &pid, &window, bundle_id, &cfg);
    if let Some(reason) = &o.degraded {
        // honour cua's own degraded/empty contract rather than diffing a mis-grounded tree
        eprintln!("[axdiff-layer] DEGRADED (no diff): {reason}");
        std::process::exit(0);
    }
    eprintln!("[axdiff-layer] pid={pid} window={window} -> {} {} key={} rev={}/{} (state: {})",
              o.tag, o.instr_tag, o.key, o.revision.0, o.revision.1, cfg.state_dir.display());
    println!("{}", o.text);
}
