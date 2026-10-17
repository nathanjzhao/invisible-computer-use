//! Every public function that takes free text must return, never panic, on any string. The
//! app is built with `panic = "abort"`, so one bad slice in a redactor kills Copilot (seen on
//! macOS 26: the model's summary "Computed 6×7=42" aborted the app). The strings mix the
//! characters these parsers split on with multi-byte ones placed right next to them.
use std::collections::BTreeSet;
use std::panic::{catch_unwind, AssertUnwindSafe};

const PIECES: &[&str] = &[
    "=", "/", ":", ".", "-", "_", "\"", "'", ";", "&", ",", "`", "$", " ", "\\", "@", "~", "(", ")", "#", "?", "%",
    "\n", "\t", "a", "Z", "7", "0", "×", "é", "日本", "😀", "\u{200B}", "\u{0301}", "ß", "Ω", "‐", "：", "＝", "／",
    "password", "token", "key", "secret", "https://", "http://", "~/", "/Users/", "com.", "app", "<<", "EOF", "--",
    "user:pw@", "AKIA", "ghp_", "sk-", "Bearer ", "export ", "PASS", "=\"", "='", "$(", "${", "}",
];

fn strings(n: usize) -> Vec<String> {
    let mut x: u64 = 0x9E37_79B9_7F4A_7C15;
    let mut next = move || { x ^= x << 13; x ^= x >> 7; x ^= x << 17; x };
    (0..n).map(|_| {
        let len = (next() % 12) as usize + 1;
        (0..len).map(|_| PIECES[(next() % PIECES.len() as u64) as usize]).collect()
    }).collect()
}

#[test]
fn text_functions_never_panic_on_mixed_script_input() {
    let known: BTreeSet<String> = ["/Users/x/a.txt".to_string()].into();
    let mut fixed = vec!["6×7=42".to_string(), "×=1".into(), "a×b=c".into(), "é=é".into(), "日本=x y".into(), "😀password=😀".into()];
    fixed.extend(strings(60_000));
    let mut failures = Vec::new();
    for s in &fixed {
        let r = catch_unwind(AssertUnwindSafe(|| {
            let _ = skysight::redact_command(s);
            let _ = skysight::redact_path(s);
            let _ = skysight::link_or_url(s);
            let _ = skysight::directive_violation(s);
            let _ = skysight::bundle_ids_in(s);
            let _ = skysight::paths_in(s);
            let _ = skysight::ungrounded_path(s, &known);
            let _ = skysight::slugify(s);
            let _ = skysight::parse_ts(s);
            let _ = skysight::parse_events_jsonl(s);
        }));
        if r.is_err() { failures.push(s.clone()); }
        if failures.len() >= 5 { break; }
    }
    assert!(failures.is_empty(), "panicked on {failures:?}");
}
