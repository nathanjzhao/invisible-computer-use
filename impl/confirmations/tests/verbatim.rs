//! The policy texts are the shipped bytes and the machine-readable taxonomy
//! agrees with the text itself (parsed from the golden, an oracle independent
//! of the TAXONOMY table).
use confirmations::*;
use std::collections::BTreeMap;
use std::path::PathBuf;

fn repo_root() -> PathBuf { PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..").canonicalize().unwrap() }

#[test]
fn windows_policy_is_the_golden_and_corpus_bytes() {
    let golden = std::fs::read(repo_root().join("conformance/golden/confirmations-policy.md")).unwrap();
    let corpus = std::fs::read(repo_root().join("codex-corpus/windows/confirmations.md")).unwrap();
    assert_eq!(POLICY_WINDOWS.as_bytes(), &golden[..]);
    assert_eq!(POLICY_WINDOWS.as_bytes(), &corpus[..]);
}

#[test]
fn macos_policy_is_the_guardian_section_verbatim() {
    let g = std::fs::read_to_string(repo_root().join("codex-corpus/mac/ComputerUse/computer-use-node-repl-guardian.md")).unwrap();
    let start = g.find("# Computer Use Confirmations Policy").expect("section heading");
    assert_eq!(POLICY_MACOS, &g[start..]);
    assert!(POLICY_MACOS.starts_with("# Computer Use Confirmations Policy\n"));
}

/// Parse `#### N) Title` headings and `**[tags]**` bullets into tag -> mode number.
fn taxonomy_from_text(text: &str, heading_prefix: &str) -> BTreeMap<String, u8> {
    let mut out = BTreeMap::new();
    let mut current: Option<u8> = None;
    for line in text.lines() {
        if let Some(rest) = line.strip_prefix(heading_prefix) {
            if let Some((n, _)) = rest.split_once(") ") {
                if let Ok(n) = n.parse::<u8>() { current = Some(n); continue; }
            }
        }
        if let (Some(mode), Some(s)) = (current, line.find("**[")) {
            let e = line[s..].find("]**").unwrap() + s;
            let inner = &line[s + 3..e];
            for part in inner.split(',') {
                let part = part.trim();
                if let Some((lo, hi)) = part.split_once('\u{2013}') {
                    // a range like 8.3–8.5: same major, minor lo..=hi
                    let (maj, lo_min) = lo.split_once('.').unwrap();
                    let (_, hi_min) = hi.split_once('.').unwrap();
                    for m in lo_min.parse::<u32>().unwrap()..=hi_min.parse::<u32>().unwrap() {
                        out.insert(format!("{maj}.{m}"), mode);
                    }
                } else {
                    out.insert(part.to_string(), mode);
                }
            }
        }
    }
    out
}

#[test]
fn taxonomy_table_equals_the_tags_parsed_from_both_policy_texts() {
    let from_windows = taxonomy_from_text(POLICY_WINDOWS, "#### ");
    let from_macos = taxonomy_from_text(POLICY_MACOS, "### ");
    assert_eq!(from_windows, from_macos, "the two shipped texts disagree on a category");
    let table: BTreeMap<String, u8> = TAXONOMY.iter().map(|(t, m)| (t.to_string(), m.number())).collect();
    assert_eq!(table, from_windows);
    for (tag, n) in &from_windows {
        assert_eq!(mode_for(tag).number(), *n, "tag {tag}");
    }
}
