use computer_history::{
    browser::{document_url, same_document},
    capture::{Content, Recorder, Source},
    policy::{Behavior, Context, Rule, Website},
    store::{CaptureMode, Commit, Store},
    Result,
};
use std::sync::atomic::AtomicBool;
struct Browser {
    resolves: usize,
    reads: usize,
    private: bool,
    swapped: bool,
    changed: bool,
    after_resolve: Option<Box<dyn FnOnce()>>,
}
impl Browser {
    fn new() -> Self {
        Self {
            resolves: 0,
            reads: 0,
            private: false,
            swapped: false,
            changed: false,
            after_resolve: None,
        }
    }
}
impl Source for Browser {
    fn identify(&mut self) -> Result<Option<Context>> {
        Ok(Some(Context {
            application: "com.google.Chrome".into(),
            process_id: 10,
            window_id: 20,
            website: Website::Unknown,
        }))
    }
    fn resolve(&mut self, c: &Context, enabled: bool) -> Result<Option<Context>> {
        self.resolves += 1;
        if !enabled {
            return Ok(Some(c.clone()));
        }
        if self.private {
            return Ok(None);
        }
        let mut c = c.clone();
        c.website = Website::Url("https://public.example/path?secret=NEVER_PERSIST_URL".into());
        if self.swapped {
            c.window_id += 1
        }
        if let Some(f) = self.after_resolve.take() {
            f()
        }
        Ok(Some(c))
    }
    fn read(&mut self, _: &Context, _: CaptureMode) -> Result<Option<Content>> {
        self.reads += 1;
        Ok(Some(Content {
            role: "AXTextField".into(),
            label: Some("Public draft".into()),
            value: Some("Public fixture content".into()),
            identifier: None,
        }))
    }
    fn verify(&mut self, _: &Context) -> Result<bool> {
        Ok(!self.changed)
    }
}
fn setup() -> (tempfile::TempDir, Store) {
    let d = tempfile::tempdir().unwrap();
    let s = Store::open(d.path().join("history")).unwrap();
    let mut settings = s.state().unwrap().settings;
    settings.observation.default_application_behavior = Behavior::Observe;
    settings.observation.default_url_behavior = Behavior::Observe;
    s.update(settings, None).unwrap();
    s.configure(true, 7, true).unwrap();
    let state = s.state().unwrap();
    s.capture_mode(
        CaptureMode::Values,
        true,
        (&state.generation, state.revision),
    )
    .unwrap();
    (d, s)
}
fn enabled(s: &Store) {
    let state = s.state().unwrap();
    s.browser_capture(true, true, (&state.generation, state.revision))
        .unwrap();
}
fn bytes(s: &Store) -> String {
    s.status().unwrap()["recent_activity_files"]
        .as_array()
        .unwrap()
        .iter()
        .map(|p| std::fs::read_to_string(p.as_str().unwrap()).unwrap())
        .collect()
}
fn run(s: &Store, b: &mut Browser) -> Option<Commit> {
    Recorder::default()
        .sample(s, b, &AtomicBool::new(false), |_, _, _| Ok(()))
        .unwrap()
}
#[test]
fn browser_consent_is_separate_does_not_enable_history_and_invalidates_tickets() {
    let (_d, s) = setup();
    let state = s.state().unwrap();
    assert!(!state.browser_capture);
    assert!(s
        .browser_capture(true, false, (&state.generation, state.revision))
        .is_err());
    let ticket = s.observation().unwrap().unwrap();
    enabled(&s);
    assert!(s
        .bind(
            ticket,
            Context {
                application: "fixture".into(),
                process_id: 1,
                window_id: 2,
                website: Website::NotWeb
            }
        )
        .unwrap()
        .is_none());
    assert!(s
        .browser_capture(false, false, (&state.generation, state.revision))
        .is_err());
    let state = s.state().unwrap();
    s.browser_capture(false, false, (&state.generation, state.revision))
        .unwrap();
    s.configure(false, 7, false).unwrap();
    enabled(&s);
    assert!(!s.state().unwrap().enabled);
}
#[test]
fn default_consent_and_private_browser_do_not_read_content() {
    let (_d, s) = setup();
    let mut b = Browser::new();
    assert_eq!(run(&s, &mut b), None);
    assert_eq!(b.reads, 0);
    enabled(&s);
    b.private = true;
    assert_eq!(run(&s, &mut b), None);
    assert_eq!(b.reads, 0);
    assert!(bytes(&s).is_empty());
}
#[test]
fn blocked_apps_do_not_query_browser_and_blocked_domains_do_not_read_controls() {
    for app in [true, false] {
        let (_d, s) = setup();
        enabled(&s);
        let mut settings = s.state().unwrap().settings;
        settings.observation.blocklist.push(if app {
            Rule::App {
                bundle_id: "com.google.Chrome".into(),
            }
        } else {
            Rule::Url {
                url_domain: "public.example".into(),
            }
        });
        s.update(settings, None).unwrap();
        let mut b = Browser::new();
        assert_eq!(run(&s, &mut b), None);
        assert_eq!(b.resolves, usize::from(!app));
        assert_eq!(b.reads, 0);
        assert!(bytes(&s).is_empty());
    }
}
#[test]
fn public_browser_records_domain_without_url_credentials_path_or_query() {
    let (_d, s) = setup();
    enabled(&s);
    let mut b = Browser::new();
    assert_eq!(run(&s, &mut b), Some(Commit::Written));
    let data = bytes(&s);
    assert!(data.contains("public.example"));
    assert!(!data.contains("NEVER_PERSIST_URL"));
    assert!(!data.contains("/path"));
    assert!(data.contains("Public fixture content"));
}
#[test]
fn changing_native_window_or_page_never_commits() {
    for swap in [true, false] {
        let (_d, s) = setup();
        enabled(&s);
        let mut b = Browser::new();
        b.swapped = swap;
        b.changed = !swap;
        assert_ne!(run(&s, &mut b), Some(Commit::Written));
        assert!(bytes(&s).is_empty());
    }
}
#[test]
fn disabling_browser_capture_during_resolution_blocks_optional_content() {
    let (_d, s) = setup();
    enabled(&s);
    let other = s.clone();
    let mut b = Browser::new();
    b.after_resolve = Some(Box::new(move || {
        let state = other.state().unwrap();
        other
            .browser_capture(false, false, (&state.generation, state.revision))
            .unwrap();
    }));
    assert_eq!(run(&s, &mut b), None);
    assert_eq!(b.reads, 0);
    assert!(bytes(&s).is_empty());
}
#[test]
fn page_identity_never_uses_a_link_destination_or_different_frame() {
    assert!(same_document(
        "https://EXAMPLE.com:443/path?q=1#part",
        "https://example.com/path?q=1#part"
    ));
    for other in [
        "https://blocked.example/path?q=1#part",
        "https://example.com/frame",
        "https://example.com/path?q=2#part",
        "https://example.com/path?q=1#other",
    ] {
        assert!(!same_document(other, "https://example.com/path?q=1#part"));
    }
    for invalid in [
        "about:blank",
        "chrome://newtab",
        "javascript:alert(1)",
        "file:///tmp/private",
    ] {
        assert!(document_url(invalid).is_none())
    }
}
#[test]
fn model_controls_cannot_expand_browser_scope_or_erase_host_consent() {
    use computer_history::mcp::Session;
    let (_d, s) = setup();
    let mut api = Session::new(s.clone());
    let mut settings = api
        .call("computer_history_get_settings", serde_json::json!({}))
        .unwrap();
    settings["browserCapture"] = serde_json::json!(true);
    assert!(api
        .call("computer_history_update_settings", settings)
        .is_err());
    assert!(!s.state().unwrap().browser_capture);
    assert!(api
        .call("computer_history_browser_capture", serde_json::json!({}))
        .is_err());
    enabled(&s);
    let settings = api
        .call("computer_history_get_settings", serde_json::json!({}))
        .unwrap();
    api.call("computer_history_update_settings", settings)
        .unwrap();
    assert!(s.state().unwrap().browser_capture);
}
