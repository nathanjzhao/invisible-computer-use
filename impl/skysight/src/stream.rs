//! The event-stream recorder, platform-neutral half: Codex Skysight's
//! `EventStreamRecorder` record types, its inline redaction tokens, the observation
//! policy (per-app and per-URL-domain rules, the URL blocklist, private browsing,
//! system and background-only apps, the locked screen), the typed-text buffer and the
//! terminal buffer. A platform layer ([`crate::macos`]) turns what it sees into
//! [`Observation`]s; [`Recorder::observe`] turns those into records or suppressed
//! markers. Nothing here reads the screen or posts input.
//!
//! CONFIRMED from `SkyComputerUseService` strings (spec 03, refresh 2026-10-08): the
//! event type names, the redaction tokens, `TextInputBuffer` /
//! `TerminalValueChangedBuffer` / `latestURLByWindowID`, `shouldCaptureApplication`,
//! `onSuppressedRecordStored`, `EventStreamURLPolicyRecordFilter.isURLBlocked`,
//! `ObservationRuleScope` (app / url), `ObservationDefaultBehavior`,
//! `_observePrivateBrowsing`, `PrivateBrowsingIndicators` with the title markers
//! below, `secureInputPID`, `isScreenLocked`, and the `/System/` path checks.
//! INFERRED (the binary does not pin them): the record field names, the flush rules of
//! the text buffer (submit, focus change, click, [`IDLE_FLUSH_SECS`] idle,
//! [`MAX_BUFFER_CHARS`]), and how terminal output is diffed.

use crate::{redact_command, EventRecord, Ts};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

pub const SESSION_STARTED: &str = "session.started";
pub const SESSION_ENDED: &str = "session.ended";
pub const WINDOW_CHANGED: &str = "window.changed";
pub const MOUSE_CLICK: &str = "mouse.click";
pub const MOUSE_CONTEXT_MENU: &str = "mouse.context_menu";
pub const MOUSE_DRAG: &str = "mouse.drag";
pub const TEXT_INPUT: &str = "keyboard.text_input";
pub const SUBMIT: &str = "keyboard.submit";
pub const SHORTCUT: &str = "keyboard.shortcut";
pub const TERMINAL_VALUE_CHANGED: &str = "terminal.value_changed";
pub const SELECTION_CHANGED: &str = "selection.changed";

/// Every event type the recorder writes, in Codex's names.
pub const EVENT_TYPES: &[&str] = &[SESSION_STARTED, SESSION_ENDED, WINDOW_CHANGED, MOUSE_CLICK, MOUSE_CONTEXT_MENU, MOUSE_DRAG,
    TEXT_INPUT, SUBMIT, SHORTCUT, TERMINAL_VALUE_CHANGED, SELECTION_CHANGED];

/// Whether `kind` is one of the event-stream types (as opposed to the legacy focus-only
/// kinds such as `app_focus`). Stream records were redacted when they were captured.
pub fn is_stream_kind(kind: &str) -> bool { EVENT_TYPES.contains(&kind) }

/// Typed text is flushed into one `keyboard.text_input` record after this many seconds
/// without a keystroke (INFERRED).
pub const IDLE_FLUSH_SECS: i64 = 5;
/// A buffer this long is flushed even while typing continues.
pub const MAX_BUFFER_CHARS: usize = 2000;
/// Bounds on what one record carries.
pub const MAX_LABEL_CHARS: usize = 160;
pub const MAX_TITLE_CHARS: usize = 300;
pub const MAX_SELECTION_CHARS: usize = 1000;
pub const MAX_TERMINAL_CHARS: usize = 2000;

// ---- redaction -----------------------------------------------------------------------

pub mod redact {
    //! Inline redaction to Codex's token set, then the crate's own secret passes. Applied
    //! to every free-text field before a record is written, so a secret never reaches
    //! the disk, the summarizer or memory.
    use regex::Regex;
    use std::sync::OnceLock;

    pub const API_KEY: &str = "[REDACTED_API_KEY]";
    pub const AWS_ACCESS_KEY: &str = "[REDACTED_AWS_ACCESS_KEY]";
    pub const BEARER_TOKEN: &str = "[REDACTED_BEARER_TOKEN]";
    pub const SECRET: &str = "[REDACTED_SECRET]";
    pub const CREDIT_CARD: &str = "[REDACTED_CREDIT_CARD]";
    pub const TOKENS: &[&str] = &[API_KEY, AWS_ACCESS_KEY, BEARER_TOKEN, SECRET, CREDIT_CARD];

    fn re(cell: &'static OnceLock<Regex>, pattern: &str) -> &'static Regex {
        cell.get_or_init(|| Regex::new(pattern).expect("redaction pattern"))
    }

    /// `Bearer x`, `Basic x`, and the value of an `Authorization:` header. The value must look
    /// like a credential (a digit, `=`, `.`, `_` or `-` in it, or 20+ characters), so prose
    /// such as "Basic authentication" is left alone.
    fn bearer(s: &str) -> String {
        static R: OnceLock<Regex> = OnceLock::new();
        static H: OnceLock<Regex> = OnceLock::new();
        let looks = |v: &str| v.len() >= 20 || v.chars().any(|c| c.is_ascii_digit() || matches!(c, '=' | '.' | '_' | '-'));
        let s = re(&H, r"(?i)\b(authorization\s*:\s*)(token\s+|bearer\s+|basic\s+)?([^\s\x22',;]{8,})")
            .replace_all(s, |c: &regex::Captures| if looks(&c[3]) { format!("{}{}{}", &c[1], c.get(2).map_or("", |m| m.as_str()), BEARER_TOKEN) } else { c[0].to_string() });
        re(&R, r"(?i)\b(bearer|basic)(\s+)([A-Za-z0-9\-._~+/]{8,}=*)")
            .replace_all(&s, |c: &regex::Captures| if looks(&c[3]) && !TOKENS.iter().any(|t| c[3].contains(&t[1..t.len() - 1])) { format!("{}{}{}", &c[1], &c[2], BEARER_TOKEN) } else { c[0].to_string() }).into_owned()
    }

    fn aws(s: &str) -> String {
        static R: OnceLock<Regex> = OnceLock::new();
        re(&R, r"\b(AKIA|ASIA|AGPA|AIDA|AROA|ANPA|ANVA|AIPA)[A-Z0-9]{16}\b").replace_all(s, AWS_ACCESS_KEY).into_owned()
    }

    /// Tokens whose prefix names their issuer.
    fn api_keys(s: &str) -> String {
        static R: OnceLock<Regex> = OnceLock::new();
        re(&R, concat!(r"\b(",
            r"sk-ant-[A-Za-z0-9_\-]{20,}|sk-proj-[A-Za-z0-9_\-]{20,}|sk-[A-Za-z0-9_\-]{20,}|",
            r"gh[pousr]_[A-Za-z0-9]{20,}|github_pat_[A-Za-z0-9_]{20,}|glpat-[A-Za-z0-9_\-]{20,}|",
            r"xox[abprs]-[A-Za-z0-9\-]{10,}|xapp-[A-Za-z0-9\-]{10,}|",
            r"(sk|rk|pk)_(live|test)_[A-Za-z0-9]{10,}|AIza[A-Za-z0-9_\-]{30,}|ya29\.[A-Za-z0-9_\-]{20,}|",
            r"npm_[A-Za-z0-9]{30,}|hf_[A-Za-z0-9]{30,}|shpat_[A-Za-z0-9]{20,}|dop_v1_[A-Za-z0-9]{40,}|SG\.[A-Za-z0-9_\-\.]{20,}",
            r")")).replace_all(s, API_KEY).into_owned()
    }

    /// JSON web tokens and PEM private-key blocks.
    fn structured(s: &str) -> String {
        static JWT: OnceLock<Regex> = OnceLock::new();
        static PEM: OnceLock<Regex> = OnceLock::new();
        let s = re(&JWT, r"\beyJ[A-Za-z0-9_\-]{8,}\.[A-Za-z0-9_\-]{8,}\.[A-Za-z0-9_\-]{8,}").replace_all(s, SECRET);
        re(&PEM, r"(?s)-----BEGIN [A-Z ]*PRIVATE KEY-----.*?(-----END [A-Z ]*PRIVATE KEY-----|$)").replace_all(&s, SECRET).into_owned()
    }

    /// A value after a name that says it is a secret: `password=x`, `"api_key": "x"`,
    /// `secret: x`, `passwd x` is left alone (a word, not an assignment).
    fn pairs(s: &str) -> String {
        static R: OnceLock<Regex> = OnceLock::new();
        re(&R, concat!(r#"(?i)(["']?[A-Za-z0-9_\-.]*(pass(word|wd|phrase)?|pwd|secret|token|api[_\-]?key|access[_\-]?key|private[_\-]?key|client[_\-]?secret|auth)["']?\s*[:=]\s*)"#,
            r#"(["']?)([^\s"',;&]{3,})(["']?)"#))
            .replace_all(s, |c: &regex::Captures| {
                let v = &c[5];
                if TOKENS.iter().any(|t| v.contains(t)) || matches!(v.to_ascii_lowercase().as_str(), "true" | "false" | "null" | "none" | "yes" | "no") {
                    c[0].to_string()
                } else { format!("{}{}{}{}", &c[1], &c[4], SECRET, &c[6]) }
            }).into_owned()
    }

    fn luhn(digits: &[u32]) -> bool {
        let mut sum = 0;
        for (i, d) in digits.iter().rev().enumerate() {
            let mut d = *d;
            if i % 2 == 1 { d *= 2; if d > 9 { d -= 9; } }
            sum += d;
        }
        sum % 10 == 0
    }

    /// 13 to 19 digits, optionally grouped by spaces or dashes, that pass the Luhn check.
    fn cards(s: &str) -> String {
        static R: OnceLock<Regex> = OnceLock::new();
        re(&R, r"\b\d(?:[ \-]?\d){12,18}\b").replace_all(s, |c: &regex::Captures| {
            let digits: Vec<u32> = c[0].chars().filter_map(|ch| ch.to_digit(10)).collect();
            if (13..=19).contains(&digits.len()) && luhn(&digits) { CREDIT_CARD.to_string() } else { c[0].to_string() }
        }).into_owned()
    }

    /// A long opaque token: 32+ characters mixing letters and digits with no spaces, the
    /// shape of a key or a hash that no prefix names.
    fn opaque(s: &str) -> String {
        static R: OnceLock<Regex> = OnceLock::new();
        re(&R, r"[A-Za-z0-9_\-+/=]{32,}").replace_all(s, |c: &regex::Captures| {
            let t = &c[0];
            let letters = t.chars().filter(|ch| ch.is_ascii_alphabetic()).count();
            let digits = t.chars().filter(|ch| ch.is_ascii_digit()).count();
            let path_like = t.contains('/') && t.split('/').all(|seg| seg.len() < 32);
            if letters >= 4 && digits >= 4 && !path_like { SECRET.to_string() } else { t.to_string() }
        }).into_owned()
    }

    /// Codex's token set, in the order that keeps the most specific name.
    pub fn tokens(text: &str) -> String {
        let s = structured(text);
        let s = aws(&s);
        let s = api_keys(&s);
        let s = bearer(&s);
        let s = pairs(&s);
        let s = cards(&s);
        opaque(&s)
    }

    /// [`tokens`], then the crate's own passes: the password in a URL's userinfo and any
    /// prefixed token the first pass left.
    pub fn inline(text: &str) -> String {
        crate::redact_userinfo_pub(&crate::redact_known_tokens_pub(&tokens(text)))
    }
}

// ---- observation policy ---------------------------------------------------------------

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DefaultBehavior { #[default] Observe, DontObserve }

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RuleScope { App, Url }

/// One observe / don't-observe rule, in the shape Codex's `skysightUpdateObservationPolicy`
/// sends: `{scope: "app", bundleID}` or `{scope: "url", urlDomain}`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ObservationRule {
    pub scope: RuleScope,
    #[serde(default, rename = "bundleID", skip_serializing_if = "Option::is_none")]
    pub bundle_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub url_domain: Option<String>,
    pub observe: bool,
}

/// What the recorder may observe.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct ObservationSettings {
    pub default_behavior: DefaultBehavior,
    pub rules: Vec<ObservationRule>,
    /// Domains never observed, whatever the rules say (`example.com` covers its subdomains).
    pub url_blocklist: Vec<String>,
    /// Private and incognito browser windows are skipped unless this is on.
    pub observe_private_browsing: bool,
}

pub const MAX_RULES: usize = 500;

fn valid_domain(d: &str) -> bool {
    !d.is_empty() && d.len() <= 253 && !d.starts_with('.') && !d.ends_with('.')
        && d.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-')) && d.contains('.')
}

fn valid_bundle(b: &str) -> bool { !b.trim().is_empty() && b.len() <= 255 && !b.contains(char::is_whitespace) }

/// A domain as rules store it: lowercase, no scheme, path, port or leading `*.`.
pub fn normalize_domain(d: &str) -> String {
    let d = d.trim().to_ascii_lowercase();
    let d = d.split("://").last().unwrap_or("").to_string();
    let d = d.split(['/', '?', '#']).next().unwrap_or("").to_string();
    let d = d.rsplit('@').next().unwrap_or("").to_string();
    let d = d.split(':').next().unwrap_or("").to_string();
    d.trim_start_matches("*.").to_string()
}

impl ObservationSettings {
    pub fn validate(&self) -> Result<(), String> {
        if self.rules.len() > MAX_RULES || self.url_blocklist.len() > MAX_RULES { return Err(format!("at most {MAX_RULES} rules")); }
        for r in &self.rules {
            match r.scope {
                RuleScope::App => if !r.bundle_id.as_deref().is_some_and(valid_bundle) {
                    return Err("app observation rules require only a non-empty bundleID".into());
                },
                RuleScope::Url => if !r.url_domain.as_deref().is_some_and(valid_domain) {
                    return Err("URL observation rules require only a valid urlDomain".into());
                },
            }
        }
        if let Some(d) = self.url_blocklist.iter().find(|d| !valid_domain(d)) { return Err(format!("invalid blocklist domain {d:?}")); }
        Ok(())
    }

    /// Set one rule, replacing the rule for the same app or domain.
    pub fn set_rule(&mut self, rule: ObservationRule) -> Result<(), String> {
        let rule = ObservationRule { url_domain: rule.url_domain.as_deref().map(normalize_domain), ..rule };
        self.rules.retain(|r| !(r.scope == rule.scope && r.bundle_id == rule.bundle_id && r.url_domain == rule.url_domain));
        self.rules.push(rule);
        self.validate()
    }

    fn app_rule(&self, bundle: &str) -> Option<bool> {
        self.rules.iter().rev().find(|r| r.scope == RuleScope::App && r.bundle_id.as_deref() == Some(bundle)).map(|r| r.observe)
    }

    /// The most specific URL rule for `host` (the longest matching domain).
    fn url_rule(&self, host: &str) -> Option<bool> {
        self.rules.iter().filter(|r| r.scope == RuleScope::Url)
            .filter_map(|r| r.url_domain.as_deref().filter(|d| domain_matches(host, d)).map(|d| (d.len(), r.observe)))
            .max_by_key(|(n, _)| *n).map(|(_, o)| o)
    }

    pub fn url_blocked(&self, url: &str) -> bool {
        let Some(host) = url_host(url) else { return false };
        self.url_blocklist.iter().any(|d| domain_matches(&host, &normalize_domain(d)))
    }
}

/// `host` is `domain` or one of its subdomains.
pub fn domain_matches(host: &str, domain: &str) -> bool {
    let (h, d) = (host.to_ascii_lowercase(), domain.to_ascii_lowercase());
    !d.is_empty() && (h == d || h.ends_with(&format!(".{d}")))
}

/// The host of an `http(s)` URL, lowercase, without userinfo or port.
pub fn url_host(url: &str) -> Option<String> {
    let rest = url.trim().split_once("://").map(|(_, r)| r)?;
    let auth = rest.split(['/', '?', '#']).next()?;
    let host = auth.rsplit('@').next()?.split(':').next()?.to_ascii_lowercase();
    (!host.is_empty()).then_some(host)
}

/// Window-title and toolbar markers of a private or incognito browser window
/// (`PrivateBrowsingIndicators`; the localized ones are those in the binary).
pub const PRIVATE_TITLE_MARKERS: &[&str] = &["(Incognito)", "(In incognito)", "(Private)", "— Private", "- Private", "Private Browsing",
    "InPrivate", "Privater Modus", "Privates Surfen", "Navigation privée", "Navigation private"];
/// Exact control labels that mark a private window (Safari shows "Private" in the tab
/// bar / search field of a private window).
pub const PRIVATE_LABELS: &[&str] = &["Private", "Private Browsing", "Incognito", "InPrivate", "Privat", "Privé", "Privado"];

/// Whether a window with this title, and these labels found on its toolbar, is private.
pub fn looks_private(title: Option<&str>, labels: &[String]) -> bool {
    title.is_some_and(|t| PRIVATE_TITLE_MARKERS.iter().any(|m| t.contains(m)))
        || labels.iter().any(|l| { let l = l.trim(); PRIVATE_LABELS.iter().any(|p| l.eq_ignore_ascii_case(p)) || l.starts_with("Private Browsing") })
}

/// Terminal apps, whose submitted lines are commands and whose output is diffed.
pub const TERMINAL_BUNDLES: &[&str] = &["com.apple.Terminal", "com.googlecode.iterm2", "dev.warp.Warp-Stable", "com.mitchellh.ghostty",
    "net.kovidgoyal.kitty", "io.alacritty", "com.github.wez.wezterm"];
/// Browsers whose windows carry a URL.
pub const BROWSER_BUNDLES: &[&str] = &["com.apple.Safari", "com.apple.SafariTechnologyPreview", "com.google.Chrome", "com.google.Chrome.canary",
    "company.thebrowser.Browser", "com.microsoft.edgemac", "com.brave.Browser", "org.mozilla.firefox", "org.chromium.Chromium", "com.vivaldi.Vivaldi", "com.operasoftware.Opera"];

pub fn is_terminal(bundle: Option<&str>) -> bool {
    bundle.map(|b| b.strip_suffix(" (deleted)").unwrap_or(b)).is_some_and(|b| TERMINAL_BUNDLES.contains(&b) || matches!(b.to_ascii_lowercase().as_str(),
        "windowsterminal.exe"|"openconsole.exe"|"conhost.exe"|"cmd.exe"|"powershell.exe"|"pwsh.exe"|
        "wsl.exe"|"mintty.exe"|"alacritty.exe"|"wezterm-gui.exe"|
        "gnome-terminal-server"|"gnome-console"|"kgx"|"konsole"|"xterm"|"uxterm"|"xfce4-terminal"|
        "mate-terminal"|"tilix"|"terminator"|"alacritty"|"kitty"|"foot"|"footclient"|"wezterm-gui"|
        "qterminal"|"lxterminal"|"st"|"urxvt"|"urxvtd"|"ptyxis"|"cosmic-term"))
}
pub fn is_browser(bundle: Option<&str>) -> bool { bundle.is_some_and(|b| BROWSER_BUNDLES.contains(&b)) }

// ---- observations ---------------------------------------------------------------------

/// The app in front.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct AppInfo {
    pub bundle_id: Option<String>,
    pub name: String,
    pub pid: i64,
    /// The bundle's path on disk (`/System/Library/...` apps are skipped).
    pub path: Option<String>,
    /// No Dock icon and no menu bar (an agent or a background-only process).
    pub background_only: bool,
}

/// Its focused window.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct WindowInfo {
    pub id: u32,
    pub title: Option<String>,
    /// The page URL, for a browser window.
    pub url: Option<String>,
    /// The document's path (`AXDocument`), when the window has one.
    pub document: Option<String>,
    /// The platform found a private-browsing marker ([`looks_private`]).
    pub private: bool,
    /// The URLs of the frames between the focused element and the page, innermost first (an
    /// iframe's own web area). `url` is the page's. Every one of them is judged by the blocklist
    /// and the site rules (round 5, M1: a login frame from another site inside a blocked page was
    /// judged by the frame's URL alone).
    pub frame_urls: Vec<String>,
}

/// The page URL and the frame URLs around an element, from a walk up its parents: every web
/// area met on the way is a frame, the outermost one is the page. `None` when the element is in
/// no web area (the address bar, a start page). When the walk runs out of hops before the top,
/// the page is not known (`url: None`) and the frames found are kept.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct PageUrls { pub url: Option<String>, pub frames: Vec<String> }

pub fn page_urls_up<N>(start: N, max_hops: usize, is_web_area: impl Fn(&N) -> bool, url: impl Fn(&N) -> Option<String>,
    parent: impl Fn(&N) -> Option<N>) -> Option<PageUrls> {
    let mut found: Vec<Option<String>> = Vec::new();
    let mut el = start;
    for _ in 0..max_hops {
        if is_web_area(&el) { found.push(url(&el)); }
        match parent(&el) {
            Some(p) => el = p,
            None => {
                let top = found.pop()?;
                return Some(PageUrls { url: top, frames: found.into_iter().flatten().collect() });
            }
        }
    }
    (!found.is_empty()).then(|| PageUrls { url: None, frames: found.into_iter().flatten().collect() })
}

/// Per-process watches (the macOS source's focus observers on browsers), keyed by pid and the
/// process's start time, so a pid the system hands to a new process is not taken for the old one
/// (round 7, L4).
pub struct Watches<T> { m: BTreeMap<i32, (u64, T)> }

impl<T> Default for Watches<T> { fn default() -> Self { Watches { m: BTreeMap::new() } } }

impl<T> Watches<T> {
    /// Watch `pid` started at `start`: nothing when it is already watched; otherwise a watch made by
    /// `make` replaces any left from an earlier process with that pid, which is returned to release.
    pub fn add(&mut self, pid: i32, start: u64, make: impl FnOnce() -> Option<T>) -> Vec<T> {
        if self.m.get(&pid).is_some_and(|(s, _)| *s == start) { return vec![]; }
        let old = self.m.remove(&pid).map(|(_, t)| t).into_iter().collect();
        if let Some(t) = make() { self.m.insert(pid, (start, t)); }
        old
    }
    /// Drop the watches whose process is gone (`start_of` gives a live pid's start time).
    pub fn prune(&mut self, start_of: impl Fn(i32) -> Option<u64>) -> Vec<T> {
        let gone: Vec<i32> = self.m.iter().filter(|(p, (s, _))| start_of(**p) != Some(*s)).map(|(p, _)| *p).collect();
        gone.into_iter().filter_map(|p| self.m.remove(&p).map(|(_, t)| t)).collect()
    }
    /// What is kept for `pid` when it is still the process that started at `start` (fresh review,
    /// F5: an app's identity cached by pid alone passed to whatever process got the pid next).
    pub fn get(&self, pid: i32, start: u64) -> Option<&T> { self.m.get(&pid).filter(|(s, _)| *s == start).map(|(_, t)| t) }
    pub fn len(&self) -> usize { self.m.len() }
    pub fn is_empty(&self) -> bool { self.m.is_empty() }
    pub fn drain(&mut self) -> Vec<T> { std::mem::take(&mut self.m).into_values().map(|(_, t)| t).collect() }
}

/// The window's one page, found by a bounded walk down that does not enter pages (round 5, L2):
/// `None` when there is none, when there is more than one (a side panel or a docked inspector next
/// to the tab, and which one the user is on is not known), or when the walk was cut short.
pub fn sole_web_area<N>(root: &N, max_nodes: usize, max_depth: usize, children: impl Fn(&N) -> Vec<N>, is_web_area: impl Fn(&N) -> bool) -> Option<N> {
    let mut q: std::collections::VecDeque<(N, usize)> = children(root).into_iter().map(|c| (c, 1)).collect();
    let (mut seen, mut found) = (0, Vec::new());
    while let Some((c, depth)) = q.pop_front() {
        seen += 1;
        if seen > max_nodes { return None; }
        if is_web_area(&c) { found.push(c); if found.len() > 1 { return None; } continue; }
        if depth < max_depth { q.extend(children(&c).into_iter().map(|k| (k, depth + 1))); }
    }
    found.pop()
}

/// The accessibility element an input went to.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Target {
    pub role: Option<String>,
    pub subrole: Option<String>,
    pub label: Option<String>,
    /// A secure text field (`AXSecureTextField`): nothing typed in it is recorded.
    pub secure: bool,
    /// The process the element belongs to, when the platform read it.
    pub pid: Option<i64>,
    /// Known native window ancestry, absent for web, secure or uncertain nodes.
    pub native_window_id: Option<u32>,
}

impl Target {
    fn same_field(&self, other: &Target) -> bool { self.role == other.role && self.label == other.label }
    pub fn is_secure(&self) -> bool {
        self.secure || [self.role.as_deref(), self.subrole.as_deref()].into_iter().flatten().any(|r| r == "AXSecureTextField")
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Mods { pub cmd: bool, pub ctrl: bool, pub alt: bool, pub shift: bool }

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Key { Char, Return, Tab, Backspace, Escape, ForwardDelete, Arrow, Other }

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Button { Left, Right, Other }

/// What the platform layer saw.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Observation {
    /// The frontmost app and its focused window (sent on change, or on every poll).
    Focus { app: AppInfo, window: Option<WindowInfo> },
    Click { button: Button, count: u32, ctrl: bool, target: Option<Target> },
    Drag { target: Option<Target> },
    /// One key down. `chars` is what it types; `secure_input` is the system's secure
    /// keyboard entry flag (`IsSecureEventInputEnabled`) at that moment.
    Key { chars: String, key: Key, mods: Mods, target: Option<Target>, secure_input: bool, key_name: String },
    /// The full text of the focused terminal view, read after a submit.
    TerminalValue { window_id: u32, value: String },
    /// The selected text of the focused element, read after a selection gesture.
    Selection { text: String, target: Option<Target> },
    /// The screen locked or unlocked.
    Locked(bool),
}

/// Why something was not recorded. A suppressed marker carries the time, the event type
/// and this reason, and nothing of the content.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Reason { ScreenLocked, OwnApp, SystemApp, BackgroundApp, AppRule, DefaultDontObserve, UrlRule, UrlBlocked, PrivateBrowsing, SecureInput,
    /// A browser window whose URL could not be read while a site rule or the blocklist applies:
    /// it may be a blocked site, so nothing in it is kept.
    UrlUnknown,
    /// No app is known to be in front (the recorder has not seen the focus yet).
    NoFocus,
    /// A terminal asking for a password, passphrase or PIN, or one whose prompt was not read
    /// since the last command (the next line might be a secret).
    TerminalSecret }

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Suppressed { pub ts: String, pub kind: String, pub reason: Reason }

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Output { Record(EventRecord), Suppressed(Suppressed) }

impl Output {
    pub fn record(&self) -> Option<&EventRecord> { match self { Output::Record(r) => Some(r), _ => None } }
}

/// The ISO form records carry.
pub fn iso(ts: Ts) -> String {
    let f = crate::format_ts(ts);
    format!("{}T{}Z", &f[..10], f[11..].replace('-', ":"))
}

fn bound(s: &str, n: usize) -> String {
    if s.chars().count() <= n { s.to_string() } else { s.chars().take(n).collect::<String>() + "…" }
}

/// A host hook run after the token pass (the app passes its prose scrub).
pub type Scrub = Box<dyn Fn(&str) -> String + Send + Sync>;

struct TextBuf { text: String, target: Target, window: Option<u32>, last: Ts, started: Ts }

/// Whether an Accessibility element read for the app in front may be used: it belongs to that app
/// (round 7: a focus taken from the frontmost window's owner may be another app's).
pub fn ax_element_is_front(el_pid: Option<i32>, front_pid: Option<i32>) -> bool {
    matches!((el_pid, front_pid), (Some(a), Some(b)) if a > 0 && a == b)
}

/// A terminal's prompt, as the read before a line's first key ended (fresh review 3, item 1).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Prompt {
    /// The whole last line, trailing spaces dropped.
    pub left: String,
    /// The same line split before a right-hand prompt (zsh's RPROMPT, powerlevel10k's status and
    /// clock): what precedes a run of three or more spaces, and what follows it. Used only when the
    /// whole line is not found again (round 4, item 5: a prompt with three spaces of its own).
    pub split: Option<(String, String)>,
    /// The non-empty line above it (a question printed on a line of its own).
    pub above: Option<String>,
    /// The widest line of that read, in characters: the terminal's width, as far as it shows.
    pub width: usize,
}

/// Digits read as zeros: a prompt's clock, exit code or git counts change between the read before
/// typing and the read at Return (round 4, item 4). Same length in bytes.
fn masked(s: &str) -> String { s.chars().map(|c| if c.is_ascii_digit() { '0' } else { c }).collect() }

/// The prompts zsh and bash print while a command goes on over more lines (a backslash at the end,
/// an open quote, a heredoc): `> `, `quote> `, `dquote> `, `heredoc> `, `cmdsubst> ` and the like.
pub fn is_continuation_prompt(left: &str) -> bool {
    static RE: std::sync::OnceLock<regex::Regex> = std::sync::OnceLock::new();
    let re = RE.get_or_init(|| regex::Regex::new(concat!(r"^(?:(?:quote|dquote|bquote|heredoc|cmdsubst|subsh|pipe|cmdand|cmdor|for|while|until|if|then|else|elif|",
        r"function|case|select|foreach|repeat|cursh|braceparam|math|array|mathsubst|nested) ?)*>$")).unwrap());
    re.is_match(left.trim())
}

impl Prompt {
    /// The prompt a terminal's text ends with.
    pub fn of(text: &str) -> Prompt {
        let lines: Vec<&str> = text.trim_end_matches(['\n', '\r']).split('\n').map(|l| l.trim_end_matches('\r')).collect();
        let last = lines.last().copied().unwrap_or("");
        let above = lines.iter().rev().skip(1).find(|l| !l.trim().is_empty()).map(|l| l.trim_end().to_string());
        let width = lines.iter().map(|l| l.chars().count()).max().unwrap_or(0);
        static RIGHT: std::sync::OnceLock<regex::Regex> = std::sync::OnceLock::new();
        let re = RIGHT.get_or_init(|| regex::Regex::new(r"\S {3,}(\S.*?)\s*$").unwrap());
        let split = re.captures(last).map(|c| { let m = c.get(1).unwrap(); (last[..m.start()].trim_end().to_string(), m.as_str().to_string()) });
        Prompt { left: last.trim_end().to_string(), split, above, width }
    }

    /// The prompt asks for a password, passphrase, PIN, code or key: on its line, on the line above
    /// it (`print("Enter your API key:"); input()`), or wrapped across the two.
    pub fn asks_secret(&self) -> bool {
        let asks = |left: &str| terminal_asks_secret(left)
            || self.above.as_deref().is_some_and(|a| (prompt_only(left) && terminal_asks_secret(a)) || wrapped_asks_secret(a, left));
        asks(&self.left) || self.split.as_ref().is_some_and(|(l, _)| asks(l))
    }

    /// The prompt's left parts to look for, the whole line first, each followed by the same with
    /// the first keys of the line taken off its end (round 4, item 1: the read before the first key
    /// can land after a key or two were echoed, and they are then at the prompt's end).
    /// Of `earlier` prompt parts, the ones this prompt (or its left side) extends: what follows them
    /// may be input echoed before the read, the longest first.
    pub fn extended(&self, earlier: &[(String, bool)]) -> Vec<(String, bool)> {
        let mine = |split: bool| if split { self.split.as_ref().map(|(l, _)| l.as_str()) } else { Some(self.left.as_str()) };
        let mut v: Vec<(String, bool)> = earlier.iter().filter(|(e, split)| !e.trim().is_empty()
            && mine(*split).or(mine(false)).is_some_and(|m| m.len() > e.len() && masked(m).starts_with(&masked(e))))
            // the right prompt is cut off whenever this prompt has one in front of which the part ends
            .map(|(e, split)| (e.clone(), *split || self.split.as_ref().is_some_and(|(l, _)| masked(l).starts_with(&masked(e))))).collect();
        v.sort_by_key(|(e, _)| std::cmp::Reverse(e.len()));
        v.dedup();
        v
    }

    fn lefts(&self, lead: Option<&str>) -> Vec<(String, bool)> {
        let mut bases = vec![(self.left.clone(), false)];
        if let Some((l, _)) = &self.split { bases.push((l.clone(), true)); }
        let mut out = Vec::new();
        for (b, split) in bases {
            out.push((b.clone(), split));
            if let Some(lead) = lead {
                let mut ends: Vec<usize> = lead.char_indices().map(|(i, c)| i + c.len_utf8()).collect();
                ends.reverse();
                for k in ends { if b.ends_with(&lead[..k]) { out.push((b[..b.len() - k].to_string(), split)); } }
            }
        }
        out
    }
}

/// The line typed after `prompt`, as `text` (read as Return was pressed) shows it, with the prompt
/// part it was found after. The line is the last one starting with the prompt (digits aside), the
/// right-hand prompt dropped, with the lines below it when they are the terminal's wrap of it: when
/// the keys typed are the line's end as joined, or each line before one is exactly as wide as the
/// terminal (the widest line seen outside the line itself, in this read or the one before typing);
/// output below the line is neither. When the line started with keys typed, it has to start with
/// the first of them (`lead`). `None` when no prompt part is there, or other lines follow (the read
/// came after the terminal took the Return), or the line does not start with its first keys.
///
/// Round 5, item 1: a paste (no keys to go by) can already be on screen when the read before the
/// line lands, and then reads as the prompt's end, leaving the line empty. When the line had input
/// (`need_text`), an empty line is not taken, and `earlier` (prompt parts from reads before the
/// line, and ones lines were taken after, each with whether it is a right-prompt split) is tried
/// where the prompt as read extends one of them.
///
/// Mac run (Terminal.app, macOS 26): the read before the line raced the terminal drawing a paste
/// or a history line, and caught its first character (`➜ … ✗ m`), so the line read `ysql -u root
/// -phunter2`. A prompt part an earlier line was taken after (`known`), when the prompt as read
/// extends it, is tried first. And the read at Return often lands after the terminal took the
/// Return (output and a new prompt below the line): when every key of the line was typed in order
/// with nothing else (`exact`), the line is the last one at the prompt that holds exactly them.
pub fn line_after(text: &str, prompt: &Prompt, keys: &str, lead: Option<&str>, earlier: &[(String, bool)], need_text: bool, known: &[String], exact: bool) -> Option<(String, String)> {
    let lines: Vec<&str> = text.trim_end_matches(['\n', '\r']).split('\n').map(|l| l.trim_end_matches('\r')).collect();
    let masked_lines: Vec<String> = lines.iter().map(|l| masked(l)).collect();
    static TRAIL: std::sync::OnceLock<regex::Regex> = std::sync::OnceLock::new();
    let trail = TRAIL.get_or_init(|| regex::Regex::new(r"\s{3,}\S(?:\S| {1,2}\S)*\s*$").unwrap());
    let first: Option<String> = lead.map(|l| l.chars().take(3).collect());
    let known: Vec<(String, bool)> = known.iter().map(|k| (k.clone(), false)).collect();
    let mut candidates = prompt.extended(&known);
    candidates.extend(prompt.lefts(lead));
    candidates.extend(prompt.extended(earlier));
    for (left, split) in candidates.clone() {
        let ml = masked(&left);
        let Some(i) = (0..lines.len()).rev().find(|&i| masked_lines[i].starts_with(&ml)) else { continue };
        let width = lines[..i].iter().map(|l| l.chars().count()).max().unwrap_or(0).max(prompt.width);
        let strip = |l: &str| -> String {
            let l = l.trim_end();
            if !split { return l.trim().to_string(); }
            let right = prompt.split.as_ref().map(|(_, r)| r.as_str()).unwrap_or("");
            // the right prompt as read before typing, or changed since (a status, a clock)
            let l = if !right.is_empty() && masked(l).ends_with(&masked(right)) { &l[..l.len() - right.len()] }
                    else { trail.find(l).map_or(l, |m| &l[..m.start()]) };
            l.trim().to_string()
        };
        let joined: String = std::iter::once(&lines[i][left.len()..]).chain(lines[i + 1..].iter().copied()).collect();
        let line = if i + 1 == lines.len() { Some(strip(&joined)) } else {
            let keys = keys.trim();
            let by_keys = !keys.is_empty() && strip(&joined).ends_with(keys) && keys.ends_with(lines[lines.len() - 1].trim_end());
            let by_width = width > 0 && lines[i..lines.len() - 1].iter().all(|l| l.chars().count() == width);
            (by_keys || by_width).then(|| strip(&joined))
        };
        if let Some(line) = line {
            if (!need_text || !line.is_empty()) && first.as_deref().is_none_or(|f| line.starts_with(f)) { return Some((left, line)); }
        }
    }
    let keys = keys.trim();
    if exact && !keys.is_empty() {
        for (left, split) in &candidates {
            let ml = masked(left);
            let at = (0..lines.len()).rev().find(|&i| masked_lines[i].starts_with(&ml) && {
                let rest = lines[i][left.len()..].trim_end();
                let rest = if *split { trail.find(rest).map_or(rest, |m| &rest[..m.start()]) } else { rest };
                rest.trim() == keys
            });
            if at.is_some() { return Some((left.clone(), keys.to_string())); }
        }
    }
    None
}

/// [`line_after`]'s line alone, for a line with no first keys to check.
pub fn typed_after(text: &str, prompt: &Prompt, keys: &str) -> Option<String> { line_after(text, prompt, keys, None, &[], false, &[], false).map(|(_, l)| l) }

/// `text` with the command after each of `prompts` (the window's prompts lines were taken after)
/// passed through [`redact_command`]: the terminal's own echo of a line, however it was typed
/// (pasted, completed, recalled from history). Lines the terminal wrapped onto are not joined here
/// (output can be as wide as a command); what a command hid is hidden wherever it shows by
/// [`redact_words`].
pub fn redact_at_prompts(text: &str, prompts: &[String]) -> String {
    text.split('\n').map(|line| {
        let ml = masked(line);
        // longest first, the first that hides something (round 5: a prompt read with a paste in it
        // is longer than the real one and leaves nothing after it)
        let mut ps: Vec<&String> = prompts.iter().filter(|p| !p.trim().is_empty() && ml.starts_with(&masked(p))).collect();
        ps.sort_by_key(|p| std::cmp::Reverse(p.len()));
        for p in ps {
            let rest = line[p.len()..].trim();
            let red = redact_command(rest);
            // spacing alone is no change (it would split a word the word pass hides whole)
            if red != rest.split_whitespace().collect::<Vec<_>>().join(" ") { return format!("{p} {red}"); }
        }
        line.to_string()
    }).collect::<Vec<_>>().join("\n")
}

/// The last `n` non-empty lines of `text`, joined by spaces (a line and what it wrapped onto),
/// as they are and with a right-hand prompt cut off each.
fn last_lines(text: &str, n: usize) -> String {
    static TRAIL: std::sync::OnceLock<regex::Regex> = std::sync::OnceLock::new();
    let trail = TRAIL.get_or_init(|| regex::Regex::new(r"\s{3,}\S(?:\S| {1,2}\S)*\s*$").unwrap());
    let v: Vec<&str> = text.lines().filter(|l| !l.trim().is_empty()).collect();
    let v = &v[v.len().saturating_sub(n)..];
    let cut: Vec<&str> = v.iter().map(|l| trail.find(l).map_or(*l, |m| &l[..m.start()])).collect();
    format!("{}\n{}", v.join(" "), cut.join(" "))
}

/// [`secret_words`] of `text` read from each word on: a prompt read with a command already in it
/// (round 5) has the command somewhere after its start.
pub fn secret_words_anywhere(text: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for line in text.lines() {
        let w: Vec<&str> = line.split_whitespace().collect();
        for i in 0..w.len().min(40) { for x in secret_words(&w[i..].join(" ")) { if !out.contains(&x) { out.push(x); } } }
    }
    out
}

/// The words of `cmd` that [`redact_command`] hides: six characters or more, or four or more
/// where the command form puts `<redacted>` in that word's place (a value after `-p`,
/// `--password`, `-u user:`; round 7). Shorter ones are not hidden in the output: a two-letter
/// password would take every two-letter run with it.
pub fn secret_words(cmd: &str) -> Vec<String> {
    let red = redact_command(cmd);
    let kept: std::collections::BTreeSet<&str> = red.split_whitespace().collect();
    let (words, reds): (Vec<&str>, Vec<&str>) = (cmd.split_whitespace().collect(), red.split_whitespace().collect());
    let aligned = words.len() == reds.len();
    words.iter().enumerate().filter(|(i, w)| !kept.contains(*w) && {
        let n = w.chars().count();
        n >= 6 || (n >= 4 && aligned && reds[*i].contains(crate::REDACTED))
    }).map(|(_, w)| w.to_string()).collect()
}

/// `text` with each of `words` hidden wherever it shows, and where a wrap split one across two
/// lines (a line ending with its first four or more characters, or starting with its last four or
/// more): after a resize the terminal rewraps old lines anywhere.
///
/// Cost (round 4, item 6): the words are at most 100 per window and each is cut to 200 characters;
/// a line is checked for a split only when its first or last four characters occur in some word
/// (a set built once per call), so a read costs one pass per word plus one set lookup per line.
///
/// A word under six characters (round 8: `yaml`, `test`, `1234`, which the command form hides in
/// `kubectl get secret -o yaml`, `openssl passwd -1 test`, `echo token: 1234`) is hidden only where
/// it stands as a token: between boundaries (start or end, whitespace, quotes, `=`, `:`, `,`), or
/// right after a secret flag (`-pq1w2`). A wrap that splits one is found as its two halves on
/// either side of the line break. So `latest` and `values.yaml` keep their text.
pub fn redact_words(text: &str, words: &[String]) -> String {
    let mut words: Vec<&str> = words.iter().map(|w| w.as_str()).filter(|w| !w.is_empty() && w.chars().count() <= 200).collect();
    // A sampled response can first be observed as a prefix. Hide its longest
    // known form first so replacing that prefix cannot expose the suffix.
    words.sort_by_key(|word| std::cmp::Reverse(word.len()));
    words.dedup();
    let (long, short): (Vec<&str>, Vec<&str>) = words.iter().partition(|w| w.chars().count() >= 6);
    let mut out = text.to_string();
    for w in &long { if out.contains(w) { out = out.replace(w, crate::REDACTED); } }
    for w in &short { out = replace_token(&out, w); }
    let words = long;
    if words.is_empty() { return out; }
    let mut grams: std::collections::HashSet<&str> = std::collections::HashSet::new();
    for w in &words {
        let ix: Vec<usize> = w.char_indices().map(|(i, _)| i).chain([w.len()]).collect();
        for j in 0..ix.len().saturating_sub(4) { grams.insert(&w[ix[j]..ix[j + 4]]); }
    }
    let four = |l: &str, from_end: bool| -> Option<String> {
        let c: Vec<char> = l.chars().collect();
        if c.len() < 4 { return None; }
        Some(if from_end { c[c.len() - 4..].iter().collect() } else { c[..4].iter().collect() })
    };
    out.split('\n').map(|line| {
        let hit = |e: bool| four(line, e).is_some_and(|g| grams.contains(g.as_str()));
        if !hit(false) && !hit(true) { return line.to_string(); }
        let mut line = line.to_string();
        for w in &words {
            for (k, _) in w.char_indices().skip(1) {
                let (head, tail) = (&w[..k], &w[k..]);
                if tail.chars().count() >= 4 && line.starts_with(tail) { line = format!("{}{}", crate::REDACTED, &line[tail.len()..]); break; }
                if head.chars().count() >= 4 && line.ends_with(head) { line = format!("{}{}", &line[..line.len() - head.len()], crate::REDACTED); break; }
            }
        }
        line
    }).collect::<Vec<_>>().join("\n")
}

/// A character that ends a token for [`replace_token`].
/// Punctuation and non-ASCII symbols (a prompt's `➜`, `❯`, `λ`) count; `.` does not, so
/// `values.yaml` keeps its `yaml`.
fn token_edge(c: Option<char>) -> bool {
    c.is_none_or(|c| c.is_whitespace()
        || matches!(c, '"' | '\'' | '=' | ':' | ',' | '`' | ';' | '(' | ')' | '[' | ']' | '{' | '}' | '@' | '/' | '|' | '&' | '<' | '>')
        || (!c.is_ascii() && !c.is_alphanumeric()))
}

/// Flags whose value can be attached (`-pq1w2`, `--password=q1w2`).
const SECRET_FLAG_PREFIXES: &[&str] = &["-p", "-a", "-P", "-N", "-k", "--password=", "--pass=", "--passwd=", "--token=", "--secret=", "--api-key="];

/// `text` with `w` replaced where it stands as a token (see [`redact_words`]), and where a line break
/// splits it into halves that do.
fn replace_token(text: &str, w: &str) -> String {
    if w.is_empty() { return text.to_string(); }
    let before_ok = |t: &str, i: usize| {
        let head = &t[..i];
        token_edge(head.chars().next_back())
            || SECRET_FLAG_PREFIXES.iter().any(|f| head.ends_with(f) && token_edge(head[..head.len() - f.len()].chars().next_back()))
    };
    let after_ok = |t: &str, j: usize| token_edge(t[j..].chars().next());
    // whole
    let mut out = String::with_capacity(text.len());
    let mut last = 0;
    for (i, _) in text.match_indices(w) {
        if i < last { continue; }
        if before_ok(text, i) && after_ok(text, i + w.len()) {
            out.push_str(&text[last..i]);
            out.push_str(crate::REDACTED);
            last = i + w.len();
        }
    }
    out.push_str(&text[last..]);
    // split by a wrap: `q1` at a line's end, `w2` at the next one's start
    for (k, _) in w.char_indices().skip(1) {
        let joined = format!("{}\n{}", &w[..k], &w[k..]);
        let t = out.clone();
        let mut next = String::with_capacity(t.len());
        let mut last = 0;
        for (i, _) in t.match_indices(&joined) {
            if i < last { continue; }
            if before_ok(&t, i) && after_ok(&t, i + joined.len()) {
                next.push_str(&t[last..i]);
                next.push_str(&format!("{}\n{}", crate::REDACTED, crate::REDACTED));
                last = i + joined.len();
            }
        }
        next.push_str(&t[last..]);
        out = next;
    }
    out
}

/// A line being entered in a terminal window.
#[derive(Clone, Debug, Default)]
struct TermLine {
    /// The prompt it is typed after: the end of the last read before its first key.
    prompt: Option<Prompt>,
    /// The keys typed (Tab and edits ignored): only to hide the line in the output when its prompt
    /// is lost.
    keys: String,
    /// The prompt is known to be the bare prompt: read after a line ended in this session, or the
    /// same as one an earlier line was taken after.
    trusted: bool,
    /// The keys typed before any other key (an arrow, Tab, Backspace, a shortcut): the line on
    /// screen has to start with them. `None` when the line started with another key.
    lead: Option<String>,
    /// Another key came after the lead.
    lead_done: bool,
    started: Ts,
    last: Ts,
}

struct Ctx { app: AppInfo, window: Option<WindowInfo>, decision: Result<(), Reason> }

/// The recorder state machine. One per recording session; the host calls
/// [`Recorder::start`], feeds [`Observation`]s and [`Recorder::tick`], and
/// [`Recorder::stop`]s it.
pub struct Recorder {
    pub policy: ObservationSettings,
    own_pid: i64,
    scrub: Option<Scrub>,
    cur: Option<Ctx>,
    buf: Option<TextBuf>,
    /// Last full terminal text per window, to diff the next read against.
    terminal: BTreeMap<u32, String>,
    /// When each terminal window's text was last read: a line whose last key came after the read
    /// taken at its Return is not known from the screen, and is not kept.
    terminal_read_at: BTreeMap<u32, Ts>,
    /// Lines typed into each terminal whose command form hides something (re-review 1): the
    /// terminal's text has them replaced by that form, whatever the prompt looks like.
    typed_lines: BTreeMap<u32, std::collections::VecDeque<String>>,
    /// The prompt each terminal window's last read ended with, while no line was being typed.
    term_prompt: BTreeMap<u32, Prompt>,
    /// The line being typed in each terminal window.
    term_line: BTreeMap<u32, TermLine>,
    /// The prompts lines were taken after, per window (the last 20), for the output.
    term_prompts: BTreeMap<u32, std::collections::VecDeque<String>>,
    /// What the commands typed in each window hid ([`secret_words`]), the last 100.
    term_secrets: BTreeMap<u32, std::collections::VecDeque<String>>,
    /// The command taken at the last Return in each window, its continuation lines joined, for a
    /// line typed at a continuation prompt (`> `, `dquote> `, `heredoc> `) to be redacted with it.
    term_cont: BTreeMap<u32, String>,
    /// The prompt parts of the last few reads taken while no line was typed, per window (round 5).
    term_seen: BTreeMap<u32, std::collections::VecDeque<(String, bool)>>,
    /// Windows where a line ended (Return, Control-C, Control-D) in this session: a prompt read
    /// after that is the bare prompt. Before it, the first read may be of a line already half typed
    /// (round 6: the session started with `mysql -u root -p` on screen).
    term_confirmed: std::collections::BTreeSet<u32>,
    /// Each terminal window's text as read at its last Return.
    term_at_return: BTreeMap<u32, String>,
    /// Keys were typed in a terminal window with no id since its last Return (no state is kept there).
    term_zero_typed: bool,
    last_selection: Option<String>,
    locked: bool,
    running: bool,
    /// A secure-input marker was already written for the field in focus.
    secure_noted: bool,
    /// The last suppression marker written (kind, reason, when), so a run of keystrokes in a
    /// blocked window is one marker rather than one per key (which would keep their count and timing).
    last_marker: Option<(String, Reason, Ts)>,
}

/// A repeat of the same suppressed kind for the same reason within this many seconds of the
/// previous marker adds no marker.
pub const MARKER_COALESCE_SECS: Ts = 10;

impl Recorder {
    pub fn new(policy: ObservationSettings, own_pid: i64) -> Recorder {
        Recorder { policy, own_pid, scrub: None, cur: None, buf: None, terminal: BTreeMap::new(), terminal_read_at: BTreeMap::new(), typed_lines: BTreeMap::new(), term_prompt: BTreeMap::new(), term_line: BTreeMap::new(), term_prompts: BTreeMap::new(), term_secrets: BTreeMap::new(), term_cont: BTreeMap::new(), term_seen: BTreeMap::new(), term_confirmed: Default::default(), term_at_return: BTreeMap::new(), term_zero_typed: false, last_selection: None, locked: false, running: false, secure_noted: false, last_marker: None }
    }
    /// Keep a typed terminal line whose command form hides something, the last 20 per window.
    fn remember_typed(&mut self, w: u32, line: String) {
        if line.chars().count() < 4 || redact_command(&line) == line { return; }
        let words = self.term_secrets.entry(w).or_default();
        for s in secret_words(&line) { words.retain(|x| *x != s); words.push_back(s); }
        while words.len() > 100 { words.pop_front(); }
        let q = self.typed_lines.entry(w).or_default();
        q.retain(|l| *l != line);
        q.push_back(line);
        while q.len() > 20 { q.pop_front(); }
    }
    pub fn with_scrub(mut self, scrub: Scrub) -> Recorder { self.scrub = Some(scrub); self }
    pub fn running(&self) -> bool { self.running }
    pub fn locked(&self) -> bool { self.locked }

    /// Every free-text field passes through here: Codex's tokens, the crate's passes,
    /// then the host's scrub.
    pub fn clean(&self, text: &str) -> String {
        let t = redact::inline(text);
        match &self.scrub { Some(f) => f(&t), None => t }
    }

    /// Whether the app in front, and its window, may be observed, and if not why.
    pub fn decide(&self, app: &AppInfo, window: Option<&WindowInfo>) -> Result<(), Reason> {
        if self.locked { return Err(Reason::ScreenLocked); }
        if app.pid == self.own_pid { return Err(Reason::OwnApp); }
        if app.path.as_deref().is_some_and(|p| p.starts_with("/System/Library/") || p.starts_with("/usr/") || p.starts_with("/Library/Apple/")) {
            return Err(Reason::SystemApp);
        }
        if app.background_only { return Err(Reason::BackgroundApp); }
        let app_rule = app.bundle_id.as_deref().and_then(|b| self.policy.app_rule(b));
        if app_rule == Some(false) { return Err(Reason::AppRule); }
        let url = window.and_then(|w| w.url.as_deref());
        let host = url.and_then(url_host);
        if url.is_none() && is_browser(app.bundle_id.as_deref())
            && (!self.policy.url_blocklist.is_empty()
                // a never-record site rule fails closed the same way (fresh review, F9)
                || self.policy.rules.iter().any(|r| r.scope == RuleScope::Url && !r.observe)
                || (app_rule != Some(true) && (self.policy.rules.iter().any(|r| r.scope == RuleScope::Url) || self.policy.default_behavior == DefaultBehavior::DontObserve))) {
            // The blocklist fails closed even for an app marked "always record": that rule is about
            // the app, and a blocked site inside it is still blocked.
            return Err(Reason::UrlUnknown);
        }
        let frames = window.map(|w| w.frame_urls.as_slice()).unwrap_or(&[]);
        if url.into_iter().chain(frames.iter().map(String::as_str)).any(|u| self.policy.url_blocked(u)) { return Err(Reason::UrlBlocked); }
        if window.is_some_and(|w| w.private) && !self.policy.observe_private_browsing { return Err(Reason::PrivateBrowsing); }
        let url_rule = host.as_deref().and_then(|h| self.policy.url_rule(h));
        if url_rule == Some(false) || frames.iter().any(|u| url_host(u).and_then(|h| self.policy.url_rule(&h)) == Some(false)) { return Err(Reason::UrlRule); }
        if self.policy.default_behavior == DefaultBehavior::DontObserve && app_rule != Some(true) && url_rule != Some(true) {
            return Err(Reason::DefaultDontObserve);
        }
        Ok(())
    }

    fn base(&self, kind: &str, now: Ts) -> EventRecord {
        let mut r = EventRecord { ts: iso(now), kind: kind.into(), ..Default::default() };
        if let Some(c) = &self.cur {
            r.bundle_id = c.app.bundle_id.clone();
            r.app_name = Some(c.app.name.clone()).filter(|n| !n.is_empty());
            r.pid = Some(c.app.pid);
            if let Some(w) = &c.window {
                r.window_id = Some(w.id);
                if is_browser(c.app.bundle_id.as_deref()) { r.url = w.url.as_deref().map(|u| self.clean_url(u)); }
            }
        }
        r
    }

    /// The URL as recorded: scheme, host and path, with the query and fragment dropped (they carry
    /// search terms, tokens and ids) and id-like path segments replaced by `:id`. This is stricter
    /// than what is known of Codex, whose handling of queries is not visible in the binary.
    fn clean_url(&self, url: &str) -> String { bound(&redact::inline(&strip_path_ids(strip_query(url))), 500) }

    fn target_fields(&self, r: &mut EventRecord, t: Option<&Target>) {
        if let Some(t) = t {
            r.role = t.subrole.clone().filter(|s| s == "AXSecureTextField").or_else(|| t.role.clone());
            r.label = t.label.as_deref().map(|l| bound(&self.clean(l), MAX_LABEL_CHARS)).filter(|l| !l.is_empty());
        }
    }

    fn suppressed(&self, kind: &str, now: Ts, reason: Reason) -> Output { Output::Suppressed(Suppressed { ts: iso(now), kind: kind.into(), reason }) }

    /// Open a session.
    pub fn start(&mut self, now: Ts) -> Vec<Output> {
        self.running = true;
        let mut r = EventRecord { ts: iso(now), kind: SESSION_STARTED.into(), ..Default::default() };
        r.reason = Some("started".into());
        vec![Output::Record(r)]
    }

    /// Flush what is pending and close the session.
    pub fn stop(&mut self, now: Ts, reason: &str) -> Vec<Output> {
        if !self.running { return vec![]; }
        let mut out = self.flush(now);
        let mut r = EventRecord { ts: iso(now), kind: SESSION_ENDED.into(), ..Default::default() };
        r.reason = Some(reason.into());
        out.push(Output::Record(r));
        self.running = false;
        self.cur = None;
        self.terminal.clear();
        out
    }

    /// The policy changed: the next focus observation is decided afresh.
    pub fn repolicy(&mut self) { self.cur = None; }

    /// Forget everything not yet written (a clear-history ran).
    pub fn discard_pending(&mut self) {
        self.buf = None; self.terminal.clear(); self.terminal_read_at.clear(); self.typed_lines.clear();
        self.term_prompt.clear(); self.term_line.clear(); self.term_prompts.clear(); self.term_secrets.clear(); self.term_cont.clear(); self.term_seen.clear(); self.term_confirmed.clear(); self.term_at_return.clear(); self.last_selection = None;
    }

    /// Input attribution was lost while the same approved window stayed active.
    /// The next command must not inherit an incomplete sequence of key events.
    /// Terminal snapshots have their own identity and policy checks: keep their
    /// diff baseline and learned redactions so an input gap neither loses new
    /// output nor reveals an earlier secret echoed after its prompt scrolls away.
    /// Policy changes and history deletion still use `discard_pending`.
    pub fn discard_input_attribution(&mut self) {
        self.buf = None;
        self.term_prompt.clear();
        self.term_line.clear();
        self.term_prompts.clear();
        self.term_cont.clear();
        self.term_seen.clear();
        self.term_confirmed.clear();
        self.term_at_return.clear();
        self.term_zero_typed = false;
        self.last_selection = None;
    }

    /// The terminal window in front, when one is.
    fn terminal_window(&self) -> Option<u32> {
        self.cur.as_ref().filter(|c| is_terminal(c.app.bundle_id.as_deref())).map(|c| c.window.as_ref().map(|w| w.id).unwrap_or(0))
    }

    /// The line in terminal window `w`, started by a key now if none is: its prompt is the one
    /// the last read ended with.
    /// Prompt parts known from before the line in window `w`: those lines were taken after, and
    /// those of reads taken while no line was typed.
    fn earlier_prompts(&self, w: u32) -> Vec<(String, bool)> {
        let mut v: Vec<(String, bool)> = self.term_prompts.get(&w).map(|q| q.iter().map(|p| (p.clone(), false)).collect()).unwrap_or_default();
        v.extend(self.term_seen.get(&w).map(|q| q.iter().cloned().collect::<Vec<_>>()).unwrap_or_default());
        v
    }

    fn term_line_at(&mut self, w: u32, now: Ts) -> &mut TermLine {
        let prompt = self.term_prompt.get(&w).cloned();
        let known = |p: &Prompt| self.term_prompts.get(&w).is_some_and(|q| q.iter().any(|k| {
            let k = masked(k);
            masked(&p.left) == k || p.split.as_ref().is_some_and(|(l, _)| masked(l) == k)
        }));
        let trusted = self.term_confirmed.contains(&w) || prompt.as_ref().is_some_and(known);
        let l = self.term_line.entry(w).or_insert_with(|| TermLine { prompt, keys: String::new(), trusted, lead: None, lead_done: false, started: now, last: now });
        l.last = now;
        l
    }

    /// Return in terminal window `w` (fresh review 3, item 1): the line is what the terminal shows
    /// after the prompt read before its first key, in the read taken as Return was pressed, not the
    /// keys (a paste, a completion, an edit with the arrows, a line from history are all on screen
    /// and none are in the keys); see [`line_after`]. At a continuation prompt it is joined to the
    /// command before it (round 4, item 3: `mysql -u root \` then `> -phunter2`), and the joined
    /// command is what is written. Not kept, and marked, when that read is older than the last key,
    /// no prompt was read before the line, the prompt asks for a secret, the line is not on screen
    /// after it, or a continuation line has no command before it that was taken.
    fn terminal_return(&mut self, w: u32, target: &Target, now: Ts) -> (Vec<Output>, Option<String>) {
        let line = self.term_line.remove(&w);
        let idle = self.term_prompt.remove(&w);
        let before = self.term_cont.remove(&w);
        // a line ended here: prompts read from now on are bare ones
        let was_confirmed = !self.term_confirmed.insert(w);
        if line.as_ref().is_some_and(|l| !l.trusted) {
            // round 6: the prompt may have held part of the line; what the line on screen hid is
            // hidden from now on, and the line is not kept
            let tail = self.terminal.get(&w).map(|t| last_lines(t, 3)).unwrap_or_default();
            let q = self.term_secrets.entry(w).or_default();
            for x in secret_words_anywhere(&tail) { q.retain(|y| *y != x); q.push_back(x); }
            while q.len() > 100 { q.pop_front(); }
            if let Some(l) = &line { let k = l.keys.trim().to_string(); self.remember_typed(w, k); }
            return (vec![self.suppressed(TEXT_INPUT, now, Reason::TerminalSecret)], None);
        }
        let _ = was_confirmed;
        if let Some(l) = &line { let k = l.keys.trim().to_string(); self.remember_typed(w, k); }
        let prompt = match &line { Some(l) => l.prompt.clone(), None => idle };
        let fresh = self.terminal_read_at.get(&w).is_some_and(|at| line.as_ref().is_none_or(|l| *at >= l.last));
        let (keys, lead) = line.as_ref().map_or(("", None), |l| (l.keys.as_str(), l.lead.as_deref()));
        let earlier = self.earlier_prompts(w);
        let known: Vec<String> = self.term_prompts.get(&w).map(|q| q.iter().cloned().collect()).unwrap_or_default();
        let exact = line.as_ref().is_some_and(|l| !l.lead_done && l.lead.is_some());
        // a terminal whose text is what it was at the last Return although a line was entered since
        // is not being read afresh (the Mac run: iTerm2's text stopped changing after one line)
        let now_text = self.terminal.get(&w).cloned();
        let stale = line.is_some() && now_text.is_some() && self.term_at_return.get(&w) == now_text.as_ref();
        if let Some(t) = now_text { self.term_at_return.insert(w, t); }
        let got = match (fresh && !stale, &prompt, self.terminal.get(&w)) {
            (true, Some(p), Some(text)) if !p.asks_secret() => line_after(text, p, keys, lead, &earlier, line.is_some(), &known, exact),
            _ => None,
        };
        // A delayed first-key read can already contain the echoed response.
        // Recovering a shorter prompt must not turn that response into a shell
        // command. Judge the recovered prompt with its original context too.
        let got = got.filter(|(left, _)| {
            !Prompt { left: left.clone(), split: None,
                above: prompt.as_ref().and_then(|p| p.above.clone()),
                width: prompt.as_ref().map_or(0, |p| p.width) }.asks_secret()
        });
        // a continuation line goes with the command before it, or not at all
        let got = match got {
            Some((left, cmd)) if is_continuation_prompt(&left) => before.map(|b| (left, format!("{} {}", b.trim_end().trim_end_matches('\\').trim_end(), cmd))),
            g => g,
        };
        match got {
            Some((left, cmd)) if !cmd.is_empty() => {
                // and by what the whole line on screen hides, should the line have been cut wrong
                let mut words = self.terminal.get(&w).map(|t| secret_words_anywhere(&last_lines(t, 3))).unwrap_or_default();
                words.extend(self.term_secrets.get(&w).into_iter().flatten().cloned());
                let red = redact_words(&redact_command(&self.clean(&cmd)), &words);
                // and in the output from now on
                let q = self.term_secrets.entry(w).or_default();
                for x in words { q.retain(|y| *y != x); q.push_back(x); }
                while q.len() > 100 { q.pop_front(); }
                self.remember_typed(w, cmd.clone());
                self.term_cont.insert(w, cmd);
                if !left.trim().is_empty() && !is_continuation_prompt(&left) {
                    let q = self.term_prompts.entry(w).or_default();
                    q.retain(|x| *x != left);
                    q.push_back(left);
                    while q.len() > 20 { q.pop_front(); }
                }
                let mut r = self.base(TEXT_INPUT, line.as_ref().map_or(now, |l| now.max(l.started)));
                self.target_fields(&mut r, Some(target));
                r.text = Some(red.clone());
                (vec![Output::Record(r)], Some(red))
            }
            Some(_) if line.is_none() => (vec![], None),
            _ if line.is_some() => {
                // the prompt as read may hold the line (a paste echoed before the read): what it hid
                // stays hidden in the output
                if let Some(p) = &prompt {
                    let q = self.term_secrets.entry(w).or_default();
                    for x in secret_words_anywhere(&p.left) { q.retain(|y| *y != x); q.push_back(x); }
                    while q.len() > 100 { q.pop_front(); }
                }
                (vec![self.suppressed(TEXT_INPUT, now, Reason::TerminalSecret)], None)
            }
            _ => (vec![], None),
        }
    }

    /// Flush typed text idle for [`IDLE_FLUSH_SECS`]. A terminal's keys never enter the buffer: its
    /// lines are taken from the screen at Return.
    pub fn tick(&mut self, now: Ts) -> Vec<Output> {
        let terminal = self.terminal_window();
        match &self.buf {
            Some(b) if terminal.is_some_and(|w| b.window.unwrap_or(0) == w) => vec![],
            Some(b) if now - b.last >= IDLE_FLUSH_SECS => self.flush(now),
            _ => vec![],
        }
    }

    /// Wall-clock idle time is meaningful only after earlier input is drained.
    pub fn tick_when_drained(&mut self, now: Ts, input_pending: bool) -> Vec<Output> {
        if input_pending { vec![] } else { self.tick(now) }
    }

    /// The pending typed text as one `keyboard.text_input` record.
    pub fn flush(&mut self, _now: Ts) -> Vec<Output> {
        let Some(b) = self.buf.take() else { return vec![] };
        if b.text.trim().is_empty() || self.observing().is_err() { return vec![]; }
        // Stamped when written (it may close later than the window it started in had its summary).
        // (A terminal's keys never get here: its lines are taken at Return, see `terminal_return`.)
        let mut r = self.base(TEXT_INPUT, _now.max(b.started));
        r.observed_at = Some(iso(b.last));
        self.target_fields(&mut r, Some(&b.target));
        // A typing burst may begin after an earlier credential phrase in the
        // same field. Only complete field observations can resolve shape
        // ambiguity; raw fragments keep the conservative host scrub.
        r.text = Some(self.clean(&b.text));
        vec![Output::Record(r)]
    }

    fn observing(&self) -> Result<(), Reason> {
        match &self.cur { Some(c) => c.decision, None => Err(if self.locked { Reason::ScreenLocked } else { Reason::NoFocus }) }
    }

    pub fn observe(&mut self, obs: Observation, now: Ts) -> Vec<Output> {
        let out = self.observe_raw(obs, now);
        self.coalesce(out, now)
    }

    fn coalesce(&mut self, out: Vec<Output>, now: Ts) -> Vec<Output> {
        let mut kept = Vec::with_capacity(out.len());
        for o in out {
            match &o {
                Output::Suppressed(m) => {
                    let at = now;
                    let repeat = self.last_marker.as_ref().is_some_and(|(k, r, t)| *k == m.kind && *r == m.reason && at - *t < MARKER_COALESCE_SECS);
                    // A repeat extends the run, so a steady stream of keys stays one marker.
                    self.last_marker = Some((m.kind.clone(), m.reason, at));
                    if !repeat { kept.push(o); }
                }
                Output::Record(_) => { self.last_marker = None; kept.push(o); }
            }
        }
        kept
    }

    fn observe_raw(&mut self, obs: Observation, now: Ts) -> Vec<Output> {
        if !self.running { return vec![]; }
        match obs {
            Observation::Locked(locked) => {
                if locked == self.locked { return vec![]; }
                if locked {
                    let mut out = self.flush(now);
                    self.discard_pending();
                    let mut r = EventRecord { ts: iso(now), kind: SESSION_ENDED.into(), ..Default::default() };
                    r.reason = Some("screen_locked".into());
                    out.push(Output::Record(r));
                    self.locked = true;
                    self.cur = None;
                    out
                } else {
                    self.locked = false;
                    let mut r = EventRecord { ts: iso(now), kind: SESSION_STARTED.into(), ..Default::default() };
                    r.reason = Some("screen_unlocked".into());
                    vec![Output::Record(r)]
                }
            }
            _ if self.locked => vec![self.suppressed(kind_of(&obs), now, Reason::ScreenLocked)],
            Observation::Focus { app, window } => self.focus(app, window, now),
            Observation::Click { button, count, ctrl, target } => {
                let mut out = self.flush(now);
                let kind = if button == Button::Right || (button == Button::Left && ctrl) { MOUSE_CONTEXT_MENU } else { MOUSE_CLICK };
                if let Err(why) = self.observing() { out.push(self.suppressed(kind, now, why)); return out; }
                let mut r = self.base(kind, now);
                self.target_fields(&mut r, target.as_ref());
                if kind == MOUSE_CLICK {
                    r.button = Some(match button { Button::Left => "left", Button::Right => "right", Button::Other => "other" }.into());
                    r.click_count = Some(count.max(1));
                }
                out.push(Output::Record(r));
                out
            }
            Observation::Drag { target } => {
                let mut out = self.flush(now);
                if let Err(why) = self.observing() { out.push(self.suppressed(MOUSE_DRAG, now, why)); return out; }
                let mut r = self.base(MOUSE_DRAG, now);
                self.target_fields(&mut r, target.as_ref());
                out.push(Output::Record(r));
                out
            }
            Observation::Key { chars, key, mods, target, secure_input, key_name } => self.key(chars, key, mods, target, secure_input, key_name, now),
            Observation::TerminalValue { window_id, value } => {
                if self.observing().is_err() || !self.cur.as_ref().is_some_and(|c| is_terminal(c.app.bundle_id.as_deref())) { return vec![]; }
                // a window with no id would share its state with every other such window (round 6)
                if window_id == 0 { return vec![]; }
                // Platforms that omit printable key events still see echoed
                // prompt responses. Remember these before the initial baseline
                // too, so a later bare echo cannot bypass prompt redaction.
                let secrets = self.term_secrets.entry(window_id).or_default();
                map_prompted_secrets(&value, |response| {
                    let response = response.trim();
                    if !response.is_empty() && response.chars().count() <= 200
                        && !response.contains(crate::REDACTED) {
                        secrets.retain(|previous| previous != response);
                        secrets.push_back(response.to_owned());
                        while secrets.len() > 100 {secrets.pop_front();}
                    }
                });
                self.terminal_read_at.insert(window_id, now);
                // read with no line being typed: the prompt the next line is typed after
                if !self.term_line.contains_key(&window_id) {
                    let p = Prompt::of(&value);
                    let q = self.term_seen.entry(window_id).or_default();
                    for part in std::iter::once((p.left.clone(), false)).chain(p.split.as_ref().map(|(l, _)| (l.clone(), true))) {
                        q.retain(|x| *x != part);
                        q.push_back(part);
                    }
                    while q.len() > 8 { q.pop_front(); }
                    self.term_prompt.insert(window_id, p);
                }
                let prev = self.terminal.insert(window_id, value.clone());
                let Some(prev) = prev else { return vec![] };
                // The lines typed here, the one being typed included, replaced by their command form
                // wherever they show, whatever the prompt in front of them (re-review 1: an
                // Oh-My-Zsh `➜` prompt has no prompt character the line pass knows).
                let mut lines: Vec<String> = self.typed_lines.get(&window_id).map(|q| q.iter().cloned().collect()).unwrap_or_default();
                if let Some(l) = self.term_line.get(&window_id) { lines.push(l.keys.trim().to_string()); }
                // the command after each prompt a line was taken after, and the current line's
                // (fresh review 3, item 1)
                let mut prompts: Vec<String> = self.term_prompts.get(&window_id).map(|q| q.iter().cloned().collect()).unwrap_or_default();
                // the current line's prompt as found in this read (a key or two may have been echoed into
                // the read before typing), else as read
                let earlier = self.earlier_prompts(window_id);
                let known: Vec<String> = self.term_prompts.get(&window_id).map(|q| q.iter().cloned().collect()).unwrap_or_default();
                let found = self.term_line.get(&window_id).and_then(|l| l.prompt.as_ref().and_then(|p| line_after(&value, p, &l.keys, l.lead.as_deref(), &earlier, true, &known, !l.lead_done && l.lead.is_some())));
                if let Some((left, _)) = &found { if !is_continuation_prompt(left) { prompts.push(left.clone()); } }
                else if let Some(p) = self.term_line.get(&window_id).and_then(|l| l.prompt.as_ref()) { prompts.push(p.split.as_ref().map_or(p.left.clone(), |(l, _)| l.clone())); }
                // and the earlier prompts the line's prompt, or this read's last line, extends: what
                // follows them may be input echoed before the line was known (round 5)
                if let Some(p) = self.term_line.get(&window_id).and_then(|l| l.prompt.as_ref()) { prompts.extend(p.extended(&earlier).into_iter().map(|(e, _)| e)); }
                let here = Prompt::of(&value);
                let ext = here.extended(&earlier);
                let here_words = if ext.is_empty() { vec![] } else { secret_words_anywhere(&here.left) };
                prompts.extend(ext.into_iter().map(|(e, _)| e));

                // Hidden before the diff, not after: when the prompt was already on screen at the last
                // read, the new text starts mid-line with the secret alone (found on the Mac run:
                // "Password: " read at one Return, "TERMCANARY9" diffed at the next).
                // commands on prompt lines too (F1), before the diff for the same reason
                // (after the prompt pass: a `<redacted>` at a line's end would read as a prompt)
                let mut words: Vec<String> = self.term_secrets.get(&window_id).map(|q| q.iter().cloned().collect()).unwrap_or_default();
                words.extend(here_words);
                if let Some(l) = self.term_line.get(&window_id) {
                    let before = self.term_cont.get(&window_id).map(|b| format!("{} ", b.trim_end().trim_end_matches('\\').trim_end())).unwrap_or_default();
                    words.extend(secret_words(l.keys.trim()));
                    if let Some(p) = &l.prompt { words.extend(secret_words_anywhere(&p.left)); }
                    if !l.trusted { words.extend(secret_words_anywhere(&last_lines(&value, 3))); }
                    words.extend(secret_words(&format!("{before}{}", l.keys.trim())));
                    // the line as this read shows it (pasted, wrapped through a secret, or a
                    // continuation of the command before it)
                    if let Some((left, line)) = &found {
                        words.extend(secret_words(line));
                        words.extend(secret_words(&format!("{before}{line}")));
                        // a continuation line with no command taken before it is not known: hidden whole
                        if before.is_empty() && is_continuation_prompt(left) && line.chars().count() >= 4 { words.push(line.clone()); }
                    }
                }
                // Redact complete prompt commands before remembered words or partial
                // typed lines can remove their executable. Otherwise a delayed read
                // can lose `smbclient` first and leave its `-U user%password` intact.
                let pass = |t: &str| redact_prompt_lines(&redact_words(&redact_typed(&redact_at_prompts(&redact_words(&redact_prompt_lines(&hide_prompted_secrets(t)), &words), &prompts), &lines), &words));
                let (hp, hv) = (pass(&prev), pass(&value));
                let tail = terminal_tail(&hp, &hv);
                if tail.trim().is_empty() { return vec![]; }
                let start = tail.char_indices().rev().nth(MAX_TERMINAL_CHARS.saturating_sub(1)).map(|(i, _)| i).unwrap_or(0);
                let mut r = self.base(TERMINAL_VALUE_CHANGED, now);
                // The terminal pass above already hides a prompted response,
                // including a response on the next line. Preserve its line
                // boundaries through the host's prose scrub: flattening the
                // whole diff lets an old Password: marker consume subsequent
                // unrelated commands and output as one secret clause.
                r.text = Some(tail[start..].split('\n').map(|line| self.clean(line))
                    .collect::<Vec<_>>().join("\n"));
                vec![Output::Record(r)]
            }
            Observation::Selection { text, target } => {
                if let Err(why) = self.observing() { return vec![self.suppressed(SELECTION_CHANGED, now, why)]; }
                // A selection read in another app than the one the recorder judged (focus moved
                // between the gesture and the read) is not kept under this one's rules (fresh review, F7).
                if target.as_ref().and_then(|t| t.pid).is_some_and(|p| self.cur.as_ref().is_some_and(|c| c.app.pid != p)) { return vec![]; }
                if target.as_ref().is_some_and(Target::is_secure) { return vec![self.suppressed(SELECTION_CHANGED, now, Reason::SecureInput)]; }
                let text = text.trim().to_string();
                if text.is_empty() || self.last_selection.as_deref() == Some(text.as_str()) { return vec![]; }
                self.last_selection = Some(text.clone());
                let mut r = self.base(SELECTION_CHANGED, now);
                self.target_fields(&mut r, target.as_ref());
                r.text = Some(bound(&self.clean(&text), MAX_SELECTION_CHARS));
                vec![Output::Record(r)]
            }
        }
    }

    fn focus(&mut self, app: AppInfo, window: Option<WindowInfo>, now: Ts) -> Vec<Output> {
        // The frames count (found on the Mac run, round 5: focus moving into a blocked site's frame
        // on the same page was taken for no change and never judged).
        let same = self.cur.as_ref().is_some_and(|c| c.app.pid == app.pid && c.window.as_ref().map(|w| (w.id, &w.title, &w.url, w.private, &w.frame_urls))
            == window.as_ref().map(|w| (w.id, &w.title, &w.url, w.private, &w.frame_urls)));
        if same { return vec![]; }
        let mut out = self.flush(now);
        self.last_selection = None;
        self.secure_noted = false;
        let decision = self.decide(&app, window.as_ref());
        self.cur = Some(Ctx { app, window, decision });
        match decision {
            Err(why) => out.push(self.suppressed(WINDOW_CHANGED, now, why)),
            Ok(()) => {
                let mut r = self.base(WINDOW_CHANGED, now);
                let c = self.cur.as_ref().unwrap();
                if let Some(w) = &c.window {
                    r.title = w.title.as_deref().map(|t| bound(&self.clean(t), MAX_TITLE_CHARS)).filter(|t| !t.is_empty());
                    r.url = w.url.as_deref().map(|u| self.clean_url(u));
                    r.path = w.document.as_deref().filter(|p| p.starts_with('/')).map(str::to_string);
                }
                out.push(Output::Record(r));
            }
        }
        out
    }

    #[allow(clippy::too_many_arguments)]
    fn key(&mut self, chars: String, key: Key, mods: Mods, target: Option<Target>, secure_input: bool, key_name: String, now: Ts) -> Vec<Output> {
        let target = target.unwrap_or_default();
        if let Err(why) = self.observing() {
            self.buf = None;
            // what a terminal shows when observing resumes is not known to be a bare prompt
            self.term_confirmed.clear();
            return vec![self.suppressed(TEXT_INPUT, now, why)];
        }
        // Secure keyboard entry, or a password field: nothing of it is buffered or written,
        // and what was typed before into another field is flushed first.
        let tw = self.terminal_window();
        if secure_input || target.is_secure() {
            // what was typed on a terminal line is not known any more
            if let Some(w) = tw { self.term_line.remove(&w); self.term_prompt.remove(&w); }
            let mut out = match &self.buf { Some(b) if !b.target.same_field(&target) => self.flush(now), _ => { self.buf = None; vec![] } };
            if !self.secure_noted { self.secure_noted = true; out.push(self.suppressed(TEXT_INPUT, now, Reason::SecureInput)); }
            return out;
        }
        self.secure_noted = false;
        let terminal = self.cur.as_ref().is_some_and(|c| is_terminal(c.app.bundle_id.as_deref()));
        if mods.cmd || (mods.ctrl && key != Key::Return) {
            let mut out = self.flush(now);
            if let Some(w) = tw {
                // Control-C and Control-D end the line; a paste or an edit is part of it
                let name = if key_name.is_empty() { chars.to_lowercase() } else { key_name.to_lowercase() };
                if w == 0 { self.term_zero_typed = true; }
                else if mods.ctrl && !mods.cmd && matches!(name.as_str(), "c" | "d" | "\u{3}" | "\u{4}") {
                    self.term_line.remove(&w); self.term_prompt.remove(&w); self.term_cont.remove(&w); self.term_confirmed.insert(w);
                }
                else { self.term_line_at(w, now).lead_done = true; }
            }
            let mut r = self.base(SHORTCUT, now);
            let mut keys = Vec::new();
            if mods.ctrl { keys.push("ctrl") }
            if mods.alt { keys.push("alt") }
            if mods.shift { keys.push("shift") }
            if mods.cmd { keys.push("cmd") }
            let name = if key_name.is_empty() { chars.to_lowercase() } else { key_name.to_lowercase() };
            r.keys = Some(format!("{}+{}", keys.join("+"), bound(&name, 20)));
            self.target_fields(&mut r, Some(&target));
            out.push(Output::Record(r));
            return out;
        }
        match key {
            Key::Return => {
                let mut out = self.flush(now);
                let mut command = None;
                if tw == Some(0) {
                    if std::mem::take(&mut self.term_zero_typed) { out.push(self.suppressed(TEXT_INPUT, now, Reason::TerminalSecret)); }
                } else if let Some(w) = tw {
                    let (recs, cmd) = self.terminal_return(w, &target, now);
                    out.extend(recs);
                    command = cmd;
                }
                let mut r = self.base(SUBMIT, now);
                self.target_fields(&mut r, Some(&target));
                if terminal { r.command = command; }
                out.push(Output::Record(r));
                out
            }
            // A terminal line is taken from the screen at Return; the keys only start it (and are
            // kept to hide it in the output should its prompt be lost). Tab completes, it does not
            // end the line.
            _ if tw == Some(0) => { self.term_zero_typed = true; self.flush(now) }
            _ if tw.is_some() => {
                let w = tw.unwrap_or(0);
                let out = self.flush(now);
                let l = self.term_line_at(w, now);
                match key {
                    Key::Char => {
                        let printable: String = chars.chars().filter(|c| !c.is_control()).collect();
                        if l.keys.chars().count() < MAX_BUFFER_CHARS { l.keys.push_str(&printable); }
                        if !l.lead_done { l.lead.get_or_insert_with(String::new).push_str(&printable); }
                    }
                    Key::Backspace => { l.keys.pop(); l.lead_done = true; }
                    _ => { l.lead_done = true; }
                }
                out
            }
            Key::Tab | Key::Escape => self.flush(now),
            Key::Backspace => { if let Some(b) = &mut self.buf { b.text.pop(); b.last = now; } vec![] }
            Key::Arrow | Key::ForwardDelete | Key::Other => vec![],
            Key::Char => {
                let printable: String = chars.chars().filter(|c| !c.is_control()).collect();
                if printable.is_empty() { return vec![]; }
                let window = self.cur.as_ref().and_then(|c| c.window.as_ref().map(|w| w.id));
                let mut out = match &self.buf { Some(b) if !b.target.same_field(&target) || b.window != window => self.flush(now), _ => vec![] };
                let b = self.buf.get_or_insert_with(|| TextBuf { text: String::new(), target: target.clone(), window, last: now, started: now });
                b.text.push_str(&printable);
                b.last = now;
                if b.text.chars().count() >= MAX_BUFFER_CHARS { out.extend(self.flush(now)); }
                out
            }
        }
    }
}

/// `url` with each path segment that looks like an id or a token replaced by `:id`: a UUID or hex
/// string, a run of 4 or more digits, a segment of 6 or more letters and digits at least a third
/// digits, or any 24 or more letters and digits (`/reset/abc123`, `/s/AbCdEf0123456789`).
pub fn strip_path_ids(url: &str) -> String {
    let (head, path) = match url.find("://").map(|i| i + 3).and_then(|i| url[i..].find('/').map(|j| i + j)) {
        Some(k) => (&url[..k], &url[k..]),
        None => return url.to_string(),
    };
    // Round 3, L7: digit-heavy or hex-like only, so "/wiki/Python3" and "/v2" stay.
    let id_like = |seg: &str| {
        let n = seg.chars().count();
        let digits = seg.chars().filter(|c| c.is_ascii_digit()).count();
        let plain = seg.chars().all(|c| c.is_ascii_alphanumeric() || c == '_');
        let hex = n >= 8 && digits > 0 && seg.chars().all(|c| c.is_ascii_hexdigit() || c == '-');
        let uuid = n == 36 && seg.chars().filter(|c| *c == '-').count() == 4 && hex;
        uuid || hex || (digits >= 4 && digits == n)
            || (plain && n >= 6 && digits >= 3 && digits * 3 >= n)
            || (plain && n >= 24)
    };
    let path: Vec<String> = path.split('/').map(|seg| if id_like(seg) { ":id".to_string() } else { seg.to_string() }).collect();
    format!("{head}{}", path.join("/"))
}

/// `url` without its `?query` and `#fragment`.
pub fn strip_query(url: &str) -> &str { url.find(['?', '#']).map(|i| &url[..i]).unwrap_or(url) }

/// The last line of a terminal's text asks for a secret: a password, passphrase, passcode, PIN,
/// one-time code or token prompt, as sudo, ssh, gpg, su, read -s and most login tools print them.
pub fn terminal_asks_secret(text: &str) -> bool {
    static RE: std::sync::OnceLock<regex::Regex> = std::sync::OnceLock::new();
    // A keyword, whole words only (round 3, L3: "compass> " and "Bypass cache? [y/N]:" matched),
    // then at most "for <who>" or "(<note>)" before the prompt's colon, question mark or angle
    // bracket at the end of the line ("Token count: 1200" is not a prompt).
    let re = RE.get_or_init(|| regex::Regex::new(concat!(
        r"(?i)((?:\b(?:pass(?:word|phrase|code|wort|ord)?|kennwort|mot de passe|contrase(?:ñ|n)a|senha|parola(?: d'ordine)?|wachtwoord|has(?:ł|l)o|",
        r"lösenord|losenord|adgangskode|salasana|heslo|jelszó|şifre|sifre|пароль|pin|one[- ]time (?:code|password)|otp|verification code|security code|",
        r"auth(?:entication)? code|2fa|mfa|token|secret|api[ _-]?key|access key|private key|recovery key|unlock code)\b",
        r"|密码|密碼|口令|パスワード|暗証番号|비밀번호))",
        // round 7, L1: a comma or the like right after the keyword ("Password, again:")
        r"[,;.]?",
        // round 6, L1: or up to three more words ("Enter passphrase now:", "Enter PIN code again:")
        r"(?:\s+(?:for|für|pour|para|per|voor|för|for)\s+[^\n]{0,60}|\s*\([^)\n]{0,40}\)|((?:[ \t]+[^\s:?>：＞#❯»]{1,30}){1,3}))?\s*[:?>：＞#❯»]\s*$")).unwrap());
    let last = text.trim_end_matches(['\n', '\r']).rsplit('\n').next().unwrap_or("").trim_end_matches('\r');
    // A shell prompt followed by typed text ("~/dev % read -p 'PIN: '"): only what follows it is
    // the question. A prompt character at the end of the line is the question's own ("Enter
    // OTP> ", "PIN❯ "), so the whole line is judged (round 4, M1: cutting there let every prompt
    // ending in '>' through).
    let mut cut = None;
    for (i, c) in last.char_indices() {
        let after = &last[i + c.len_utf8()..];
        if matches!(c, '$' | '#' | '%' | '>' | '❯' | 'λ' | '»') && after.starts_with(char::is_whitespace) && !after.trim().is_empty() { cut = Some(i + c.len_utf8()); }
    }
    let judged = cut.map_or(last, |i| &last[i..]);
    // The secret word decides, unless it is part of a path ("~/dev/token> " names a folder).
    let mut at = 0;
    while let Some(m) = re.captures_at(judged, at) {
        let k = m.get(1).or_else(|| m.get(0)).unwrap();
        let before = judged[..k.start()].chars().next_back();
        // words after the keyword that make it a label, not a question ("Token count: ")
        // (round 7, L1: unless the line ends in a secret noun, "Password reset code:")
        let words: Vec<String> = m.get(2).map(|w| w.as_str().split_whitespace().map(|x| x.to_lowercase()).collect()).unwrap_or_default();
        let label = words.first().is_some_and(|w| LABEL_WORDS.contains(&w.as_str()))
            && !words.last().is_some_and(|w| SECRET_NOUNS.contains(&w.trim_matches(|c: char| !c.is_alphanumeric())));
        if !matches!(before, Some('/' | '~' | '.' | '-')) && !label { return true; }
        at = k.end();
        if at >= judged.len() { break; }
    }
    false
}

/// A word right after a secret keyword that makes the line a label or a report rather than a
/// question for the secret itself ("Token count:", "Password policy:", "PIN changed:").
const LABEL_WORDS: &[&str] = &["count", "counts", "usage", "used", "limit", "limits", "length", "type", "expires", "expired", "expiry", "cache",
    "file", "path", "manager", "policy", "strength", "hint", "changed", "updated", "saved", "ok", "status", "list", "budget", "total", "rate", "id", "name"];

/// The last word of a prompt that asks for a secret whatever came before it ("Token usage code:").
const SECRET_NOUNS: &[&str] = &["code", "pin", "token", "key", "password", "passphrase", "passcode", "otp", "secret"];

/// Terminal output with what follows a secret prompt on the same line hidden: a program that asks
/// for a password without turning echo off shows it on screen ("Password: hunter2").
///
/// Round 5, L1: a prompt on a line of its own (`print("Enter your API key:"); input()`) hides the
/// next non-empty line whole, and a prompt the terminal wrapped onto two lines is read joined.
pub fn hide_prompted_secrets(text: &str) -> String {
    map_prompted_secrets(text, |_| {})
}
fn map_prompted_secrets(text: &str, mut hidden: impl FnMut(&str)) -> String {
    let mut out = Vec::new();
    let mut prev: Option<&str> = None;
    let mut asked = false;
    for line in text.split('\n') {
        if line.trim().is_empty() { out.push(line.to_string()); continue; }
        if asked {
            hidden(line);
            out.push(crate::REDACTED.to_string());
            asked = false;
            prev = None;
            continue;
        }
        let asks = |i: usize| terminal_asks_secret(&line[..i]) || prev.is_some_and(|p| wrapped_asks_secret(p, &line[..i]));
        let cut = line.char_indices().filter(|(_, c)| matches!(c, ':' | '?' | '>' | '：' | '＞' | '#' | '❯' | '»')).map(|(i, c)| i + c.len_utf8())
            .find(|&i| asks(i) && !line[i..].trim().is_empty());
        match cut {
            Some(i) => {hidden(&line[i..]);out.push(format!("{} {}", &line[..i], crate::REDACTED));},
            None => { asked = asks(line.len()); out.push(line.to_string()); }
        }
        prev = Some(line);
    }
    out.join("\n")
}

/// What precedes the typed text on its line is no question of its own: nothing, or a prompt
/// character or two (`> `, `>>> `, `: `).
fn prompt_only(before: &str) -> bool {
    let t = before.trim();
    t.chars().count() <= 4 && t.chars().all(|c| matches!(c, '>' | '$' | '#' | '%' | ':' | '?' | '❯' | '»' | 'λ' | '.' | '＞' | '：'))
}

/// `line` continues `prev`, wrapped by the terminal mid-word or at a space, into a secret prompt
/// ("Enter your pass" / "word: ").
fn wrapped_asks_secret(prev: &str, line: &str) -> bool {
    if line.starts_with(char::is_whitespace) || line.trim().is_empty() { return false; }
    let p = prev.trim_end();
    terminal_asks_secret(&format!("{p}{line}")) || terminal_asks_secret(&format!("{p} {line}"))
}

/// `text` with each of `lines` (commands typed into the terminal) replaced by its
/// [`redact_command`] form, where that form differs.
pub fn redact_typed(text: &str, lines: &[String]) -> String {
    let mut out = text.to_string();
    for l in lines.iter().map(|l| l.trim()).filter(|l| l.chars().count() >= 4) {
        let r = redact_command(l);
        if r != l { out = out.replace(l, &r); }
    }
    out
}

/// Terminal output with the command on each shell-prompt line passed through [`redact_command`]
/// (fresh review, F1): the echoed `$ mysql -u root -phunter2` would otherwise keep the password.
/// A prompt line is one with a prompt character followed by a space and a command; the output
/// lines are left as they are.
pub fn redact_prompt_lines(text: &str) -> String {
    text.split('\n').map(|line| {
        // A backstop for lines not typed here (a pasted command, a script echoing one): only where
        // what follows the prompt character starts like a command, so `100% done, see ...` is left
        // alone (re-review 6).
        let cut = line.char_indices().filter(|&(i, c)| {
            let after = &line[i + c.len_utf8()..];
            matches!(c, '$' | '#' | '%' | '>' | '❯' | 'λ' | '»') && after.starts_with(' ') && starts_like_command(after.trim_start())
        }).map(|(i, c)| i + c.len_utf8()).next();
        match cut {
            Some(i) => { let red = redact_command(line[i..].trim_start()); format!("{} {red}", &line[..i]) }
            None => line.to_string(),
        }
    }).collect::<Vec<_>>().join("\n")
}

/// The first word of `rest` could be a program: a name like `mysql` or `git-lfs`, or a path.
fn starts_like_command(rest: &str) -> bool {
    let Some(w) = rest.split_whitespace().next() else { return false };
    let path = (w.starts_with('/') || w.starts_with("./") || w.starts_with("~/")) && !w.contains(',');
    let name = w.chars().next().is_some_and(|c| c.is_ascii_alphabetic() || c == '_')
        && w.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '.' | '+'));
    path || name
}

/// What a terminal's text gained since `prev`, anchored on whole lines: the reader keeps only
/// the last 20,000 characters, so once scrollback is longer every read starts somewhere else and
/// a plain common-prefix diff would treat the whole window as new. The lines before `prev`'s last
/// line are found again in `value`, and what follows them is the new part (the prompt line the
/// command was typed on, and the output after it).
pub fn terminal_tail<'a>(prev: &str, value: &'a str) -> &'a str {
    let prev_lines: Vec<&str> = prev.split('\n').collect();
    if prev_lines.len() >= 3 {
        let take = (prev_lines.len() - 1).min(4);
        let anchor = &prev_lines[prev_lines.len() - 1 - take..prev_lines.len() - 1];
        if anchor.iter().any(|l| !l.trim().is_empty()) {
            let new_lines: Vec<&str> = value.split('\n').collect();
            if let Some(p) = (0..=new_lines.len().saturating_sub(take)).rev().find(|&p| new_lines.get(p..p + take) == Some(anchor)) {
                let skip: usize = new_lines[..p + take].iter().map(|l| l.len() + 1).sum();
                return &value[skip.min(value.len())..];
            }
        }
    }
    let common = prev.char_indices().zip(value.char_indices()).take_while(|((_, a), (_, b))| a == b).last()
        .map(|((i, c), _)| i + c.len_utf8()).unwrap_or(0);
    &value[common.min(value.len())..]
}

fn kind_of(obs: &Observation) -> &'static str {
    match obs {
        Observation::Focus { .. } => WINDOW_CHANGED,
        Observation::Click { button: Button::Right, .. } => MOUSE_CONTEXT_MENU,
        Observation::Click { .. } => MOUSE_CLICK,
        Observation::Drag { .. } => MOUSE_DRAG,
        Observation::Key { .. } => TEXT_INPUT,
        Observation::TerminalValue { .. } => TERMINAL_VALUE_CHANGED,
        Observation::Selection { .. } => SELECTION_CHANGED,
        Observation::Locked(_) => SESSION_ENDED,
    }
}
