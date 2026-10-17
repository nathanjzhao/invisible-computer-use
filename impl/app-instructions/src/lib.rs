//! Per-app instruction packs — the Codex/Sky parity slice for `specs/04-per-app-instructions.md`.
//!
//! Sky ships one Markdown playbook per app and prefixes it onto the accessibility
//! text the FIRST time an agent observes that app in a session, wrapped as
//!
//! ```text
//! <app_specific_instructions>
//! {pack}
//! </app_specific_instructions>
//! {accessibility text}
//! ```
//!
//! Grounding (all verbatim from the shipped artefacts; see the spec for paths):
//! - The seven packs under `packs/` are byte-identical to the corpus copies and to
//!   the shipped bundle (`reference/openai-sky/app-bundle-packs/SHA256SUMS.txt`);
//!   `tests/verbatim.rs` re-checks this against the corpus and the golden dir.
//! - `<app_specific_instructions>` / `</app_specific_instructions>` are the exact
//!   strings in `SkyComputerUseService` (and in `sky_js/.../window_result.js`).
//! - The wrap/dedup algorithm in [`window_result_text`] is a line-for-line port of
//!   `window_result.js`'s `r()` (bundle-id keyed `Set`, app-name fallback, the
//!   module-level `new Set(["com.apple.iWork.Numbers"])` exclusion).
//! - The window2/MCP path dedups natively: `ComputerUseMCPServer` carries an
//!   `AppInstructionDeliveryState.bundleIdentifiersWithDeliveredInstructions`
//!   (binary strings). [`DeliveryState::deliver`] models that path.
//!
//! INFERRED (not recoverable from the binaries) and labelled as such below:
//! - Bundle ids for Apple Music, Clock and Notion (the binary only carries Slack,
//!   Spotify, iPhone Mirroring literally; Numbers comes from the JS exclusion).
//!   Music/Clock/Numbers/Slack/Spotify/iPhone Mirroring were verified against the
//!   installed apps on 2026-09-10 (`osascript -e 'id of app ...'`); Notion is the
//!   standard `notion.id` and was NOT verified locally (not installed).
//! - Whether the native `appSpecificInstructions` string carries the file's
//!   trailing newline. We pass the file bytes verbatim (each ends in `\n`).
//! - Why Numbers is excluded from the JS wrapper (the spec's reading: the native
//!   service handles it inline). The window2 path here delivers Numbers once like
//!   every other pack, because no exclusion string exists on the native MCP side.
//!
//! Deliberately NOT built (spec 04 step 5, "optional… not required for baseline
//! parity"): the dynamic Spotify link-rewriting supplement.

pub mod cli;

use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

/// Exact opening tag (binary string, `window_result.js`).
pub const OPEN_TAG: &str = "<app_specific_instructions>";
/// Exact closing tag (binary string, `window_result.js`).
pub const CLOSE_TAG: &str = "</app_specific_instructions>";

/// `window_result.js`: `const e = new Set(["com.apple.iWork.Numbers"])` — apps the
/// JS wrapper never wraps (returns the plain text without recording a touch).
pub const JS_WRAPPER_EXCLUDED_BUNDLE_IDS: &[&str] = &["com.apple.iWork.Numbers"];

/// One shipped playbook.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Pack {
    /// Bundle identifier the pack is keyed by.
    pub bundle_id: &'static str,
    /// File name inside the shipped `AppInstructions/` directory.
    pub file: &'static str,
    /// The pack body, byte-for-byte the shipped file.
    pub body: &'static str,
    /// `true` when the bundle id is a literal in the shipped binary/JS;
    /// `false` when INFERRED (standard id, verified only against a local install
    /// where noted in the module docs).
    pub bundle_id_confirmed: bool,
}

/// The seven shipped packs (spec 04, "the content lives in seven Markdown files").
pub const PACKS: &[Pack] = &[
    Pack { bundle_id: "com.tinyspeck.slackmacgap", file: "Slack.md",
           body: include_str!("../packs/Slack.md"), bundle_id_confirmed: true },
    Pack { bundle_id: "com.spotify.client", file: "Spotify.md",
           body: include_str!("../packs/Spotify.md"), bundle_id_confirmed: true },
    Pack { bundle_id: "com.apple.ScreenContinuity", file: "iPhone Mirroring.md",
           body: include_str!("../packs/iPhone Mirroring.md"), bundle_id_confirmed: true },
    Pack { bundle_id: "com.apple.iWork.Numbers", file: "Numbers.md",
           body: include_str!("../packs/Numbers.md"), bundle_id_confirmed: true },
    // INFERRED bundle ids (standard; Music + Clock verified on the local install).
    Pack { bundle_id: "com.apple.Music", file: "AppleMusic.md",
           body: include_str!("../packs/AppleMusic.md"), bundle_id_confirmed: false },
    Pack { bundle_id: "com.apple.clock", file: "Clock.md",
           body: include_str!("../packs/Clock.md"), bundle_id_confirmed: false },
    Pack { bundle_id: "notion.id", file: "Notion.md",
           body: include_str!("../packs/Notion.md"), bundle_id_confirmed: false },
];

/// Resolve a bundle id to its pack. Exact (case-sensitive) match, like the JS `Set`.
/// The dynamic supplemental note Sky appends for Spotify (spec 04 step 5), verbatim from the
/// `SkyComputerUseService` binary (`strings`, 2026-09-12). It is NOT in the .md pack: Sky adds
/// it at runtime. Unlike a pack (prefixed, wrapped in the tag), the supplement is appended AFTER
/// the tree — the note refers to "the UI above", so it must follow the rendered tree. INFERRED:
/// placement after the tree and first-touch-per-session, matching the pack's delivery.
pub const SPOTIFY_SUPPLEMENT: &str = "Note: In order to be usable, Spotify app links must be rewritten as regular links (e.g. use open.spotify.com instead of xpui.app.spotify.com). Only use Spotify links that are written verbatim in the UI above. Note that IDs are only valid with their associated type (e.g. you cannot change an \"album\" URL to a \"track\" URL).";

/// The runtime supplement for a bundle id, appended after the tree on first touch (spec 04 step 5).
/// Only Spotify has one; every other app returns None.
pub fn supplement_for_bundle_id(bundle_id: &str) -> Option<&'static str> {
    match bundle_id { "com.spotify.client" => Some(SPOTIFY_SUPPLEMENT), _ => None }
}

pub fn pack_for_bundle_id(bundle_id: &str) -> Option<&'static Pack> {
    PACKS.iter().find(|p| p.bundle_id == bundle_id)
}

/// The wrap from `window_result.js`:
/// `` `<app_specific_instructions>\n${u}\n</app_specific_instructions>\n${s}` ``.
pub fn wrap(instructions: &str, text: &str) -> String {
    format!("{OPEN_TAG}\n{instructions}\n{CLOSE_TAG}\n{text}")
}

/// Per-session record of which apps already received their instructions.
///
/// Mirrors `create_client.js`'s per-client `const u = new Set` (JS path) and the
/// native `bundleIdentifiersWithDeliveredInstructions` (window2/MCP path). Keys
/// are whatever [`window_result_text`] / [`DeliveryState::deliver`] were given:
/// a bundle id when known, otherwise the app-name fallback.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct DeliveryState {
    delivered: BTreeSet<String>,
}

impl DeliveryState {
    pub fn new() -> Self { Self::default() }

    /// Has this key already been delivered in the session?
    pub fn has(&self, key: &str) -> bool { self.delivered.contains(key) }

    /// Keys delivered so far (sorted).
    pub fn keys(&self) -> impl Iterator<Item = &str> { self.delivered.iter().map(String::as_str) }

    /// window2 / native-MCP path: prefix `instructions` onto `text` if `key` has
    /// not been delivered yet this session, and record it. No instructions (or an
    /// empty string) leaves the text untouched and records nothing, exactly like
    /// the JS early-return before `o.add(p)`.
    pub fn deliver(&mut self, key: &str, instructions: Option<&str>, text: &str) -> String {
        self.deliver_with_supplement(key, instructions, None, text)
    }

    /// As [`deliver`], plus a runtime `supplement` (spec 04 step 5) appended AFTER the tree on
    /// its own first touch (key `"<key>#supplement"`), so the pack leads and the note that refers
    /// to "the UI above" follows the rendered tree. A supplement with no pack still delivers once.
    pub fn deliver_with_supplement(&mut self, key: &str, instructions: Option<&str>, supplement: Option<&str>, text: &str) -> String {
        let mut out = match instructions {
            Some(u) if !u.is_empty() && !self.delivered.contains(key) => { self.delivered.insert(key.to_string()); wrap(u, text) }
            _ => text.to_string(),
        };
        if let Some(sup) = supplement.filter(|s| !s.is_empty()) {
            let skey = format!("{key}#supplement");
            if !self.delivered.contains(&skey) { self.delivered.insert(skey); out = format!("{out}\n{sup}"); }
        }
        out
    }

    /// Serialize for an on-disk session store (a JSON array of keys).
    pub fn to_json(&self) -> String {
        serde_json::to_string(&self.delivered.iter().collect::<Vec<_>>()).expect("BTreeSet<String> serializes")
    }

    /// Restore from [`DeliveryState::to_json`]. Malformed input is treated as an
    /// empty session (a fresh session is the safe failure: at worst the pack is
    /// shown a second time).
    pub fn from_json(s: &str) -> Self {
        let keys: Vec<String> = serde_json::from_str(s).unwrap_or_default();
        DeliveryState { delivered: keys.into_iter().collect() }
    }
}

/// Line-for-line port of `window_result.js` `r(n, t, r, o)` for the v1 `window`
/// API (`get_app_state` -> `window_result(app, skyshot, seenSet)`):
///
/// - `app_arg` is the `n` the caller passed (the app the state was requested for);
/// - `app_bundle_id` is `t.app.bundleIdentifier` when `t.app` is an object;
/// - `app_name_field` is `t.app` when it is a string;
/// - `app_specific_instructions` is `t.appSpecificInstructions`;
/// - `text` is the skyshot's accessibility text `s`.
///
/// Behaviour: no/empty instructions -> plain text; a bundle id in
/// [`JS_WRAPPER_EXCLUDED_BUNDLE_IDS`] -> plain text (nothing recorded); the dedup
/// key `p` is the bundle id, else the non-empty app-name field, else `app_arg`;
/// already seen -> plain text; else record and wrap.
pub fn window_result_text(
    app_arg: &str,
    app_bundle_id: Option<&str>,
    app_name_field: Option<&str>,
    app_specific_instructions: Option<&str>,
    text: &str,
    seen: &mut DeliveryState,
) -> String {
    let u = match app_specific_instructions {
        Some(u) if !u.is_empty() => u,
        _ => return text.to_string(),
    };
    // `i(t)`: only a non-empty bundleIdentifier counts.
    let c = app_bundle_id.filter(|b| !b.is_empty());
    if let Some(c) = c {
        if JS_WRAPPER_EXCLUDED_BUNDLE_IDS.contains(&c) {
            return text.to_string();
        }
    }
    let p = c
        .or(app_name_field.filter(|a| !a.is_empty()))
        .unwrap_or(app_arg);
    seen.deliver(p, Some(u), text)
}

#[cfg(test)]
mod tests {
    use super::*;

    const ALL_IDS: [&str; 7] = [
        "com.tinyspeck.slackmacgap", "com.spotify.client", "com.apple.ScreenContinuity",
        "com.apple.iWork.Numbers", "com.apple.Music", "com.apple.clock", "notion.id",
    ];

    #[test]
    fn map_resolves_each_shipped_pack_exactly_once() {
        assert_eq!(PACKS.len(), 7);
        let files: BTreeSet<&str> = PACKS.iter().map(|p| p.file).collect();
        assert_eq!(files.len(), 7, "duplicate file in map");
        for id in ALL_IDS {
            let p = pack_for_bundle_id(id).unwrap_or_else(|| panic!("no pack for {id}"));
            assert_eq!(p.bundle_id, id);
            assert!(!p.body.is_empty());
        }
        // the three literal-in-binary ids + Numbers (JS exclusion) are confirmed
        for id in ["com.tinyspeck.slackmacgap", "com.spotify.client", "com.apple.ScreenContinuity", "com.apple.iWork.Numbers"] {
            assert!(pack_for_bundle_id(id).unwrap().bundle_id_confirmed, "{id} should be confirmed");
        }
    }

    #[test]
    fn map_is_exact_match_only() {
        assert!(pack_for_bundle_id("").is_none());
        assert!(pack_for_bundle_id("com.apple.ActivityMonitor").is_none());
        assert!(pack_for_bundle_id("COM.SPOTIFY.CLIENT").is_none(), "JS Set lookup is case-sensitive");
        assert!(pack_for_bundle_id("com.spotify.client ").is_none());
        assert!(pack_for_bundle_id("com.spotify").is_none(), "no prefix matching");
    }

    #[test]
    fn wrap_is_the_exact_js_template() {
        let out = wrap("BODY", "TEXT");
        assert_eq!(out, "<app_specific_instructions>\nBODY\n</app_specific_instructions>\nTEXT");
        // the pack file bytes are passed verbatim, trailing newline included
        let spot = pack_for_bundle_id("com.spotify.client").unwrap().body;
        assert!(spot.ends_with('\n'));
        let out = wrap(spot, "T");
        assert!(out.starts_with("<app_specific_instructions>\n## Spotify Computer Use"));
        assert!(out.ends_with("\n\n</app_specific_instructions>\nT"));
    }

    #[test]
    fn deliver_wraps_first_touch_only_per_key() {
        let mut s = DeliveryState::new();
        let a = s.deliver("com.spotify.client", Some("P"), "t1");
        assert_eq!(a, wrap("P", "t1"));
        let b = s.deliver("com.spotify.client", Some("P"), "t2");
        assert_eq!(b, "t2", "second touch of the same app must be plain");
        let c = s.deliver("com.apple.clock", Some("Q"), "t3");
        assert_eq!(c, wrap("Q", "t3"), "a different app still gets its first touch");
        assert!(s.has("com.spotify.client") && s.has("com.apple.clock"));
        assert_eq!(s.keys().collect::<Vec<_>>(), vec!["com.apple.clock", "com.spotify.client"]);
    }

    #[test]
    fn deliver_without_instructions_records_nothing() {
        let mut s = DeliveryState::new();
        assert_eq!(s.deliver("k", None, "t"), "t");
        assert_eq!(s.deliver("k", Some(""), "t"), "t");
        assert!(!s.has("k"), "JS returns before o.add(p) when u is null/empty");
        // so the first REAL delivery still wraps
        assert_eq!(s.deliver("k", Some("P"), "t"), wrap("P", "t"));
    }

    #[test]
    fn deliver_wraps_exactly_once_per_distinct_key_for_any_sequence() {
        // deterministic pseudo-random sequences over a key alphabet; the number of
        // wrapped outputs must equal the number of distinct keys, and each wrapped
        // output must be the first occurrence of its key.
        let alphabet = ["a", "b", "c", "d", "e", "f", "g", "h"];
        let mut x: u64 = 0x9E37_79B9_7F4A_7C15;
        for _round in 0..50 {
            let mut s = DeliveryState::new();
            let mut first_seen = BTreeSet::new();
            let len = (x % 40) as usize + 1;
            for _ in 0..len {
                x ^= x << 13; x ^= x >> 7; x ^= x << 17;
                let k = alphabet[(x % alphabet.len() as u64) as usize];
                let out = s.deliver(k, Some("I"), "T");
                let wrapped = out.contains(OPEN_TAG);
                assert_eq!(wrapped, first_seen.insert(k), "key {k}: wrapped iff first occurrence");
                assert_eq!(out.matches(OPEN_TAG).count(), if wrapped { 1 } else { 0 });
            }
            assert_eq!(s.keys().count(), first_seen.len());
        }
    }

    #[test]
    fn js_port_excludes_numbers_without_recording() {
        let mut seen = DeliveryState::new();
        let out = window_result_text("Numbers", Some("com.apple.iWork.Numbers"), None, Some("NUM"), "T", &mut seen);
        assert_eq!(out, "T", "Numbers is in the JS wrapper exclusion Set");
        assert!(!seen.has("com.apple.iWork.Numbers"), "excluded apps are not added to the seen Set");
        // every other pack app wraps on first touch
        for id in ALL_IDS.iter().filter(|i| **i != "com.apple.iWork.Numbers") {
            let out = window_result_text("x", Some(id), None, Some("I"), "T", &mut seen);
            assert_eq!(out, wrap("I", "T"), "{id}");
        }
    }

    #[test]
    fn js_port_key_fallback_chain() {
        // In the JS `t.app` is ONE value: an object (with bundleIdentifier) OR a
        // string. The reachable cases are therefore: object with non-empty
        // bundleIdentifier -> that id; object with empty bundleIdentifier -> the
        // caller's arg `n`; string app (non-empty) -> the string; empty string -> `n`.
        let mut seen = DeliveryState::new();
        window_result_text("Arg", Some("bid"), None, Some("I"), "T", &mut seen);
        assert_eq!(seen.keys().collect::<Vec<_>>(), vec!["bid"]);
        window_result_text("Arg2", Some(""), None, Some("I"), "T", &mut seen);
        assert!(seen.has("Arg2"), "object with empty bundleIdentifier falls through to the arg");
        window_result_text("Arg3", None, Some("Name"), Some("I"), "T", &mut seen);
        assert!(seen.has("Name"));
        window_result_text("Arg", None, Some(""), Some("I"), "T", &mut seen);
        assert!(seen.has("Arg"), "empty string app falls through to the arg");
        assert_eq!(seen.keys().count(), 4);
        // API-only combination (unreachable in the JS, both fields set): the bundle id still wins
        let mut api = DeliveryState::new();
        window_result_text("Arg", Some("bid"), Some("Name"), Some("I"), "T", &mut api);
        assert_eq!(api.keys().collect::<Vec<_>>(), vec!["bid"]);
        // a repeat by any of those keys is plain
        assert_eq!(window_result_text("Arg", None, None, Some("I"), "T", &mut seen), "T");
        assert_eq!(window_result_text("zzz", Some("bid"), None, Some("I"), "T", &mut seen), "T");
    }

    #[test]
    fn js_port_no_instructions_is_plain_and_unrecorded() {
        let mut seen = DeliveryState::new();
        assert_eq!(window_result_text("A", Some("bid"), None, None, "T", &mut seen), "T");
        assert_eq!(window_result_text("A", Some("bid"), None, Some(""), "T", &mut seen), "T");
        assert_eq!(seen.keys().count(), 0);
    }

    #[test]
    fn delivery_state_json_roundtrip_keeps_dedup_across_process_boundary() {
        let mut s = DeliveryState::new();
        s.deliver("com.apple.clock", Some("P"), "t");
        s.deliver("Name With Spaces", Some("P"), "t");
        let json = s.to_json();
        let r = DeliveryState::from_json(&json);
        assert_eq!(r, s);
        let mut r = r;
        assert_eq!(r.deliver("com.apple.clock", Some("P"), "t2"), "t2", "restored state must still dedup");
        assert_eq!(r.deliver("new", Some("P"), "t3"), wrap("P", "t3"));
        // malformed store -> fresh session, never a panic
        assert_eq!(DeliveryState::from_json("not json"), DeliveryState::new());
        assert_eq!(DeliveryState::from_json(""), DeliveryState::new());
    }
}

#[cfg(test)]
mod supplement_tests {
    use super::*;

    #[test]
    fn spotify_supplement_is_verbatim_from_the_binary() {
        // exact bytes strings(1) pulled from SkyComputerUseService (spec 04 step 5)
        assert!(SPOTIFY_SUPPLEMENT.starts_with("Note: In order to be usable, Spotify app links must be rewritten as regular links"));
        assert!(SPOTIFY_SUPPLEMENT.contains("open.spotify.com instead of xpui.app.spotify.com"));
        assert!(SPOTIFY_SUPPLEMENT.contains("cannot change an \"album\" URL to a \"track\" URL"));
        assert_eq!(supplement_for_bundle_id("com.spotify.client"), Some(SPOTIFY_SUPPLEMENT));
        assert_eq!(supplement_for_bundle_id("com.apple.clock"), None);
    }

    #[test]
    fn supplement_follows_the_tree_and_pack_leads_first_touch_only() {
        let mut d = DeliveryState::new();
        let out = d.deliver_with_supplement("com.spotify.client", Some("PACK BODY"), Some(SPOTIFY_SUPPLEMENT), "TREE");
        // pack is wrapped and prefixed; the supplement is appended AFTER the tree
        assert!(out.starts_with("<app_specific_instructions>
PACK BODY
</app_specific_instructions>
TREE"));
        assert!(out.trim_end().ends_with(SPOTIFY_SUPPLEMENT), "supplement after the tree:
{out}");
        assert!(out.find("PACK BODY").unwrap() < out.find("TREE").unwrap() && out.find("TREE").unwrap() < out.find("rewritten").unwrap());
        // second touch: neither pack nor supplement repeats
        let out2 = d.deliver_with_supplement("com.spotify.client", Some("PACK BODY"), Some(SPOTIFY_SUPPLEMENT), "TREE2");
        assert_eq!(out2, "TREE2");
    }
}
