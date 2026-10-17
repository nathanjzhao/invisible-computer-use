//! Live Clock across two minute boundaries (study/audit/mac-review3-fixes-2026-09-26, B3).
//! The same four reads were taken with the round-3 driver (red: no container identifier)
//! and the fixed one (green), with no input between them. Each minute SwiftUI re-creates
//! the city row with the new time. Sky keeps a row's id while it stays (golden I2).
use axdiff::layer::{observe, LayerConfig};
use serde_json::Value;
use std::path::Path;

const READS: [&str; 4] = ["first_touch", "second_read", "minute_1", "minute_2"];

fn texts(variant: &str, test: &str) -> Vec<String> {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/clock-live");
    let state = std::env::temp_dir().join(format!("clock-live-{}-{variant}-{test}", std::process::id()));
    let _ = std::fs::remove_dir_all(&state);
    let out = READS.iter().map(|read| {
        let doc: Value = serde_json::from_slice(&std::fs::read(dir.join(format!("{variant}-{read}.json"))).unwrap()).unwrap();
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

fn removed(text: &str) -> Option<&str> {
    text.lines().find_map(|l| l.strip_prefix("Removed element IDs: "))
}

#[test]
fn the_remove_clock_rows_keep_their_ids_across_minute_boundaries() {
    let t = texts("green", "ids");
    let first = rows(&t[0], "Remove clock");
    assert_eq!(first.len(), 2, "two cities:\n{}", t[0]);
    assert!(t[1].starts_with("There has been no change"), "{}", t[1]);
    for minute in &t[2..] {
        let now = rows(minute, "Remove clock");
        assert_eq!(now.iter().map(|(_, id)| *id).collect::<Vec<_>>(), first.iter().map(|(_, id)| *id).collect::<Vec<_>>(), "{minute}");
        assert!(now.iter().all(|(m, _)| *m == Some('~')), "the new time is a change, not a new row:\n{minute}");
        assert!(!removed(minute).unwrap_or("").split(['-', ',', ' ']).filter_map(|n| n.parse::<u64>().ok())
            .any(|id| first.iter().any(|(_, f)| *f == id)), "{minute}");
    }
}

/// Known limit, pinned: the world map's points sit under a heading that has no
/// identifier, so a point whose time changed is re-minted each minute. Their position
/// alone cannot tell a changed time from a removed city and an added one.
#[test]
fn the_world_map_points_are_re_minted_each_minute() {
    let t = texts("green", "map");
    for minute in &t[2..] {
        let points = rows(minute, "element Description: ");
        assert_eq!(points.len(), 2, "{minute}");
        assert!(points.iter().all(|(m, _)| *m == Some('+')), "{minute}");
        assert!(removed(minute).is_some(), "{minute}");
    }
}

/// The same reads from the round-3 driver: no container identifier, so every row whose
/// text changed is removed and re-added under a new id (review3/mac B3).
#[test]
fn without_the_container_identifier_the_rows_are_re_minted() {
    let t = texts("red", "red");
    for minute in &t[2..] {
        let now = rows(minute, "Remove clock");
        assert_eq!(now.len(), 2, "{minute}");
        assert!(now.iter().all(|(m, _)| *m == Some('+')), "{minute}");
    }
}
