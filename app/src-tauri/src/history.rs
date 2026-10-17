//! Host lifecycle for the shared Computer History store and approved capture scopes.
use computer_history::{
    policy::{Behavior, Context, Settings, Website},
    store::{Commit, Store},
};
use serde_json::{json, Value};
use std::{
    path::PathBuf,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    time::{Duration, Instant},
};

pub struct Service {
    pub store: Option<Store>,
    error: Option<String>,
    stop: Arc<AtomicBool>,
    #[cfg(target_os = "linux")]
    helper_notice: std::sync::Mutex<Option<String>>,
}
#[derive(serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DeletionReview {
    pub generation: String,
    pub revision: u64,
    pub events: Vec<computer_history::store::EventIdentity>,
    pub summaries: Vec<crate::memory::history_content::SummaryKey>,
    pub derivatives: Vec<crate::memory::history_deletion::ReviewedDerivative>,
}
impl DeletionReview {
    pub fn activity(&self) -> computer_history::store::DeletePreview {
        computer_history::store::DeletePreview { generation:self.generation.clone(),revision:self.revision,events:self.events.clone() }
    }
}
pub(crate) fn clean_window_snapshot(recorder: &skysight::stream::Recorder, text: &str) -> String {
    // The native formatter escapes embedded newlines in names and values.
    // Each physical line is a separate control, not a continuation of a
    // preceding password label's value.
    crate::memory::clean_control_snapshot(text, &|line| recorder.clean(line))
}
impl Service {
    pub fn open(root: PathBuf, previous_opt_in: bool, previous_pause: bool) -> Self {
        let fresh = !root.join("settings.json").exists();
        let result = (|| -> Result<Store, String> {
            let store = Store::open(root).map_err(|e| e.to_string())?;
            if fresh {
                let mut settings = Settings::default();
                settings.observation.default_application_behavior = Behavior::Observe;
                settings.observation.default_url_behavior = Behavior::Observe;
                store.update(settings, None).map_err(|e| e.to_string())?;
                if previous_opt_in {
                    store.configure(true, 2, true).map_err(|e| e.to_string())?;
                }
                if previous_pause {
                    store.pause(true).map_err(|e| e.to_string())?;
                }
            }
            Ok(store)
        })();
        match result {
            Ok(store) => Self {
                store: Some(store),
                error: None,
                stop: Arc::new(AtomicBool::new(false)),
                #[cfg(target_os = "linux")]
                helper_notice: std::sync::Mutex::new(None),
            },
            Err(error) => Self {
                store: None,
                error: Some(error),
                stop: Arc::new(AtomicBool::new(false)),
                #[cfg(target_os = "linux")]
                helper_notice: std::sync::Mutex::new(None),
            },
        }
    }
    fn required(&self) -> Result<&Store, String> {
        self.store.as_ref().ok_or_else(|| {
            self.error
                .clone()
                .unwrap_or("History is unavailable".into())
        })
    }
    pub fn status(&self) -> Result<Value, String> {
        let store = self.required()?;
        let mut status = store.status().map_err(|e| e.to_string())?;
        status["semantic_supported"] = json!(cfg!(any(target_os = "macos", target_os = "windows", target_os = "linux")));
        status["interaction_supported"] = json!(cfg!(any(target_os = "macos", target_os = "windows", target_os = "linux")));
        status["browser_capture_supported"] = json!(cfg!(any(target_os = "macos", target_os = "linux")));
        #[cfg(target_os = "linux")]
        {
            status["browser_permission_required"] = json!(false);
            status["browser_capture_notice"] = json!("Linux supports English Chrome and Chromium deb/rpm builds with recognized normal profiles and available accessibility. Firefox, Snap, Flatpak, other profiles and other languages are excluded. Use app identity or focused-control scopes; native interaction capture excludes browsers.");
        }
        #[cfg(target_os = "linux")]
        {
            status["helper_notice"] = json!(self.helper_notice.lock().ok().and_then(|s| s.clone())
                .or_else(|| crate::linux_helpers::notice().map(str::to_owned)));
            status["helper_setup_supported"] = json!(crate::linux_helpers::notice().is_some());
        }
        Ok(status)
    }
    pub fn setup_desktop(&self, approved: bool, generation: &str, revision: u64) -> Result<Value, String> {
        #[cfg(target_os = "linux")]
        {
            if crate::linux_helpers::notice().is_none() {
                return Err("Desktop capture setup is not needed for this session.".into());
            }
            return self.setup_desktop_with(approved, generation, revision, crate::linux_helpers::enable);
        }
        #[cfg(not(target_os = "linux"))]
        {
            let _ = (approved, generation, revision);
            Err("Desktop capture setup is unavailable on this platform.".into())
        }
    }
    #[cfg(target_os = "linux")]
    fn setup_desktop_with(&self, approved: bool, generation: &str, revision: u64,
        setup: impl FnOnce() -> Result<String, String>) -> Result<Value, String> {
        let state = self.required()?.state().map_err(|e| e.to_string())?;
        if !approved { return Err("Desktop setup needs an explicit request.".into()); }
        if !state.enabled { return Err("Enable History before setting up desktop capture.".into()); }
        if state.generation != generation || state.revision != revision {
            return Err("History changed; reload before setting up desktop capture.".into());
        }
        // This explicit host action only sets up the desktop integration. It
        // does not change capture scope, pause, rules or content-sharing consent.
        let message = setup()?;
        if let Ok(mut notice) = self.helper_notice.lock() { *notice = Some(message); }
        self.status()
    }
    pub fn capture_mode(&self, mode: computer_history::store::CaptureMode, approved: bool, generation: &str, revision: u64) -> Result<Value, String> {
        if !cfg!(any(target_os = "macos", target_os = "windows", target_os = "linux")) && mode != computer_history::store::CaptureMode::ApplicationMetadata {
            return Err("Native content capture is currently available on macOS, Windows and Linux".into());
        }
        #[cfg(target_os = "macos")]
        if mode == computer_history::store::CaptureMode::InteractionEvents && approved {
            // Only the explicit host scope-approval action can request this OS
            // grant. A model changing observation rules cannot trigger it.
            let state=self.required()?.state().map_err(|e|e.to_string())?;
            if state.generation!=generation || state.revision!=revision {return Err("History changed; reload the scope before approving it".into());}
            if skysight::macos::permissions().input_monitoring != Some(true) {
                let _=skysight::macos::request_input_monitoring();
            }
        }
        self.required()?.capture_mode(mode, approved, (generation, revision)).map_err(|e| e.to_string())?;
        #[cfg(target_os = "linux")]
        if approved && mode != computer_history::store::CaptureMode::ApplicationMetadata {
            // The store validates the consent revision first. Polling, startup,
            // model rules and rejected/stale approval never install helpers.
            let message = crate::linux_helpers::enable().unwrap_or_else(|e| format!("Helper setup: {e}"));
            if let Ok(mut notice) = self.helper_notice.lock() { *notice = Some(message); }
        }
        self.status()
    }
    pub fn configure(&self, enabled: bool, days: u32, approved: bool) -> Result<Value, String> {
        self.required()?
            .configure(enabled, days, approved)
            .map_err(|e| e.to_string())?;
        self.status()
    }
    pub fn browser_capture(&self,enabled:bool,approved:bool,generation:&str,revision:u64)->Result<Value,String>{
        if enabled&&!cfg!(any(target_os="macos",target_os="linux")){return Err("Browser history capture is currently available on macOS and Linux only".into())}
        self.required()?.browser_capture(enabled,approved,(generation,revision)).map_err(|e|e.to_string())?;self.status()
    }
    pub fn pause(&self, paused: bool) -> Result<Value, String> {
        self.required()?.pause(paused).map_err(|e| e.to_string())?;
        self.status()
    }
    pub fn update(
        &self,
        settings: Settings,
        generation: &str,
        revision: u64,
    ) -> Result<Value, String> {
        self.required()?
            .update(settings, Some((generation, revision)))
            .map_err(|e| e.to_string())?;
        self.status()
    }
    pub fn clear(&self) -> Result<usize, String> {
        self.required()?.clear().map_err(|e| e.to_string())
    }
    pub fn retention(&self, days: u32, generation: &str, revision: u64) -> Result<Value, String> {
        self.required()?
            .retention(days, (generation, revision))
            .map_err(|e| e.to_string())?;
        self.status()
    }
    pub fn start(&self, sink: impl Fn(&Context, u64, &Value) -> Result<(), String> + Send + 'static) {
        #[cfg(any(target_os = "macos", target_os = "windows", target_os = "linux"))]
        {
            let Some(store) = self.store.clone() else { return };
            let stop = self.stop.clone();
            let _ = std::thread::Builder::new().name("computer-history".into()).spawn(move || {
                let mode = if cfg!(any(target_os = "macos", target_os = "linux")) { "native_focus" } else { "application_metadata" };
                let Ok(owner) = store.attach_collector(mode) else { return };
                #[cfg(target_os = "macos")]
                let mut source = computer_history::macos::MacSource::default();
                #[cfg(target_os = "windows")]
                let mut source = computer_history::windows::WindowsSource::default();
                #[cfg(target_os = "linux")]
                let mut source = computer_history::linux::LinuxSource::default();
                let mut recorder = computer_history::capture::Recorder::default();
                #[cfg(target_os = "macos")]
                let mut interactions = crate::history_events::Collector::default();
                #[cfg(target_os = "windows")]
                let mut interactions = crate::history_events_windows::Collector::default();
                #[cfg(target_os = "linux")]
                let mut interactions = crate::history_events_linux::Collector::default();
                // Retention is a store obligation, including while interaction
                // capture is paused, disabled or unable to observe a window.
                let mut maintenance = Instant::now() - Duration::from_secs(2);
                while !stop.load(Ordering::SeqCst) {
                    if maintenance.elapsed() >= Duration::from_secs(2) {
                        maintenance = Instant::now();
                        if let Err(error) = store.prune(computer_history::now_ms()) {
                            maintenance = Instant::now() - Duration::from_secs(2);
                            interactions.stop();
                            let _ = owner.heartbeat(mode, Some(&error.to_string()));
                            std::thread::sleep(Duration::from_millis(100));
                            continue;
                        }
                    }
                    #[cfg(any(target_os = "macos", target_os = "windows", target_os = "linux"))]
                    {
                        if store.state().is_ok_and(|s|s.capture_mode==computer_history::store::CaptureMode::InteractionEvents) {
                            recorder.reset();
                            let result=interactions.sample(&store,&sink);
                            if result.is_err(){interactions.stop();}
                            let error=result.err().map(|e|e.to_string());
                            #[cfg(target_os = "linux")]
                            let error=error.or_else(||interactions.refusal().map(str::to_owned));
                            let _=owner.heartbeat("native_interaction_events",error.as_deref());
                            // Back off empty ticks so an idle session polls near the
                            // heartbeat rate instead of a fixed 50 Hz; stays fast while interacting.
                            #[cfg(target_os = "macos")]
                            std::thread::sleep(interactions.backoff());
                            #[cfg(not(target_os = "macos"))]
                            std::thread::sleep(Duration::from_millis(20));
                            continue;
                        }
                        interactions.stop();
                    }
                    let result = recorder.sample(&store, &mut source, &stop,
                        |context,timestamp,payload| sink(context,timestamp,payload).map_err(|e|computer_history::Error::new("computerHistoryMemoryWrite",e)));
                    if result.is_err() { recorder.reset(); }
                    let error = result.err().map(|e|e.to_string());
                    #[cfg(target_os = "linux")]
                    let error=error.or_else(||source.refusal().map(str::to_owned));
                    let _ = owner.heartbeat(mode,error.as_deref());
                    let deadline=Instant::now()+Duration::from_secs(2);
                    while Instant::now()<deadline && !stop.load(Ordering::SeqCst){std::thread::sleep(Duration::from_millis(10));}
                }
            });
        }
        #[cfg(not(any(target_os = "macos", target_os = "windows", target_os = "linux")))]
        let _ = self.start_with_sink(crate::cua::history_windows, Duration::from_secs(2), move |context,timestamp|sink(context,timestamp,&json!({})));
    }
    #[cfg(test)]
    fn start_with(
        &self,
        source: impl Fn() -> Result<Value, String> + Send + 'static,
        interval: Duration,
    ) -> Option<std::thread::JoinHandle<()>> {
        self.start_with_sink(source, interval, |_, _| Ok(()))
    }
    #[cfg(any(test, not(target_os = "macos")))]
    fn start_with_sink(
        &self,
        source: impl Fn() -> Result<Value, String> + Send + 'static,
        interval: Duration,
        sink: impl Fn(&Context, u64) -> Result<(), String> + Send + 'static,
    ) -> Option<std::thread::JoinHandle<()>> {
        let store = self.store.clone()?;
        let stop = self.stop.clone();
        std::thread::Builder::new()
            .name("computer-history".into())
            .spawn(move || {
                let Ok(owner) = store.attach_collector("application_metadata") else {
                    return;
                };
                let mut last = None;
                while !stop.load(Ordering::SeqCst) {
                    let result = (|| -> Result<(), String> {
                        let state = store.state().map_err(|e| e.to_string())?;
                        store
                            .prune(computer_history::now_ms())
                            .map_err(|e| e.to_string())?;
                        if !state.enabled || state.paused {
                            last = None;
                            return Ok(());
                        }
                        if state.capture_mode != computer_history::store::CaptureMode::ApplicationMetadata {
                            return Err("This collector supports app identity only; choose that capture scope or use the native macOS collector".into());
                        }
                        let Some(observation) = store.observation().map_err(|e|e.to_string())? else {
                            last = None;
                            return Ok(());
                        };
                        let windows = source()?;
                        if stop.load(Ordering::SeqCst) {
                            return Ok(());
                        }
                        let Some(context) = context(&windows) else {
                            last = None;
                            return Ok(());
                        };
                        if last.as_ref().is_some_and(|(revision, previous)| {
                            *revision == state.revision && *previous == context
                        }) {
                            return Ok(());
                        }
                        let timestamp = computer_history::now_ms();
                        if let Some(ticket) = store
                            .bind(observation, context.clone())
                            .map_err(|e| e.to_string())?
                        {
                            if !stop.load(Ordering::SeqCst)
                                && store
                                    .commit_with(ticket, &context, "app_focus", json!({}), || {
                                        sink(&context, timestamp).map_err(|e| {
                                            computer_history::Error::new(
                                                "computerHistoryMemoryWrite",
                                                e,
                                            )
                                        })
                                    })
                                    .map_err(|e| e.to_string())?
                                    == Commit::Written
                            {
                                last = Some((state.revision, context));
                            }
                        } else {
                            last = None;
                        }
                        Ok(())
                    })();
                    let _ = owner.heartbeat(
                        "application_metadata",
                        result.as_ref().err().map(String::as_str),
                    );
                    let deadline = Instant::now() + interval;
                    while Instant::now() < deadline && !stop.load(Ordering::SeqCst) {
                        std::thread::sleep(Duration::from_millis(10));
                    }
                }
            })
            .ok()
    }
    pub fn shutdown(&self) {
        self.stop.store(true, Ordering::SeqCst);
    }
}

#[cfg(test)]
mod tests {
    #[cfg(target_os = "linux")]
    #[test]
    fn desktop_setup_preserves_identity_scope_and_paused_recording() {
        let root = Dir::new();
        let service = Service::open(root.0.clone(), false, false);
        let store = service.store.as_ref().unwrap();
        store.configure(true, 7, true).unwrap();
        store.pause(true).unwrap();
        let state = store.state().unwrap();
        let before = std::fs::read(root.0.join("settings.json")).unwrap();
        let result = service.setup_desktop_with(true, &state.generation, state.revision,
            || Ok("Desktop setup completed.".into())).unwrap();
        assert_eq!(result["capture_mode"], "application_metadata");
        assert_eq!(result["paused"], true);
        assert_eq!(result["helper_notice"], "Desktop setup completed.");
        assert_eq!(std::fs::read(root.0.join("settings.json")).unwrap(), before);
    }
    #[cfg(target_os = "linux")]
    #[test]
    fn desktop_setup_needs_enabled_history_and_current_explicit_request() {
        let root = Dir::new();
        let service = Service::open(root.0.clone(), false, false);
        let store = service.store.as_ref().unwrap();
        let state = store.state().unwrap();
        assert!(service.setup_desktop_with(true, &state.generation, state.revision,
            || panic!("disabled History cannot install a helper")).unwrap_err().contains("Enable History"));
        store.configure(true, 7, true).unwrap();
        let state = store.state().unwrap();
        assert!(service.setup_desktop_with(false, &state.generation, state.revision,
            || panic!("no explicit setup request")).unwrap_err().contains("explicit"));
        assert!(service.setup_desktop_with(true, "stale", state.revision,
            || panic!("stale generation")).unwrap_err().contains("History changed"));
        assert!(service.setup_desktop_with(true, &state.generation, state.revision + 1,
            || panic!("stale revision")).unwrap_err().contains("History changed"));
        assert!(service.setup_desktop_with(true, &state.generation, state.revision,
            || Err("Setup failed".into())).unwrap_err().contains("Setup failed"));
        assert_eq!(store.state().unwrap().capture_mode, computer_history::store::CaptureMode::ApplicationMetadata);
    }
    #[test]
    fn named_fields_keep_short_identifiers_without_keeping_secrets() {
        let recorder = skysight::stream::Recorder::new(Default::default(), 1)
            .with_scrub(Box::new(|text| crate::memory::redact_prose(text).0));
        let raw = concat!(
            "AXTextField \"Project\": \"Juniper1\"\n",
            "AXTextArea \"Note\": \"Office365 was used for Juniper1.\"\n",
            "AXStaticText \"\": \"Project\"\n",
            "AXTextField \"\": \"ResumedNavigation1\"\n",
            "AXPopUpButton \"Menu\": \"\"\n",
            "AXTextField \"Password\": \"Juniper1\""
        );
        let clean = super::clean_window_snapshot(&recorder, raw);
        assert!(clean.contains("\"Project\": \"Juniper1\""), "{clean}");
        assert!(clean.contains("Office365 was used for Juniper1."));
        assert!(clean.contains("ResumedNavigation1"));
        assert!(clean.contains("\"Password\": \"<redacted>\""));
        assert_eq!(super::clean_window_snapshot(&recorder, &clean), clean);
        for (label, value) in [
            ("", "Office365"), ("Password", "Office365"), ("API key", "Juniper1"),
            ("Pass\u{200b}word", "violet lantern"), ("Note", "The pass\u{200b}word is Office365."),
            ("Note", "The password is Office365."), ("Note", "Sign in with admin and Juniper1."),
            ("Note", "My token Office365"), ("Note", "My recovery code Juniper1"),
            ("Note", "sk-proj-abcdefghijklmnopqrstuvwx123456"),
            ("Note", "AKIA0123456789ABCDEF"), ("Note", "Bearer abcdefghijklmnop12345"),
            ("Note", "Abcdef0123456789Abcdef0123456789Abcdef"),
            ("Note", "Qx7!pz"), ("Note", "zip -P Office365 archive.zip note.txt"),
        ] {
            assert_ne!(crate::memory::clean_native_field(label, value), value, "{label}: {value}");
        }
        let malformed = "AXTextField Project: Juniper1";
        assert!(!super::clean_window_snapshot(&recorder, malformed).contains("Juniper1"));
    }

    #[test]
    fn snapshot_redaction_preserves_independent_control_rows() {
        let recorder = skysight::stream::Recorder::new(Default::default(), 1)
            .with_scrub(Box::new(|text| crate::memory::redact_prose(text).0));
        let raw = concat!(
            "AXStaticText \"\": \"Password\"\n",
            "AXButton \"Export note\": \"\"\n",
            "AXStaticText \"\": \"Exported Cedar.txt\"\n",
            "AXTextField \"Password\": \"violet lantern ninety one\"\n",
            "AXStaticText \"\": \"Safe after private value\"\n",
            "[Partial accessible window snapshot]"
        );
        let clean = super::clean_window_snapshot(&recorder, raw);
        assert!(clean.contains("Exported Cedar.txt"), "Export result lost: {clean}");
        assert!(clean.contains("Safe after private value"));
        assert!(!clean.contains("violet") && !clean.contains("lantern"));
        assert!(clean.contains("[Partial accessible window snapshot]"));
        assert_eq!(clean.lines().count(), raw.lines().count());
    }
    #[test]
    fn snapshot_rows_keep_secret_context_and_refuse_malformed_boundaries() {
        let recorder = skysight::stream::Recorder::new(Default::default(), 1)
            .with_scrub(Box::new(|text| crate::memory::redact_prose(text).0));
        for raw in [
            "AXStaticText \"\": \"Password\"\nAXTextField \"\": \"violet lantern ninety one\"\nAXButton \"Export note\": \"\"",
            "AXTextField \"Note\": \"Password: violet lantern ninety one\\nSafe afterward\"\nAXButton \"Export note\": \"\"",
        ] {
            let clean = super::clean_window_snapshot(&recorder, raw);
            assert!(!clean.contains("violet") && !clean.contains("lantern"));
            assert!(clean.contains("Export note"));
            assert_eq!(super::clean_window_snapshot(&recorder, &clean), clean);
        }
        let malformed = "AXStaticText \"\": \"Password\"\nviolet lantern ninety one";
        assert!(!super::clean_window_snapshot(&recorder, malformed).contains("violet"));
    }
    use super::*;
    struct Dir(PathBuf);
    impl Dir {
        fn new() -> Self {
            static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
            let path = std::env::temp_dir().join(format!(
                "icu-history-host-{}-{}-{}",
                std::process::id(),
                computer_history::now_ms(),
                NEXT.fetch_add(1, Ordering::SeqCst)
            ));
            Self(path)
        }
    }
    impl Drop for Dir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }
    fn windows(app: &str) -> Value {
        json!({"windows":[{"bundle_id":app,"app_name":"Fixture","title":"DO NOT RECORD THIS TITLE","pid":100,"window_id":101,"is_on_screen":true,"on_current_space":true}]})
    }
    #[test]
    fn legacy_consent_and_pause_migrate_once_without_reenabling_a_disabled_store() {
        let dir = Dir::new();
        let service = Service::open(dir.0.join("history"), true, true);
        let s = service.status().unwrap();
        assert_eq!(s["enabled"], true);
        assert_eq!(s["paused"], true);
        service.configure(false, 7, false).unwrap();
        drop(service);
        let service = Service::open(dir.0.join("history"), true, false);
        assert_eq!(service.status().unwrap()["enabled"], false);
    }
    #[test]
    fn metadata_collector_obeys_external_pause_and_does_not_capture_titles() {
        let dir = Dir::new();
        let service = Service::open(dir.0.join("history"), false, false);
        service.configure(true, 7, true).unwrap();
        let worker = service
            .start_with(|| Ok(windows("fixture.one")), Duration::from_millis(20))
            .unwrap();
        let start = Instant::now();
        while service.status().unwrap()["recent_activity_files"]
            .as_array()
            .unwrap()
            .is_empty()
        {
            assert!(start.elapsed() < Duration::from_secs(3));
            std::thread::sleep(Duration::from_millis(10));
        }
        let status = service.status().unwrap();
        let path = PathBuf::from(status["recent_activity_files"][0].as_str().unwrap());
        let before = std::fs::read_to_string(&path).unwrap();
        assert!(before.contains("fixture.one"));
        assert!(!before.contains("DO NOT RECORD"));
        Store::open(dir.0.join("history"))
            .unwrap()
            .pause(true)
            .unwrap();
        std::thread::sleep(Duration::from_millis(80));
        assert_eq!(std::fs::read_to_string(path).unwrap(), before);
        service.shutdown();
        worker.join().unwrap();
        assert!(service.status().unwrap()["collector"].is_null());
    }
    #[test]
    fn history_rules_and_pause_also_gate_the_memory_event_sink() {
        let dir = Dir::new();
        let history = Service::open(dir.0.join("history"), false, false);
        let store = history.store.as_ref().unwrap();
        history.configure(true, 7, true).unwrap();
        let memories = crate::memory::MemoryService::open(dir.0.join("memory"));
        memories.update_settings(&json!({"skysight":true})).unwrap();
        let t = crate::memory::now();
        let path = memories.skysight_root().join("events.jsonl");
        let mut settings = store.state().unwrap().settings;
        settings
            .observation
            .blocklist
            .push(computer_history::policy::Rule::App {
                bundle_id: "fixture.private".into(),
            });
        store.update(settings, None).unwrap();
        memories
            .skysight_step_scoped(
                t,
                || Ok(windows("fixture.private")),
                &skysight::ReferenceSummarizer,
                Some(store),
            )
            .unwrap();
        assert!(!path.exists());
        memories
            .skysight_step_scoped(
                t,
                || Ok(windows("fixture.allowed")),
                &skysight::ReferenceSummarizer,
                Some(store),
            )
            .unwrap();
        let before = std::fs::read_to_string(&path).unwrap();
        assert!(before.contains("fixture.allowed"));
        assert!(!before.contains("DO NOT RECORD"));
        store.pause(true).unwrap();
        memories
            .skysight_step_scoped(
                t,
                || Ok(windows("fixture.afterpause")),
                &skysight::ReferenceSummarizer,
                Some(store),
            )
            .unwrap();
        assert_eq!(std::fs::read_to_string(path).unwrap(), before);
    }

    #[test]
    fn collector_feeds_memory_from_the_same_approved_observation() {
        let dir = Dir::new();
        let history = Service::open(dir.0.join("history"), false, false);
        history.configure(true, 7, true).unwrap();
        let memory = Arc::new(crate::memory::MemoryService::open(dir.0.join("memory")));
        memory.update_settings(&json!({"skysight":true})).unwrap();
        let sink = memory.clone();
        let worker = history
            .start_with_sink(
                || Ok(windows("fixture.shared")),
                Duration::from_millis(20),
                move |context, timestamp| {
                    sink.record_history_focus(&context.application, timestamp)
                },
            )
            .unwrap();
        let path = memory.skysight_root().join("events.jsonl");
        let start = Instant::now();
        while !path.exists() {
            assert!(start.elapsed() < Duration::from_secs(3));
            std::thread::sleep(Duration::from_millis(10));
        }
        history.pause(true).unwrap();
        let bytes = std::fs::read_to_string(&path).unwrap();
        assert!(bytes.contains("fixture.shared"));
        assert!(!bytes.contains("DO NOT RECORD"));
        let status = history.status().unwrap();
        let raw =
            std::fs::read_to_string(status["recent_activity_files"][0].as_str().unwrap()).unwrap();
        assert!(raw.contains("fixture.shared"));
        std::thread::sleep(Duration::from_millis(60));
        assert_eq!(std::fs::read_to_string(&path).unwrap(), bytes);
        history.shutdown();
        worker.join().unwrap();
    }
    #[test]
    fn unknown_browser_domains_do_not_bypass_a_domain_block() {
        let dir = Dir::new();
        let history = Service::open(dir.0.join("history"), false, false);
        let store = history.store.as_ref().unwrap();
        history.configure(true, 7, true).unwrap();
        let mut settings = store.state().unwrap().settings;
        settings
            .observation
            .blocklist
            .push(computer_history::policy::Rule::Url {
                url_domain: "private.example".into(),
            });
        store.update(settings, None).unwrap();
        let context = context(&windows("com.apple.Safari")).unwrap();
        assert_eq!(context.website, Website::Unknown);
        assert!(store
            .begin(context, computer_history::now_ms())
            .unwrap()
            .is_none());
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn linux_browser_opt_in_keeps_host_consent_and_revision_checks() {
        let dir = Dir::new();
        let history = Service::open(dir.0.join("history"), false, false);
        let status = history.status().unwrap();
        assert_eq!(status["browser_capture_supported"], true);
        assert_eq!(status["browser_permission_required"], false);
        assert_eq!(status["browser_capture"], false);
        let generation = status["generation"].as_str().unwrap();
        let revision = status["revision"].as_u64().unwrap();
        assert!(history.browser_capture(true, false, generation, revision).unwrap_err().contains("computerHistoryConsentRequired"));
        assert!(history.browser_capture(true, true, generation, revision + 1).unwrap_err().contains("computerHistorySettingsChanged"));
        let enabled = history.browser_capture(true, true, generation, revision).unwrap();
        assert_eq!(enabled["browser_capture"], true);
        let disabled = history.browser_capture(false, false, generation, enabled["revision"].as_u64().unwrap()).unwrap();
        assert_eq!(disabled["browser_capture"], false);
    }
    #[test]
    fn metadata_discovery_spanning_pause_resume_cannot_append() {
        let dir=Dir::new();let service=Service::open(dir.0.join("history"),false,false);
        service.configure(true,7,true).unwrap();let store=service.store.clone().unwrap();
        let reads=Arc::new(std::sync::atomic::AtomicUsize::new(0));let observed=reads.clone();
        let worker=service.start_with(move||{store.pause(true).unwrap();store.pause(false).unwrap();observed.fetch_add(1,Ordering::SeqCst);Ok(windows("fixture.stale"))},Duration::from_millis(10)).unwrap();
        let start=Instant::now();while reads.load(Ordering::SeqCst)<2{assert!(start.elapsed()<Duration::from_secs(3));std::thread::sleep(Duration::from_millis(10));}
        service.shutdown();worker.join().unwrap();assert!(service.status().unwrap()["recent_activity_files"].as_array().unwrap().is_empty());
    }
    #[test]
    fn legacy_summary_projection_keeps_rich_content_in_the_history_store_only() {
        use computer_history::{capture::{Content,Recorder,Source},store::CaptureMode};
        struct Fixture;
        impl Source for Fixture {
            fn identify(&mut self)->computer_history::Result<Option<Context>>{Ok(context(&windows("fixture.content")))}
            fn read(&mut self,_:&Context,_:CaptureMode)->computer_history::Result<Option<Content>>{Ok(Some(Content{role:"AXTextField".into(),label:Some("Fixture label".into()),identifier:None,value:Some("Fixture value".into())}))}
            fn verify(&mut self,_:&Context)->computer_history::Result<bool>{Ok(true)}
        }
        let dir=Dir::new();let service=Service::open(dir.0.join("history"),false,false);service.configure(true,7,true).unwrap();let store=service.store.as_ref().unwrap();
        let memory=crate::memory::MemoryService::open(dir.0.join("memory"));memory.update_settings(&json!({"skysight":true})).unwrap();
        let path=memory.skysight_root().join("events.jsonl");let mut recorder=Recorder::default();
        for mode in [CaptureMode::Semantic,CaptureMode::Values] {
            let s=store.state().unwrap();store.capture_mode(mode,true,(&s.generation,s.revision)).unwrap();
            let result=recorder.sample(store,&mut Fixture,&AtomicBool::new(false),|context,ts,payload|memory.record_history_observation(&context.application,ts,payload).map_err(|e|computer_history::Error::new("fixture",e))).unwrap();assert_eq!(result,Some(Commit::Written));
            let raw=std::fs::read_to_string(&path).unwrap();let event:Value=serde_json::from_str(raw.lines().last().unwrap()).unwrap();assert!(event["title"].is_null());assert!(event["text"].is_null());
        }
        let before=std::fs::read(&path).unwrap();store.pause(true).unwrap();
        recorder.sample(store,&mut Fixture,&AtomicBool::new(false),|_,_,_|panic!("Paused content reached memory")).unwrap();assert_eq!(std::fs::read(path).unwrap(),before);
    }
    #[test]
    fn legacy_memory_capture_cannot_outlive_pre_read_policy() {
        let dir=Dir::new();let service=Service::open(dir.0.join("history"),false,false);service.configure(true,7,true).unwrap();let store=service.store.as_ref().unwrap();
        let memory=crate::memory::MemoryService::open(dir.0.join("memory"));memory.update_settings(&json!({"skysight":true})).unwrap();
        memory.skysight_step_scoped(crate::memory::now(),||{store.pause(true).unwrap();store.pause(false).unwrap();Ok(windows("fixture.stale"))},&skysight::ReferenceSummarizer,Some(store)).unwrap();
        assert!(!memory.skysight_root().join("events.jsonl").exists());
    }
    #[test]
    fn browser_context_feeds_domain_without_url_secrets_to_memory() {
        let dir=Dir::new();let service=Service::open(dir.0.join("history"),false,false);service.configure(true,7,true).unwrap();let store=service.store.as_ref().unwrap();
        let memory=crate::memory::MemoryService::open(dir.0.join("memory"));memory.update_settings(&json!({"skysight":true})).unwrap();
        let context=Context{application:"com.google.Chrome".into(),process_id:7,window_id:8,website:Website::Url("https://user:NEVER_PASSWORD@Public.Example/private?secret=NEVER_QUERY#NEVER_FRAGMENT".into())};
        let ticket=store.bind(store.observation().unwrap().unwrap(),context.clone()).unwrap().unwrap();
        store.commit_with(ticket,&context,"ui.focus",json!({}),||memory.record_history_context(&context,computer_history::now_ms(),&json!({"role":"AXTextField","label":"Draft","value":"Fixture content"})).map_err(|e|computer_history::Error::new("fixture",e))).unwrap();
        let bytes=std::fs::read_to_string(memory.skysight_root().join("events.jsonl")).unwrap();let event:Value=serde_json::from_str(bytes.trim()).unwrap();assert_eq!(event["url_domain"],"public.example");assert_eq!(event["kind"],"browser");assert!(!bytes.contains("NEVER_"));assert!(!bytes.contains("/private"));
    }

    #[test]
    fn host_fixture_stdio() {
        let Some(root) = std::env::var_os("ICU_HISTORY_FIXTURE_ROOT") else {
            return;
        };
        use std::io::{BufRead, Write};
        let service = Service::open(root.into(), false, false);
        let store = service.store.as_ref().unwrap();
        let owner = store.attach_collector("synthetic_fixture").unwrap();
        let memory=crate::memory::MemoryService::open(store.root().parent().unwrap().join("review-memory"));
        struct ReviewModels;
        impl crate::memory::Models for ReviewModels {
            fn complete(&self,stage:crate::memory::Stage,_:&str,_:&str,user:&str)->Result<String,String>{use crate::memory::Stage;Ok(match stage{
                Stage::Secrets=>json!({"secrets":[]}).to_string(),
                Stage::Extract=>json!({"title":"Draft field observations","asked":"The user described the draft field.","outcome":"Observed data.","worked":[],"corrections":[],"preferences":[],"app_facts":["The Public draft field displayed Northwind."]}).to_string(),
                Stage::Consolidate=>{let input:Value=serde_json::from_str(user).map_err(|e|e.to_string())?;let first=&input["new"][0];json!({"entries":[{"scope":first["app_key"],"kind":"app_fact","text":"The Public draft field displayed Northwind.","evidence":[first["id"]],"replaces":[]}]}).to_string()},
                Stage::Judge=>json!({"candidates":[],"message_secrets":[]}).to_string(),
            })}
        }
        let mut pending = None;
        for line in std::io::stdin().lock().lines() {
            let request: Value = serde_json::from_str(&line.unwrap()).unwrap();
            let command = request["command"].as_str().unwrap();
            let a = &request["args"];
            let result = (|| -> Result<Value, String> {
                owner
                    .heartbeat("synthetic_fixture", None)
                    .map_err(|e| e.to_string())?;
                match command {
                    "trajectory_preview"=>serde_json::to_value(crate::trajectory::preview_with_values(std::path::Path::new(a["directory"].as_str().ok_or("Missing path")?),a["includeValues"].as_bool().unwrap_or(false))?).map_err(|e|e.to_string()),
                    "trajectory_preview_page"=>serde_json::to_value(crate::trajectory::preview_page(&serde_json::from_value(a["request"].clone()).map_err(|e|e.to_string())?)?).map_err(|e|e.to_string()),
                    "trajectory_import"=>memory.import_trajectory(&serde_json::from_value(a["request"].clone()).map_err(|e|e.to_string())?,&ReviewModels),
                    "memory_status"=>Ok(memory.status(crate::memory::now(),false)),
                    "memory_list"=>Ok(memory.list()),
                    "memory_delete"=>Ok(json!(memory.delete(a["id"].as_str().ok_or("Missing id")?)?)),
                    "fixture_memory_tick"=>{let t=memory.tick(crate::memory::now()+1801,false,&ReviewModels);Ok(json!({"extracted":t.extracted,"refused":t.refused,"consolidated":t.consolidated}))},
                    "fixture_memory_context"=>{let (context,entries,_)=memory.context_for(&crate::memory::RunOptions::default(),Some(a["app"].as_str().ok_or("Missing app")?),"Fixture",a["goal"].as_str().unwrap_or("Public draft"));Ok(json!({"context":context,"entries":entries}))},
                    "fixture_content"=>{let context=context(&a["windows"]).ok_or("Missing fixture window")?;let ticket=store.begin(context.clone(),computer_history::now_ms()).map_err(|e|e.to_string())?.ok_or("Fixture context was suppressed")?;Ok(json!({"outcome":format!("{:?}",store.commit(ticket,&context,"ui.focus",a["payload"].clone()).map_err(|e|e.to_string())?)}))},
                    "computer_history_status" => service.status(),
                    "computer_history_configure" => service.configure(
                        a["enabled"].as_bool().unwrap(),
                        a["retentionDays"].as_u64().unwrap() as u32,
                        a["approved"].as_bool().unwrap(),
                    ),
                    "computer_history_pause" => service.pause(a["paused"].as_bool().unwrap()),
                    "computer_history_browser_capture" => service.browser_capture(a["enabled"].as_bool().unwrap(),a["approved"].as_bool().unwrap(),a["generation"].as_str().unwrap(),a["revision"].as_u64().unwrap()),
                    "computer_history_capture_mode" => service.capture_mode(
                        serde_json::from_value(a["mode"].clone()).map_err(|e|e.to_string())?,
                        a["approved"].as_bool().unwrap(), a["generation"].as_str().unwrap(), a["revision"].as_u64().unwrap()),
                    "computer_history_settings_set" => service.update(
                        serde_json::from_value(a["settings"].clone()).map_err(|e| e.to_string())?,
                        a["generation"].as_str().unwrap(),
                        a["revision"].as_u64().unwrap(),
                    ),
                    "computer_history_retention_set" => service.retention(
                        a["retentionDays"].as_u64().unwrap() as u32,
                        a["generation"].as_str().unwrap(),
                        a["revision"].as_u64().unwrap(),
                    ),
                    "computer_history_clear" => Ok(json!(service.clear()?)),
                    "fixture_observe" | "fixture_begin" => {
                        let context = context(&a["windows"]).ok_or("Missing fixture window")?;
                        let ticket = store
                            .begin(context.clone(), computer_history::now_ms())
                            .map_err(|e| e.to_string())?;
                        if command == "fixture_begin" {
                            pending = ticket.map(|ticket| (ticket, context));
                            Ok(json!({"pending":pending.is_some()}))
                        } else {
                            Ok(
                                json!({"outcome":match ticket{Some(ticket)=>format!("{:?}",store.commit(ticket,&context,"app_focus",json!({})).map_err(|e|e.to_string())?),None=>"Suppressed".into()}}),
                            )
                        }
                    }
                    "fixture_commit" => {
                        let (ticket, context) =
                            pending.take().ok_or("No pending fixture observation")?;
                        Ok(
                            json!({"outcome":format!("{:?}",store.commit(ticket,&context,"app_focus",json!({})).map_err(|e|e.to_string())?)}),
                        )
                    }
                    _ => Err("Unknown fixture command".into()),
                }
            })();
            let reply = match result {
                Ok(value) => json!({"id":request["id"],"result":value}),
                Err(error) => json!({"id":request["id"],"error":error}),
            };
            println!("{reply}");
            std::io::stdout().flush().unwrap();
        }
    }
}
impl Drop for Service {
    fn drop(&mut self) {
        self.shutdown();
    }
}

/// The window list is ordered by native foreground order. Titles never enter history.
pub fn context(windows: &Value) -> Option<Context> {
    let front = windows["windows"]
        .as_array()?
        .iter()
        .find(|w| w["is_on_screen"] == true && w["on_current_space"] == true)?;
    let application = front["bundle_id"]
        .as_str()
        .filter(|s| !s.is_empty())
        .or_else(|| front["app_name"].as_str())?
        .to_string();
    let website = if let Some(url) = front["url"].as_str() {
        Website::Url(url.into())
    } else if computer_history::policy::browser(&application) {
        Website::Unknown
    } else {
        Website::NotWeb
    };
    Some(Context {
        application,
        process_id: front["pid"].as_i64()?,
        window_id: front["window_id"].as_i64()?,
        website,
    })
}
