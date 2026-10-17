//! Live Clock Stopwatch, running with one lap (study/audit/mac-review4-fixes-2026-09-26, R4-4).
//! Three driver documents about 3 s apart, no input between them. The running time row keeps
//! its id because it carries an identifier (`stopwatch-time`). The current lap row's text
//! changes on every read and it has no identifier and no identified container, so SwiftUI's
//! re-created row is a new row to Sky's identity rule: a second known limit next to the
//! world map's points (clock_live.rs).
use axdiff::layer::{observe, LayerConfig};
use serde_json::Value;
use std::path::Path;

fn texts(test: &str) -> Vec<String> {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/stopwatch-live");
    let state = std::env::temp_dir().join(format!("stopwatch-live-{}-{test}", std::process::id()));
    let _ = std::fs::remove_dir_all(&state);
    let out = (1..=3).map(|n| {
        let doc: Value = serde_json::from_slice(&std::fs::read(dir.join(format!("green-read_{n}.json"))).unwrap()).unwrap();
        let cfg = LayerConfig { state_dir: state.clone(), ..LayerConfig::default() };
        observe(&doc, "clock", "clock", Some("com.apple.clock".into()), &cfg).text
    }).collect();
    let _ = std::fs::remove_dir_all(&state);
    out
}

/// `(marker, id)` of every row whose text contains `needle`.
fn rows(text: &str, needle: &str) -> Vec<(Option<char>, u64)> {
    text.lines().filter(|l| l.contains(needle)).filter_map(|l| {
        let marker = l.chars().next().filter(|c| *c == '~' || *c == '+');
        let id = l.trim_start_matches(['~', '+']).trim_start_matches('\t').split(' ').next()?.parse().ok()?;
        Some((marker, id))
    }).collect()
}

fn removed(text: &str) -> Vec<u64> {
    text.lines().find_map(|l| l.strip_prefix("Removed element IDs: "))
        .map(|ids| ids.split([',', ' ']).filter_map(|n| n.trim().parse().ok()).collect()).unwrap_or_default()
}

#[test]
fn the_running_time_keeps_its_id_through_its_identifier() {
    let t = texts("time");
    let first = rows(&t[0], "ID: stopwatch-time");
    assert_eq!(first.len(), 1, "{}", t[0]);
    for read in &t[1..] {
        assert_eq!(rows(read, "ID: stopwatch-time"), vec![(Some('~'), first[0].1)], "{read}");
    }
}

/// Known limit, pinned: the current lap row is re-minted on every read.
#[test]
fn the_current_lap_row_is_re_minted_on_every_read() {
    let t = texts("lap");
    let mut previous = rows(&t[0], "Lap 1, ");
    assert_eq!(previous.len(), 1, "{}", t[0]);
    for read in &t[1..] {
        let now = rows(read, "Lap 1, ");
        assert_eq!(now.len(), 1, "{read}");
        assert_eq!(now[0].0, Some('+'), "a new row, not a change:\n{read}");
        assert_ne!(now[0].1, previous[0].1, "{read}");
        assert!(removed(read).contains(&previous[0].1), "the old id is removed:\n{read}");
        previous = now;
    }
}
