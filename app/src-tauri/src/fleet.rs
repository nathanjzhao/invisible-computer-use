//! The on-machine side of the observe-to-act bridge: polls a per-device inbox for
//! routines a fleet operator has offered, shows them passively for the user to review,
//! and reports dispatch lifecycle status back. Nothing here ever runs a routine; the
//! only run trigger is the user pressing Send in the existing workflow pipeline.
//!
//! Server-authored offer content is UNTRUSTED. The actor token and the pinned endpoint
//! NEVER leave Rust for the webview. State lives under `app_local_data_dir()/fleet/`
//! (0700 dir, 0600 files via `skysight::private`), not the memory root.
use crate::memory::{self, workflows::{self, DispatchSpec, Generated}};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::io::Read;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tauri::{AppHandle, Emitter, Manager, Runtime, State};

const MAX_INBOX_BYTES: u64 = 256 * 1024;
const MAX_SPEC_JSON: usize = 64 * 1024;
const POLL_MIN_S: u64 = 10;
const POLL_MAX_S: u64 = 600;
const POLL_DEFAULT_S: u64 = 30;
const BACKOFF_START_S: u64 = 30;
const BACKOFF_MAX_S: u64 = 300;
const EVENTS_ROTATE_BYTES: u64 = 256 * 1024;
const HTTP_TIMEOUT: Duration = Duration::from_secs(20);

fn sha256_hex(bytes: &[u8]) -> String { format!("{:x}", Sha256::digest(bytes)) }

// --- Status wire contract (B0) ---

/// The body the app POSTs to `/actor/dispatches/{id}/status` (spec 4.2/4.3). `state` is
/// one of the states the app may report (accepted/declined/started/finished/failed/
/// stopped); `result_code` is present only for states that carry one.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct StatusReport {
    pub state: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result_code: Option<String>,
}
impl StatusReport {
    pub fn new(state: &str, result_code: Option<&str>) -> Self {
        Self { state: state.into(), result_code: result_code.map(Into::into) }
    }
}

/// The server's 200 reply to a status POST (spec 4.2).
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct StatusResponse {
    pub ok: bool,
    pub state: String,
}

// --- Persisted + in-memory state ---

#[derive(Clone, Serialize, Deserialize)]
struct Link {
    endpoint: String,
    actor_token: String,
    device_label: String,
    linked_at: i64,
    #[serde(default)] expires_at: Option<String>,
}

/// One offered routine. The spec content (spec/spec_json) is held IN MEMORY only; it is
/// re-delivered by every poll while the offer is live, and is never written to disk
/// (the recorded person's routine stays off the local filesystem). Only the metadata
/// subset is persisted to offers.json.
#[derive(Clone)]
struct Offer {
    spec: DispatchSpec,
    spec_json: String,
    spec_sha256: String,
    workflow_id: String,
    state: String,
    received_at: i64,
    deadline_unix_s: i64,
}
#[derive(Clone, Serialize, Deserialize)]
struct PersistOffer {
    state: String,
    spec_sha256: String,
    workflow_id: String,
    received_at: i64,
    deadline_unix_s: i64,
}

#[derive(Default)]
struct Inner {
    root: Option<PathBuf>,
    opted_in: bool,
    endpoint: Option<String>,
    link: Option<Link>,
    offers: BTreeMap<String, Offer>,
    last_checked: Option<i64>,
    link_code: Option<String>,
    poll_token: Option<String>,
    poll_after_s: u64,
    // The tenant this device enrolled into (invite path). Display/record only; the actual
    // cross-tenant guard is the tenant-scoped actor token, not this string.
    tenant_id: Option<String>,
}

#[derive(Default)]
struct Shared { inner: Mutex<Inner> }

/// Managed as Tauri state. Holds the in-memory fleet state and the single background
/// worker (pairing poll, then inbox poll) that runs ONLY while opt-in is on.
#[derive(Default)]
pub struct Service {
    shared: Arc<Shared>,
    worker: Mutex<Option<Arc<AtomicBool>>>,
}

// --- Views returned to the webview (token NEVER included) ---

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StepPreview { pub place: String, pub action: Option<String> }

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OfferView {
    pub dispatch_id: String,
    pub title: String,
    pub description: String,
    pub issuer_label: String,
    pub issuer_note: Option<String>,
    pub steps_preview: Vec<StepPreview>,
    pub app_hint: String,
    pub deadline_unix_s: i64,
    pub state: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FleetStatus {
    pub opted_in: bool,
    pub linked: bool,
    pub device_label: Option<String>,
    pub endpoint_configured: bool,
    pub last_checked: Option<i64>,
    pub link_code: Option<String>,
    pub tenant_id: Option<String>,
    pub offers: Vec<OfferView>,
}

// --- Endpoint resolution (spec 5.1) ---

/// Whether this endpoint is safe to send the actor bearer to: https to any host, or
/// cleartext http ONLY to a loopback LITERAL. The URL is fully parsed (never a
/// prefix/substring match): `http://127.0.0.1.evil.com` (prefix trick) and
/// `http://127.0.0.1@evil.com` / `http://user@evil.com` (userinfo) both have the real
/// host `evil.com` and are rejected, so the tkact_ token can never leak in cleartext.
fn https_ok(endpoint: &str) -> bool {
    let Ok(url) = url::Url::parse(endpoint) else { return false; };
    match url.scheme() {
        "https" => url.host().is_some(),
        "http" => match url.host() {
            Some(url::Host::Domain(domain)) => domain == "localhost",
            Some(url::Host::Ipv4(ip)) => ip == std::net::Ipv4Addr::LOCALHOST,
            Some(url::Host::Ipv6(ip)) => ip == std::net::Ipv6Addr::LOCALHOST,
            _ => false,
        },
        _ => false,
    }
}

fn managed_endpoint(root: &std::path::Path) -> Option<String> {
    let text = skysight::private::read_bounded(&root.join("managed.json"), 8192)?;
    serde_json::from_str::<Value>(&text).ok()?.get("endpoint")?.as_str().map(str::to_string)
}

fn managed_tenant_id(root: &std::path::Path) -> Option<String> {
    let text = skysight::private::read_bounded(&root.join("managed.json"), 8192)?;
    serde_json::from_str::<Value>(&text).ok()?.get("tenantId")?.as_str().map(str::to_string)
}

impl Inner {
    /// env ICU_FLEET_ENDPOINT, then fleet/managed.json, then a stored endpoint.
    fn resolve_endpoint(&self) -> Option<String> {
        if let Ok(env) = std::env::var("ICU_FLEET_ENDPOINT") {
            if https_ok(&env) { return Some(env); }
        }
        if let Some(root) = &self.root {
            if let Some(managed) = managed_endpoint(root) {
                if https_ok(&managed) { return Some(managed); }
            }
        }
        self.endpoint.clone().filter(|e| https_ok(e))
    }
    /// The endpoint the worker uses THIS iteration. Once linked it is PINNED to the
    /// endpoint the actor token was minted against (`link.endpoint`), ignoring
    /// env/managed.json/stored: a later endpoint change must NEVER redirect the
    /// authenticated `tkact_` token to a new host. `resolve_endpoint` governs only the
    /// pre-link (where-to-link) phase; on unlink/relink the endpoint is re-resolved and
    /// re-pinned.
    fn worker_endpoint(&self) -> Option<String> {
        match &self.link {
            Some(link) => Some(link.endpoint.clone()),
            None => self.resolve_endpoint(),
        }
    }
    fn dir(&self) -> Option<&PathBuf> { self.root.as_ref() }
}

// --- Persistence (metadata only; never spec content) ---

fn load_persisted(inner: &mut Inner) {
    let Some(root) = inner.root.clone() else { return };
    if let Some(text) = skysight::private::read_bounded(&root.join("flag.json"), 4096) {
        inner.opted_in = serde_json::from_str::<Value>(&text).ok()
            .and_then(|v| v.get("optedIn").and_then(Value::as_bool)).unwrap_or(false);
    }
    if let Some(text) = skysight::private::read_bounded(&root.join("endpoint.json"), 8192) {
        inner.endpoint = serde_json::from_str::<Value>(&text).ok()
            .and_then(|v| v.get("endpoint").and_then(Value::as_str).map(str::to_string));
    }
    if let Some(text) = skysight::private::read_bounded(&root.join("link.json"), 16384) {
        inner.link = serde_json::from_str::<Link>(&text).ok();
    }
    // The enrolled tenant (invite path) is recorded in managed.json alongside the endpoint.
    inner.tenant_id = managed_tenant_id(&root);
    if let Some(text) = skysight::private::read_bounded(&root.join("offers.json"), 65536) {
        if let Ok(map) = serde_json::from_str::<BTreeMap<String, PersistOffer>>(&text) {
            // Metadata only; the spec content arrives on the next poll.
            for (id, p) in map {
                inner.offers.entry(id).or_insert(Offer {
                    spec: placeholder_spec(), spec_json: String::new(),
                    spec_sha256: p.spec_sha256, workflow_id: p.workflow_id,
                    state: p.state, received_at: p.received_at, deadline_unix_s: p.deadline_unix_s,
                });
            }
        }
    }
}

/// A persisted offer loaded from disk has no live spec content yet (that arrives on the
/// next poll). This stand-in is never surfaced or staged: `content_ready` gates both.
fn placeholder_spec() -> DispatchSpec {
    DispatchSpec {
        v: 0, dispatch_id: String::new(), issued_at: String::new(),
        issuer: workflows::Issuer { label: String::new(), note: None },
        source: workflows::DispatchSource { workflow_id: String::new(), window_days: 0, runs: 0, sessions: 0, median_duration_s: 0, first_seen_ms: None, last_seen_ms: None },
        title: String::new(), description: String::new(), verification: String::new(),
        parameters: vec![], steps: vec![],
    }
}
impl Offer {
    fn content_ready(&self) -> bool { !self.spec_json.is_empty() }
}

fn persist_flag(inner: &Inner) {
    if let Some(root) = inner.dir() {
        let _ = skysight::private::create_dir(root);
        let _ = skysight::private::write(&root.join("flag.json"), json!({"optedIn": inner.opted_in}).to_string());
    }
}
fn persist_endpoint(inner: &Inner) {
    if let (Some(root), Some(ep)) = (inner.dir(), inner.endpoint.clone()) {
        let _ = skysight::private::create_dir(root);
        let _ = skysight::private::write(&root.join("endpoint.json"), json!({"endpoint": ep}).to_string());
    }
}
fn persist_link(inner: &Inner) {
    if let Some(root) = inner.dir() {
        let _ = skysight::private::create_dir(root);
        match &inner.link {
            Some(link) => { let _ = skysight::private::write(&root.join("link.json"), serde_json::to_string(link).unwrap_or_default()); }
            None => { let _ = std::fs::remove_file(root.join("link.json")); }
        }
    }
}
fn persist_offers(inner: &Inner) {
    if let Some(root) = inner.dir() {
        let map: BTreeMap<&String, PersistOffer> = inner.offers.iter().map(|(id, o)| (id, PersistOffer {
            state: o.state.clone(), spec_sha256: o.spec_sha256.clone(), workflow_id: o.workflow_id.clone(),
            received_at: o.received_at, deadline_unix_s: o.deadline_unix_s,
        })).collect();
        let _ = skysight::private::create_dir(root);
        let _ = skysight::private::write(&root.join("offers.json"), serde_json::to_string(&map).unwrap_or_default());
    }
}
fn log_event(inner: &Inner, event: &str, detail: Value) {
    let Some(root) = inner.dir() else { return };
    let _ = skysight::private::create_dir(root);
    let path = root.join("events.jsonl");
    if std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0) > EVENTS_ROTATE_BYTES {
        let _ = std::fs::rename(&path, root.join("events.jsonl.1"));
    }
    if let Ok(mut f) = skysight::private::append(&path) {
        use std::io::Write;
        let line = json!({"at": memory::now(), "event": event, "detail": detail}).to_string();
        let _ = writeln!(f, "{line}");
    }
}

// --- HTTP (ureq, redirects(0), endpoint pinned at link time) ---

fn http_agent() -> ureq::Agent { ureq::builder().redirects(0).build() }

#[derive(Debug)]
enum HttpError { Unauthorized, Status(u16, String), Transport(String) }

fn read_capped(resp: ureq::Response, cap: u64) -> Result<String, HttpError> {
    let mut buf = String::new();
    resp.into_reader().take(cap + 1).read_to_string(&mut buf).map_err(|e| HttpError::Transport(e.to_string()))?;
    if buf.len() as u64 > cap { return Err(HttpError::Transport("response too large".into())); }
    Ok(buf)
}

fn call_error(e: ureq::Error) -> HttpError {
    match e {
        ureq::Error::Status(401, _) => HttpError::Unauthorized,
        ureq::Error::Status(code, resp) => HttpError::Status(code, resp.into_string().unwrap_or_default()),
        other => HttpError::Transport(other.to_string()),
    }
}

struct Inbox { items: Vec<InboxItem>, poll_after_s: u64 }
#[derive(Deserialize)]
struct InboxItem { dispatch_id: String, spec_json: String, spec_sha256: String, expires_in_s: i64 }

fn poll_inbox(agent: &ureq::Agent, endpoint: &str, token: &str) -> Result<Inbox, HttpError> {
    let resp = agent.get(&format!("{}/api/observer/actor/inbox", endpoint.trim_end_matches('/')))
        .set("Authorization", &format!("Bearer {token}"))
        .timeout(HTTP_TIMEOUT).call().map_err(call_error)?;
    let body = read_capped(resp, MAX_INBOX_BYTES)?;
    let value: Value = serde_json::from_str(&body).map_err(|e| HttpError::Transport(e.to_string()))?;
    let poll_after_s = value.get("poll_after_s").and_then(Value::as_u64).unwrap_or(POLL_DEFAULT_S).clamp(POLL_MIN_S, POLL_MAX_S);
    let items: Vec<InboxItem> = value.get("items").cloned()
        .and_then(|v| serde_json::from_value(v).ok()).unwrap_or_default();
    Ok(Inbox { items: items.into_iter().take(3).collect(), poll_after_s })
}

fn post_status(agent: &ureq::Agent, endpoint: &str, token: &str, dispatch_id: &str, report: &StatusReport)
    -> Result<StatusResponse, HttpError> {
    let url = format!("{}/api/observer/actor/dispatches/{}/status", endpoint.trim_end_matches('/'), dispatch_id);
    let resp = agent.post(&url).set("Authorization", &format!("Bearer {token}"))
        .timeout(HTTP_TIMEOUT).send_json(report).map_err(call_error)?;
    let body = read_capped(resp, 16 * 1024)?;
    serde_json::from_str(&body).map_err(|e| HttpError::Transport(e.to_string()))
}

// --- Invite enrollment (Decision A: redeem IS the pairing) ---
//
// An admin mints a one-time, TTL'd, tenant-scoped invite and sends the user a deep link
// `icu://enroll?invite=<token>&h=<host>` (short `code` accepted too). Redeeming it binds a
// fresh device to the invite's tenant and returns a `tkact_` actor token that is immediately
// usable as the inbox bearer, so the invite path SKIPS the interactive device-code handshake:
// redeem is the pairing. The returned endpoint is pinned by worker_endpoint() exactly as a
// device-code link would be. The interactive code path survives only for a manual server URL.

/// The server's 200 reply to `POST {host}/v1/enroll/redeem` (INVITE-CONTRACT.md). `deviceToken`
/// is the tenant-scoped `tkact_` bearer; `endpoint` is where that device's inbox lives (it may
/// differ from the redeem host).
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RedeemResponse {
    endpoint: String,
    device_token: String,
    #[serde(default)] device_label: Option<String>,
    tenant_id: String,
    #[serde(default)] workflow_id: Option<String>,
}

/// An invite link or a pasted short code, with the host to redeem against. `host` is the
/// `h=` param from the link, else a fallback resolved from env/managed.json; a bare pasted
/// code carries no host and relies entirely on that fallback.
#[derive(Debug, Clone, PartialEq)]
struct EnrollInput { invite: Option<String>, code: Option<String>, host: Option<String> }

/// Accept a bare host (`fleet.acme.example`) or a full URL; default the scheme to https and
/// require https_ok so the invite credential and later the `tkact_` token never go cleartext.
fn normalize_host(h: &str) -> Result<String, String> {
    let h = h.trim();
    let candidate = if h.contains("://") { h.to_string() } else { format!("https://{h}") };
    if https_ok(&candidate) { Ok(candidate.trim_end_matches('/').to_string()) }
    else { Err("Your organization server address must be https.".into()) }
}

/// Parse an `icu://enroll?invite=...&h=...` deep link (also `?code=...`), or a bare pasted
/// code. `fallback_host` is used when the link carries no `h=` (and is the only source of a
/// host for a bare code).
fn parse_enroll_input(raw: &str, fallback_host: Option<String>) -> Result<EnrollInput, String> {
    let raw = raw.trim();
    if raw.is_empty() { return Err("Paste the invite link or code your administrator sent you.".into()); }
    if let Ok(url) = url::Url::parse(raw) {
        if url.scheme() == "icu" {
            let (mut invite, mut code, mut host) = (None, None, None);
            for (k, v) in url.query_pairs() {
                match k.as_ref() {
                    "invite" => invite = Some(v.into_owned()),
                    "code" => code = Some(v.into_owned()),
                    "h" | "host" => host = Some(v.into_owned()),
                    _ => {}
                }
            }
            if invite.is_none() && code.is_none() {
                return Err("This invite link is missing its invite token. Ask your administrator to send a new one.".into());
            }
            let host = match host { Some(h) => Some(normalize_host(&h)?), None => fallback_host };
            return Ok(EnrollInput { invite, code, host });
        }
        // A full http(s) URL pasted into the invite box is not an invite; send them to Advanced.
        if url.scheme() == "http" || url.scheme() == "https" {
            return Err("That looks like a server address. Open the invite link your administrator sent, or add a server under Advanced.".into());
        }
    }
    // Not a URL: treat the whole string as a short code (needs a fallback host to route).
    Ok(EnrollInput { invite: None, code: Some(raw.to_string()), host: fallback_host })
}

/// POST the invite (or code) to `{host}/v1/enroll/redeem`. The body carries the credential;
/// the response carries the token.
fn redeem(agent: &ureq::Agent, host: &str, input: &EnrollInput) -> Result<RedeemResponse, HttpError> {
    let mut body = serde_json::Map::new();
    if let Some(invite) = &input.invite { body.insert("invite".into(), json!(invite)); }
    else if let Some(code) = &input.code { body.insert("code".into(), json!(code)); }
    let resp = agent.post(&format!("{}/v1/enroll/redeem", host.trim_end_matches('/')))
        .timeout(HTTP_TIMEOUT).send_json(Value::Object(body)).map_err(call_error)?;
    let text = read_capped(resp, 16 * 1024)?;
    serde_json::from_str(&text).map_err(|e| HttpError::Transport(e.to_string()))
}

/// Write `managed.json` atomically: a sibling temp file (0600 via skysight::private) renamed
/// over the target, so a crash mid-write can never leave a half-written enrollment record.
/// Shape extends the read-only provisioning drop the app already understands
/// (`{endpoint}`), so an old drop still resolves an endpoint.
fn write_managed_atomic(root: &std::path::Path, endpoint: &str, device_token: &str, tenant_id: &str) -> Result<(), String> {
    skysight::private::create_dir(root)?;
    let body = json!({"endpoint": endpoint, "deviceToken": device_token, "tenantId": tenant_id}).to_string();
    let tmp = root.join("managed.json.tmp");
    skysight::private::write(&tmp, &body)?;
    std::fs::rename(&tmp, root.join("managed.json")).map_err(|e| {
        let _ = std::fs::remove_file(&tmp);
        e.to_string()
    })
}

// --- Ingest (digest verified before parse; untrusted content) ---

/// Fold inbox items into the offers map. Returns declines to POST (id, report): a
/// routine with an unknown `v` is declined `unsupported_version` without ever being
/// surfaced; a digest mismatch or oversize body is dropped silently (protocol noise).
fn ingest(inner: &mut Inner, items: Vec<InboxItem>, now: i64) -> Vec<(String, StatusReport)> {
    let mut declines = Vec::new();
    for item in items {
        if item.spec_json.len() > MAX_SPEC_JSON { log_event(inner, "drop_oversize", json!({"dispatch_id": item.dispatch_id})); continue; }
        // Rule (3): hash the received string as-is, before parsing.
        if sha256_hex(item.spec_json.as_bytes()) != item.spec_sha256 {
            log_event(inner, "drop_digest_mismatch", json!({"dispatch_id": item.dispatch_id}));
            continue;
        }
        let v = serde_json::from_str::<Value>(&item.spec_json).ok()
            .and_then(|val| val.get("v").and_then(Value::as_u64));
        if v != Some(1) {
            declines.push((item.dispatch_id.clone(), StatusReport::new("declined", Some("unsupported_version"))));
            continue;
        }
        let spec: DispatchSpec = match serde_json::from_str(&item.spec_json) {
            Ok(spec) => spec,
            Err(_) => { declines.push((item.dispatch_id.clone(), StatusReport::new("declined", Some("invalid_spec")))); continue; }
        };
        let deadline = now.saturating_add(item.expires_in_s);
        let workflow_id = spec.source.workflow_id.clone();
        match inner.offers.get_mut(&item.dispatch_id) {
            // A known offer re-delivered: refresh the deadline and restore live content.
            Some(existing) => {
                existing.deadline_unix_s = deadline;
                existing.spec = spec;
                existing.spec_json = item.spec_json;
                existing.spec_sha256 = item.spec_sha256;
                existing.workflow_id = workflow_id;
                if existing.state.is_empty() { existing.state = "delivered".into(); }
            }
            None => {
                inner.offers.insert(item.dispatch_id.clone(), Offer {
                    spec, spec_json: item.spec_json, spec_sha256: item.spec_sha256, workflow_id,
                    state: "delivered".into(), received_at: now, deadline_unix_s: deadline,
                });
            }
        }
    }
    // Prune anything past its local deadline so the map and the UI stay current.
    inner.offers.retain(|_, o| o.deadline_unix_s > now || o.state == "accepted");
    persist_offers(inner);
    declines
}

fn offer_views(inner: &Inner, now: i64) -> Vec<OfferView> {
    inner.offers.values().filter(|o| o.content_ready() && o.deadline_unix_s > now)
        .map(|o| OfferView {
            dispatch_id: o.spec.dispatch_id.clone(),
            title: o.spec.title.clone(),
            description: o.spec.description.clone(),
            issuer_label: o.spec.issuer.label.clone(),
            issuer_note: o.spec.issuer.note.clone(),
            steps_preview: o.spec.steps.iter().take(6)
                .map(|s| StepPreview { place: s.place.clone(), action: s.action.clone() }).collect(),
            app_hint: o.spec.steps.first().map(|s| s.app.clone()).unwrap_or_default(),
            deadline_unix_s: o.deadline_unix_s,
            state: o.state.clone(),
        }).collect()
}

// --- Service methods ---

impl Service {
    fn with_inner<T>(&self, f: impl FnOnce(&mut Inner) -> T) -> T {
        let mut inner = self.shared.inner.lock().unwrap_or_else(|e| e.into_inner());
        f(&mut inner)
    }

    /// Resolve and remember the fleet dir from the app handle, loading persisted state
    /// exactly once. Safe to call on every command.
    fn ensure_root<R: Runtime>(&self, app: &AppHandle<R>) {
        self.with_inner(|inner| {
            if inner.root.is_some() { return; }
            if let Ok(dir) = app.path().app_local_data_dir() {
                inner.root = Some(dir.join("fleet"));
                load_persisted(inner);
            }
        });
    }

    pub fn status(&self) -> FleetStatus {
        let now = memory::now();
        self.with_inner(|inner| FleetStatus {
            opted_in: inner.opted_in,
            linked: inner.link.is_some(),
            device_label: inner.link.as_ref().map(|l| l.device_label.clone()),
            endpoint_configured: inner.resolve_endpoint().is_some(),
            last_checked: inner.last_checked,
            link_code: inner.link_code.clone(),
            tenant_id: inner.tenant_id.clone(),
            offers: offer_views(inner, now),
        })
    }

    /// Stop the background worker (opt-out, unlink, or app exit).
    pub fn shutdown(&self) {
        if let Some(stop) = self.worker.lock().unwrap_or_else(|e| e.into_inner()).take() {
            stop.store(true, Ordering::SeqCst);
        }
    }

    fn link_snapshot(&self) -> Option<(String, String)> {
        self.with_inner(|inner| inner.link.as_ref().map(|l| (l.endpoint.clone(), l.actor_token.clone())))
    }

    /// Report a dispatch status without blocking the caller: a detached short-timeout
    /// POST, failures ignored (spec 5.3 step 6 / B3). No-op if not linked.
    pub fn report_status_detached(&self, dispatch_id: String, report: StatusReport) {
        let Some((endpoint, token)) = self.link_snapshot() else { return };
        std::thread::Builder::new().name("fleet-status".into()).spawn(move || {
            let agent = http_agent();
            let _ = post_status(&agent, &endpoint, &token, &dispatch_id, &report);
        }).ok();
    }

    /// I3 live check: post `accepted` and require the server to still accept it. A
    /// 409 (withdrawn/expired) or any failure means the save must NOT proceed.
    fn confirm_accept(&self, dispatch_id: &str) -> Result<(), String> {
        let (endpoint, token) = self.link_snapshot().ok_or("Your organization link was removed or expired")?;
        let agent = http_agent();
        match post_status(&agent, &endpoint, &token, dispatch_id, &StatusReport::new("accepted", None)) {
            Ok(_) => {
                self.with_inner(|inner| {
                    if let Some(o) = inner.offers.get_mut(dispatch_id) { o.state = "accepted".into(); }
                    persist_offers(inner);
                });
                Ok(())
            }
            Err(HttpError::Status(409, _)) => {
                self.with_inner(|inner| { inner.offers.remove(dispatch_id); persist_offers(inner); });
                Err("This offer is no longer available".into())
            }
            Err(HttpError::Unauthorized) => Err("Your organization link was removed or expired".into()),
            Err(_) => Err("Could not confirm this routine with your organization; try again".into()),
        }
    }

    /// Build a reviewable draft from an offer (fleet_stage core). Enforces I2: refuses
    /// unless opted in, linked, the offer is known, content is present, it is not
    /// locally expired or withdrawn, and its digest re-verifies. `app_key`/`app_name`
    /// come from the window the user picked.
    fn stage(&self, memory: &memory::MemoryService, runtime: &workflows::Runtime,
             dispatch_id: &str, app_key: String, app_name: String) -> Result<Generated, String> {
        let now = memory::now();
        let offer = self.with_inner(|inner| -> Result<Offer, String> {
            if !inner.opted_in { return Err("Turn on routines from your organization first".into()); }
            if inner.link.is_none() { return Err("This computer is not linked to your organization".into()); }
            let offer = inner.offers.get(dispatch_id).ok_or("This offer is no longer available")?.clone();
            if !offer.content_ready() { return Err("This offer is still loading; try again in a moment".into()); }
            if offer.deadline_unix_s <= now { return Err("This offer has expired".into()); }
            if offer.state != "delivered" && offer.state != "accepted" { return Err("This offer is no longer available".into()); }
            Ok(offer)
        })?;
        if sha256_hex(offer.spec_json.as_bytes()) != offer.spec_sha256 {
            return Err("This offer failed its integrity check".into());
        }
        match runtime.stage_dispatch(memory, offer.spec, app_key, app_name) {
            Ok(generated) => Ok(generated),
            Err(stage_err) => {
                if let Some(code) = stage_err.decline_code() {
                    // A spec the local gates reject is declined to the operator, best effort.
                    if let Some((endpoint, token)) = self.link_snapshot() {
                        let agent = http_agent();
                        let _ = post_status(&agent, &endpoint, &token, dispatch_id, &StatusReport::new("declined", Some(code)));
                    }
                    self.with_inner(|inner| { if let Some(o) = inner.offers.get_mut(dispatch_id) { o.state = "declined".into(); } persist_offers(inner); });
                }
                Err(stage_err.to_string())
            }
        }
    }

    /// Redeem an invite and link this device to the returned tenant endpoint (Decision A).
    /// Writes managed.json atomically, reconstructs the Link with the returned `tkact_` token
    /// as the actor bearer, opts in, persists, and best-effort PUTs exec-consent. The caller
    /// starts the worker; worker_endpoint() then pins to the returned endpoint (HIGH-B). No
    /// interactive device-code handshake runs on this path.
    fn enroll(&self, input: EnrollInput) -> Result<(), String> {
        let host = input.host.clone().ok_or(
            "Open the full invite link your administrator sent, or add your organization server under Advanced.")?;
        let agent = http_agent();
        let redeemed = redeem(&agent, &host, &input).map_err(|e| match e {
            HttpError::Status(404, _) => "That invite was not found. Ask your administrator to send a new one.".to_string(),
            HttpError::Status(409, _) => "That invite was already used. Ask your administrator to send a new one.".to_string(),
            HttpError::Status(410, _) => "That invite has expired. Ask your administrator to send a new one.".to_string(),
            HttpError::Status(429, _) => "Your organization has reached its device limit. Ask your administrator.".to_string(),
            HttpError::Unauthorized => "That invite was rejected. Ask your administrator to send a new one.".to_string(),
            _ => "Could not reach your organization to redeem this invite. Check your connection and try again.".to_string(),
        })?;
        if !https_ok(&redeemed.endpoint) {
            return Err("Your organization returned an address this app will not use. Contact your administrator.".into());
        }
        let device_label = redeemed.device_label.clone()
            .filter(|l| !l.trim().is_empty()).unwrap_or_else(hostname);
        self.with_inner(|inner| -> Result<(), String> {
            let root = inner.root.clone().ok_or("The app data folder is not ready yet; try again.")?;
            write_managed_atomic(&root, &redeemed.endpoint, &redeemed.device_token, &redeemed.tenant_id)?;
            inner.link = Some(Link {
                endpoint: redeemed.endpoint.clone(),
                actor_token: redeemed.device_token.clone(),
                device_label: device_label.clone(),
                linked_at: memory::now(),
                expires_at: None,
            });
            inner.tenant_id = Some(redeemed.tenant_id.clone());
            inner.opted_in = true;
            inner.poll_token = None;
            inner.link_code = None;
            persist_link(inner);
            persist_flag(inner);
            // Never log the token: tenant id and label only.
            log_event(inner, "enrolled", json!({"tenantId": redeemed.tenant_id, "deviceLabel": device_label}));
            Ok(())
        })?;
        // Mirror the device-code path's post-link step: tell the server execution is on, to the
        // PINNED endpoint (worker_endpoint == link.endpoint; post_exec_consent reads the live token).
        if let Some(endpoint) = self.with_inner(|i| i.worker_endpoint()) {
            let _ = post_exec_consent(&agent, &endpoint, &self.shared, true);
        }
        Ok(())
    }
}

// --- The background worker (one thread, state-driven) ---

fn hostname() -> String {
    std::env::var("HOSTNAME").ok().or_else(|| std::env::var("COMPUTERNAME").ok())
        .filter(|h| !h.trim().is_empty()).unwrap_or_else(|| "copilot-device".into())
}

fn sleep_cancellable(stop: &AtomicBool, secs: u64) {
    for _ in 0..secs {
        if stop.load(Ordering::SeqCst) { return; }
        std::thread::sleep(Duration::from_secs(1));
    }
}

/// Start the worker if opted in, linked (or pairing), and not already running. The
/// worker pairs first (while a poll_token is pending) and then polls the inbox; it
/// exits on the stop flag (opt-out / unlink / exit) or on 401.
fn start_worker<R: Runtime>(service: &Service, app: &AppHandle<R>) {
    let mut slot = service.worker.lock().unwrap_or_else(|e| e.into_inner());
    if slot.as_ref().is_some_and(|s| !s.load(Ordering::SeqCst)) { return; }
    let (opted_in, has_link, has_pending) = service.with_inner(|i| (i.opted_in, i.link.is_some(), i.poll_token.is_some()));
    if !opted_in || (!has_link && !has_pending) { return; }
    let stop = Arc::new(AtomicBool::new(false));
    *slot = Some(stop.clone());
    let shared = service.shared.clone();
    let app = app.clone();
    std::thread::Builder::new().name("fleet-poll".into()).spawn(move || {
        worker_loop(shared, stop, move |kind, data| { let _ = app.emit("fleet", json!({"kind": kind, "data": data})); });
    }).ok();
}

fn worker_loop(shared: Arc<Shared>, stop: Arc<AtomicBool>, emit: impl Fn(&str, Value)) {
    let agent = http_agent();
    let mut backoff = BACKOFF_START_S;
    let mut warned_redirect = false;
    while !stop.load(Ordering::SeqCst) {
        // worker_endpoint pins to the linked endpoint once paired; resolve_endpoint only
        // governs the pre-link phase (see Inner::worker_endpoint).
        let (opted_in, link, pending, endpoint) = {
            let i = shared.inner.lock().unwrap_or_else(|e| e.into_inner());
            (i.opted_in, i.link.clone(), i.poll_token.clone(), i.worker_endpoint())
        };
        if !opted_in { break; }
        let Some(endpoint) = endpoint else { sleep_cancellable(&stop, BACKOFF_START_S); continue; };

        if link.is_none() {
            // Pairing: poll link/poll until the operator approves or it expires.
            let Some(token) = pending else { break; };
            match claim_link(&agent, &endpoint, &token) {
                Ok(Some(link)) => {
                    let device_label = link.device_label.clone();
                    { let mut i = shared.inner.lock().unwrap_or_else(|e| e.into_inner()); i.link = Some(link); i.poll_token = None; i.link_code = None; persist_link(&i); }
                    // `endpoint` is the host we just claimed the link against == link.endpoint (pinned).
                    let _ = post_exec_consent(&agent, &endpoint, &shared, true);
                    emit("link", json!({"deviceLabel": device_label}));
                    backoff = BACKOFF_START_S;
                }
                Ok(None) => sleep_cancellable(&stop, 5),
                Err(HttpError::Unauthorized) | Err(HttpError::Status(410, _)) => {
                    { let mut i = shared.inner.lock().unwrap_or_else(|e| e.into_inner()); i.poll_token = None; i.link_code = None; }
                    emit("error", json!({"message": "The link code expired. Turn the setting off and on to get a new one."}));
                    break;
                }
                Err(_) => sleep_cancellable(&stop, 5),
            }
            continue;
        }

        let link = link.unwrap();
        // The endpoint is pinned to link.endpoint. If the configured endpoint has since
        // changed, warn the UI ONCE and keep using the linked one; never follow the change
        // (that would hand the tkact_ token to a new host).
        if !warned_redirect {
            let resolved = shared.inner.lock().unwrap_or_else(|e| e.into_inner()).resolve_endpoint();
            if resolved.as_deref() != Some(link.endpoint.as_str()) {
                emit("error", json!({"message": "Your organization server address changed. This computer keeps using the address it linked to; unlink and link again to switch."}));
                warned_redirect = true;
            }
        }
        let token = link.actor_token.clone();
        match poll_inbox(&agent, &endpoint, &token) {
            Ok(inbox) => {
                let now = memory::now();
                let declines = {
                    let mut i = shared.inner.lock().unwrap_or_else(|e| e.into_inner());
                    i.last_checked = Some(now);
                    i.poll_after_s = inbox.poll_after_s;
                    ingest(&mut i, inbox.items, now)
                };
                for (id, report) in declines {
                    let _ = post_status(&agent, &endpoint, &token, &id, &report);
                }
                emit("offers", json!({}));
                backoff = BACKOFF_START_S;
                let wait = shared.inner.lock().unwrap_or_else(|e| e.into_inner()).poll_after_s.clamp(POLL_MIN_S, POLL_MAX_S);
                sleep_cancellable(&stop, wait);
            }
            Err(HttpError::Unauthorized) => {
                { let mut i = shared.inner.lock().unwrap_or_else(|e| e.into_inner()); i.link = None; persist_link(&i); }
                emit("unlinked", json!({"message": "Your organization link was removed or expired"}));
                break;
            }
            Err(_) => {
                emit("error", json!({}));
                sleep_cancellable(&stop, backoff);
                backoff = (backoff * 2).min(BACKOFF_MAX_S);
            }
        }
    }
}

#[derive(Deserialize)]
struct LinkStartResponse { user_code: String, poll_token: String }
#[derive(Deserialize)]
struct LinkPollResponse { actor_token: String, device_label: String, #[serde(default)] expires_at: Option<String> }

fn link_start(agent: &ureq::Agent, endpoint: &str) -> Result<LinkStartResponse, HttpError> {
    let resp = agent.post(&format!("{}/api/observer/actor/link/start", endpoint.trim_end_matches('/')))
        .timeout(HTTP_TIMEOUT)
        .send_json(json!({"hostname": hostname(), "app_version": env!("CARGO_PKG_VERSION")}))
        .map_err(call_error)?;
    let body = read_capped(resp, 16 * 1024)?;
    serde_json::from_str(&body).map_err(|e| HttpError::Transport(e.to_string()))
}

/// Poll link/poll once. Ok(None) = still pending (202); Ok(Some(link)) = claimed once.
fn claim_link(agent: &ureq::Agent, endpoint: &str, poll_token: &str) -> Result<Option<Link>, HttpError> {
    let resp = match agent.post(&format!("{}/api/observer/actor/link/poll", endpoint.trim_end_matches('/')))
        .timeout(HTTP_TIMEOUT).send_json(json!({"poll_token": poll_token})) {
        Ok(resp) => resp,
        Err(e) => return Err(call_error(e)),
    };
    if resp.status() == 202 { return Ok(None); }
    let body = read_capped(resp, 16 * 1024)?;
    let parsed: LinkPollResponse = serde_json::from_str(&body).map_err(|e| HttpError::Transport(e.to_string()))?;
    Ok(Some(Link {
        endpoint: endpoint.to_string(), actor_token: parsed.actor_token,
        device_label: parsed.device_label, linked_at: memory::now(), expires_at: parsed.expires_at,
    }))
}

/// PUT exec-consent with the actor bearer (read from the LIVE link). Callers MUST pass the
/// PINNED endpoint (link.endpoint via worker_endpoint()), never a re-resolved one: the token is
/// minted for the linked host, so an env/managed.json change must not redirect it (HIGH-B). No
/// link -> no token -> no-op (the endpoint arg is then irrelevant).
fn post_exec_consent(agent: &ureq::Agent, endpoint: &str, shared: &Shared, enabled: bool) -> Result<(), HttpError> {
    let token = { let i = shared.inner.lock().unwrap_or_else(|e| e.into_inner());
        i.link.as_ref().map(|l| l.actor_token.clone()) };
    let Some(token) = token else { return Ok(()); };
    agent.put(&format!("{}/api/observer/actor/exec-consent", endpoint.trim_end_matches('/')))
        .set("Authorization", &format!("Bearer {token}"))
        .timeout(HTTP_TIMEOUT).send_json(json!({"enabled": enabled})).map_err(call_error)?;
    Ok(())
}

// --- Invite enrollment entrypoints (deep link + paste) ---

/// Ingest an `icu://enroll?...` link delivered by the deep-link plugin (a cold start or a
/// second launch forwarded by single-instance). Redeems and links; the `tkact_` token never
/// reaches the webview. Result is surfaced through the `fleet` event the UI already listens to:
/// `enrolled` on success (then a status refresh shows phase 2), `enroll_error` with a message.
pub fn ingest_enroll_url<R: Runtime>(app: &AppHandle<R>, url: &str) {
    let service = app.state::<Service>();
    service.ensure_root(app);
    let fallback = service.with_inner(|i| i.resolve_endpoint());
    match parse_enroll_input(url, fallback).and_then(|input| service.enroll(input)) {
        Ok(()) => {
            start_worker(&service, app);
            let _ = app.emit("fleet", json!({"kind": "enrolled", "data": {}}));
        }
        Err(message) => {
            let _ = app.emit("fleet", json!({"kind": "enroll_error", "data": {"message": message}}));
        }
    }
}

/// The Connect screen's "paste your invite link or code" path. Same ingest as the deep link,
/// but returns the refreshed status (or an error string the UI shows inline) to the caller.
#[tauri::command]
pub fn fleet_enroll<R: Runtime>(app: AppHandle<R>, service: State<'_, Service>, input: String)
    -> Result<FleetStatus, String> {
    service.ensure_root(&app);
    let fallback = service.with_inner(|i| i.resolve_endpoint());
    let parsed = parse_enroll_input(&input, fallback)?;
    service.enroll(parsed)?;
    start_worker(&service, &app);
    Ok(service.status())
}

// --- Tauri commands (JS argument names are camelCase) ---

#[tauri::command]
pub fn fleet_status<R: Runtime>(app: AppHandle<R>, service: State<'_, Service>) -> FleetStatus {
    service.ensure_root(&app);
    start_worker(&service, &app); // resume polling after a relaunch if still opted in
    service.status()
}

#[tauri::command]
pub fn fleet_set_exec_optin<R: Runtime>(app: AppHandle<R>, service: State<'_, Service>, enabled: bool, endpoint: Option<String>)
    -> Result<FleetStatus, String> {
    service.ensure_root(&app);
    if enabled {
        // The opt-in panel's endpoint text box (fallback provisioning, spec 5.1/9.1).
        // Env and managed.json still take precedence inside resolve_endpoint.
        if let Some(entered) = endpoint.map(|e| e.trim().to_string()).filter(|e| !e.is_empty()) {
            if !https_ok(&entered) { return Err("Enter an https address for your organization server".into()); }
            service.with_inner(|inner| { inner.endpoint = Some(entered); persist_endpoint(inner); });
        }
        service.with_inner(|inner| { inner.opted_in = true; persist_flag(inner); });
        if service.with_inner(|i| i.link.is_some()) {
            // Already linked: re-affirm exec-consent to the PINNED endpoint. worker_endpoint()
            // returns link.endpoint while linked, so an env/managed.json change can't redirect
            // the actor token to a new host (HIGH-B).
            if let Some(endpoint) = service.with_inner(|i| i.worker_endpoint()) {
                let agent = http_agent();
                let _ = post_exec_consent(&agent, &endpoint, &service.shared, true);
            }
        } else {
            // Not linked yet: resolve where to link (pre-link, no token) and start pairing.
            let Some(endpoint) = service.with_inner(|i| i.resolve_endpoint()) else {
                // No endpoint yet: the panel shows the text box. Stay opted in, unlinked.
                return Ok(service.status());
            };
            let agent = http_agent();
            match link_start(&agent, &endpoint) {
                Ok(start) => service.with_inner(|i| { i.link_code = Some(start.user_code); i.poll_token = Some(start.poll_token); }),
                Err(_) => return Err("Could not reach your organization to start linking".into()),
            }
        }
        start_worker(&service, &app);
        Ok(service.status())
    } else {
        service.shutdown();
        service.with_inner(|inner| {
            inner.opted_in = false;
            inner.link_code = None;
            inner.poll_token = None;
            persist_flag(inner);
        });
        // Best-effort: tell the server execution is off, to the PINNED endpoint. worker_endpoint()
        // is link.endpoint while linked, and post_exec_consent no-ops when not linked (no token).
        // An off-toggle must not leak the token to a re-resolved host either (HIGH-B).
        if let Some(endpoint) = service.with_inner(|i| i.worker_endpoint()) {
            let agent = http_agent();
            let _ = post_exec_consent(&agent, &endpoint, &service.shared, false);
        }
        // Discard staged dispatch drafts so an off switch cannot leave a reviewable one.
        app.state::<workflows::Runtime>().discard_dispatch_drafts();
        service.with_inner(|inner| { inner.offers.clear(); persist_offers(inner); });
        Ok(service.status())
    }
}

#[tauri::command]
pub fn fleet_decline<R: Runtime>(app: AppHandle<R>, service: State<'_, Service>, dispatch_id: String)
    -> Result<FleetStatus, String> {
    service.ensure_root(&app);
    if let Some((endpoint, token)) = service.link_snapshot() {
        let agent = http_agent();
        let _ = post_status(&agent, &endpoint, &token, &dispatch_id, &StatusReport::new("declined", Some("declined_by_user")));
    }
    service.with_inner(|inner| { inner.offers.remove(&dispatch_id); persist_offers(inner); });
    Ok(service.status())
}

#[tauri::command]
pub fn fleet_unlink<R: Runtime>(app: AppHandle<R>, service: State<'_, Service>) -> Result<FleetStatus, String> {
    service.ensure_root(&app);
    service.shutdown();
    if let Some((endpoint, token)) = service.link_snapshot() {
        let agent = http_agent();
        let _ = agent.delete(&format!("{}/api/observer/actor/link", endpoint.trim_end_matches('/')))
            .set("Authorization", &format!("Bearer {token}")).timeout(HTTP_TIMEOUT).call();
    }
    app.state::<workflows::Runtime>().discard_dispatch_drafts();
    service.with_inner(|inner| { inner.link = None; inner.offers.clear(); inner.link_code = None; inner.poll_token = None; persist_link(inner); persist_offers(inner); });
    Ok(service.status())
}

/// Bridge an offered routine into the existing workflow draft pipeline. Builds a draft
/// ONLY; it never runs anything (I1) and refuses unless the gates in `Service::stage`
/// pass (I2). The window the user picked supplies the app identity, never the spec.
#[tauri::command]
pub fn fleet_stage<R: Runtime>(
    app: AppHandle<R>, service: State<'_, Service>, runtime: State<'_, workflows::Runtime>,
    memory: State<'_, memory::MemoryService>, dispatch_id: String, pid: i64, app_name: String,
) -> Result<Generated, String> {
    service.ensure_root(&app);
    let app_key = crate::agent::bundle_for_pid(pid).ok_or("The selected target has no stable application identity")?;
    service.stage(memory.inner(), runtime.inner(), &dispatch_id, app_key, app_name)
}

/// Called by `workflow_save` (lib.rs) before the local save of a dispatch draft: posts
/// the live `accepted` status and refuses the save on 409 (I3). Returns Ok for a
/// non-dispatch draft (nothing to confirm).
pub fn confirm_dispatch_accept<R: Runtime>(app: &AppHandle<R>, request_id: &str) -> Result<(), String> {
    let Some(dispatch_id) = app.state::<workflows::Runtime>().draft_dispatch_id(request_id) else { return Ok(()); };
    let service = app.state::<Service>();
    service.ensure_root(app);
    service.confirm_accept(&dispatch_id)
}

/// Called by `workflow_save` when the local save of a dispatch draft fails after it was
/// accepted: reports `failed`/`local_error` so the operator sees the outcome.
pub fn report_local_failure<R: Runtime>(app: &AppHandle<R>, dispatch_id: &str) {
    app.state::<Service>().report_status_detached(dispatch_id.to_string(), StatusReport::new("failed", Some("local_error")));
}

/// Map an `agent` event kind to the dispatch status to report (spec 5.3 step 6). Only
/// terminal kinds carry a status; everything else returns None.
pub fn terminal_status(kind: &str) -> Option<(&'static str, Option<&'static str>)> {
    match kind {
        "done" => Some(("finished", Some("reported_done"))),
        "fail" => Some(("failed", Some("agent_failed"))),
        "error" => Some(("failed", Some("agent_error"))),
        "stopped" => Some(("stopped", Some("stopped_by_user"))),
        _ => None,
    }
}

/// Report a dispatch lifecycle status from the agent run (B3). Detached, best-effort;
/// no-op if the fleet Service is absent or this build has no link.
pub fn report_dispatch_status<R: Runtime>(app: &AppHandle<R>, dispatch_id: &str, state: &str, result_code: Option<&str>) {
    if let Some(service) = app.try_state::<Service>() {
        service.report_status_detached(dispatch_id.to_string(), StatusReport::new(state, result_code));
    }
}

#[cfg(test)]
mod enroll_parse {
    //! Invite ingest parsing + the managed.json write (no network). The live redeem round
    //! trip is the gated `live_enroll_roundtrip` in `live_e2e`.
    use super::*;

    #[test]
    fn full_invite_link_with_host_parses() {
        let got = parse_enroll_input("icu://enroll?invite=inv_abc&h=fleet.acme.example", None).unwrap();
        assert_eq!(got.invite.as_deref(), Some("inv_abc"));
        assert_eq!(got.code, None);
        assert_eq!(got.host.as_deref(), Some("https://fleet.acme.example"));
    }

    #[test]
    fn link_host_accepts_a_full_https_url_and_trims_trailing_slash() {
        let got = parse_enroll_input("icu://enroll?invite=inv_abc&h=https://fleet.acme.example/", None).unwrap();
        assert_eq!(got.host.as_deref(), Some("https://fleet.acme.example"));
    }

    #[test]
    fn code_param_is_read_and_a_missing_host_falls_back() {
        let got = parse_enroll_input("icu://enroll?code=WXYZ-7Q2K", Some("https://fb.example".into())).unwrap();
        assert_eq!(got.code.as_deref(), Some("WXYZ-7Q2K"));
        assert_eq!(got.invite, None);
        assert_eq!(got.host.as_deref(), Some("https://fb.example"));
    }

    #[test]
    fn link_without_invite_or_code_is_rejected() {
        assert!(parse_enroll_input("icu://enroll?x=1", None).is_err());
    }

    #[test]
    fn bare_pasted_code_becomes_a_code_with_the_fallback_host() {
        let got = parse_enroll_input("WXYZ-7Q2K", Some("https://fb.example".into())).unwrap();
        assert_eq!(got.code.as_deref(), Some("WXYZ-7Q2K"));
        assert_eq!(got.host.as_deref(), Some("https://fb.example"));
        // No fallback host: a bare code cannot route on its own; enroll() turns this into a
        // clear "open the full link" error.
        assert_eq!(parse_enroll_input("WXYZ-7Q2K", None).unwrap().host, None);
    }

    #[test]
    fn a_pasted_server_url_is_pushed_to_advanced_not_treated_as_an_invite() {
        assert!(parse_enroll_input("https://fleet.acme.example", None).is_err());
        assert!(parse_enroll_input("", None).is_err());
    }

    #[test]
    fn host_must_be_https() {
        assert!(parse_enroll_input("icu://enroll?invite=x&h=http://evil.example", None).is_err());
        // localhost cleartext is allowed (matches https_ok / dev servers).
        assert_eq!(parse_enroll_input("icu://enroll?invite=x&h=http://localhost:8080", None).unwrap().host.as_deref(),
            Some("http://localhost:8080"));
    }

    #[test]
    fn managed_json_is_written_atomically_and_reads_back() {
        let root = std::env::temp_dir().join(format!("icu-enroll-managed-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        write_managed_atomic(&root, "https://dev.acme.example", "tkact_secret", "org_acme").unwrap();
        assert_eq!(managed_endpoint(&root).as_deref(), Some("https://dev.acme.example"));
        assert_eq!(managed_tenant_id(&root).as_deref(), Some("org_acme"));
        // No temp file is left behind.
        assert!(!root.join("managed.json.tmp").exists());
        // Overwriting (re-enroll) replaces cleanly.
        write_managed_atomic(&root, "https://dev2.acme.example", "tkact_2", "org_acme").unwrap();
        assert_eq!(managed_endpoint(&root).as_deref(), Some("https://dev2.acme.example"));
        let _ = std::fs::remove_dir_all(&root);
    }
}

#[cfg(test)]
mod dispatch_contract {
    //! B0 contract test: the app and server agree on the dispatch wire bytes. Fixtures
    //! are byte-identical to the server copy; `python3 gen_dispatch_fixtures.py --check`
    //! guards them in CI.
    use super::*;
    use crate::memory::{self, workflows::Runtime};
    use std::path::PathBuf;

    const DIGEST: &str = "c1d36fe486a44ac06277ee71823e5f326f3a66b457cf1844bafbbf6ca8fbed71";

    fn fixture(name: &str) -> String {
        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/dispatch").join(name);
        std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()))
    }

    #[test]
    fn inbox_spec_json_round_trips_and_matches_the_published_digest() {
        let inbox: Value = serde_json::from_str(&fixture("inbox_response.json")).unwrap();
        let item = &inbox["items"][0];
        let spec_json = item["spec_json"].as_str().unwrap();
        let spec_sha256 = item["spec_sha256"].as_str().unwrap();
        assert_eq!(sha256_hex(spec_json.as_bytes()), spec_sha256);
        assert_eq!(spec_sha256, DIGEST);
        let spec: DispatchSpec = serde_json::from_str(spec_json).unwrap();
        let reserialized = serde_json::to_string(&spec).unwrap();
        assert_eq!(reserialized, spec_json, "DispatchSpec serialization must equal items[0].spec_json");
        assert_eq!(sha256_hex(reserialized.as_bytes()), DIGEST);
        let pretty: DispatchSpec = serde_json::from_str(&fixture("dispatch_spec_v1.json")).unwrap();
        assert_eq!(serde_json::to_string(&pretty).unwrap(), spec_json);
        assert_eq!(inbox["poll_after_s"].as_u64(), Some(30));
        assert_eq!(item["expires_in_s"].as_u64(), Some(259200));
    }

    #[test]
    fn staging_the_fixture_spec_yields_a_valid_reviewable_plan() {
        let inbox: Value = serde_json::from_str(&fixture("inbox_response.json")).unwrap();
        let spec_json = inbox["items"][0]["spec_json"].as_str().unwrap();
        let spec: DispatchSpec = serde_json::from_str(spec_json).unwrap();
        let root = std::env::temp_dir().join(format!("icu-fleet-contract-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let memory = memory::MemoryService::open(root.clone());
        let rt = Runtime::default();
        let generated = rt.stage_dispatch(&memory, spec, "com.google.Chrome".into(), "Google Chrome".into())
            .expect("the fixture spec must stage into a valid Generated");
        assert_eq!(generated.plan.steps.len(), 3);
        assert_eq!(generated.source.format, "fleet-dispatch/v1");
        assert!(generated.request_id.starts_with("wf-"));
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn status_payloads_match_the_fixtures() {
        let payload = StatusReport::new("finished", Some("reported_done"));
        let expected: Value = serde_json::from_str(&fixture("status_payload.json")).unwrap();
        assert_eq!(serde_json::to_value(&payload).unwrap(), expected);
        assert_eq!(serde_json::to_string(&StatusReport::new("started", None)).unwrap(), r#"{"state":"started"}"#);
        let response: StatusResponse = serde_json::from_str(&fixture("status_response.json")).unwrap();
        assert_eq!(response, StatusResponse { ok: true, state: "finished".into() });
    }
}

#[cfg(test)]
mod service_tests;

#[cfg(test)]
mod live_e2e;
