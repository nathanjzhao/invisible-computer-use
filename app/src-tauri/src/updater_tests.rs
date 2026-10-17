// The updater, end to end, against a local manifest server and a throwaway signing key.
//
// The plugin under test is the one the app ships (`tauri-plugin-updater`), configured with the
// app's own `plugins.updater` block from tauri.conf.json: only the endpoint (a local http
// server) and the public key (a throwaway) are replaced, so every other setting is the shipped
// one. Nothing is published and nothing real is signed. On macOS an install runs against a
// fake `Copilot.app` in the temp dir, never the running app.
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::{Arc, Mutex};

use serde_json::{json, Value};
use tauri_plugin_updater::UpdaterExt;

const SHIPPED_CONFIG: &str = include_str!("../tauri.conf.json");
type Mock = tauri::test::MockRuntime;

struct Keys { pk: minisign::PublicKey, sk: minisign::SecretKey }
fn keys() -> Keys {
    let minisign::KeyPair { pk, sk } = minisign::KeyPair::generate_unencrypted_keypair().unwrap();
    Keys { pk, sk }
}
fn b64(s: &str) -> String { base64_encode(s.as_bytes()) }
fn base64_encode(bytes: &[u8]) -> String {
    use base64::Engine;
    base64::engine::general_purpose::STANDARD.encode(bytes)
}
fn pubkey_of(k: &Keys) -> String { b64(&k.pk.to_box().unwrap().to_string()) }

/// What `tauri build` writes next to an updater archive: a minisign signature whose trusted
/// comment is `timestamp:<t>\tfile:<name>`, plus `\tversion:<v>` from the 2.12 CLI on.
fn sign(k: &Keys, bytes: &[u8], version: Option<&str>) -> String {
    let mut trusted = "timestamp:1791100000\tfile:Copilot.app.tar.gz".to_string();
    if let Some(v) = version { trusted.push_str(&format!("\tversion:{v}")); }
    let sig = minisign::sign(Some(&k.pk), &k.sk, std::io::Cursor::new(bytes), Some(&trusted), Some("signature from a throwaway test key")).unwrap();
    b64(&sig.to_string())
}

/// The updater archive: on Linux what `tauri build` writes for an AppImage (`Copilot.AppImage.tar.gz`
/// holding `Copilot.AppImage`), elsewhere a `.app.tar.gz` whose app holds `marker` as its executable.
#[cfg(target_os = "linux")]
fn archive(marker: &str) -> Vec<u8> {
    let mut gz = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
    {
        let mut tar = tar::Builder::new(&mut gz);
        let mut header = tar::Header::new_gnu();
        header.set_size(marker.len() as u64);
        header.set_mode(0o755);
        header.set_cksum();
        tar.append_data(&mut header, "Copilot.AppImage", marker.as_bytes()).unwrap();
        tar.finish().unwrap();
    }
    gz.finish().unwrap()
}
#[cfg(not(target_os = "linux"))]
fn archive(marker: &str) -> Vec<u8> {
    static N: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
    let dir = std::env::temp_dir().join(format!("icu-upd-src-{}-{}", std::process::id(), N.fetch_add(1, std::sync::atomic::Ordering::Relaxed)));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("Copilot.app/Contents/MacOS")).unwrap();
    std::fs::write(dir.join("Copilot.app/Contents/MacOS/app"), marker).unwrap();
    let out = dir.join("Copilot.app.tar.gz");
    let status = std::process::Command::new("tar").env("COPYFILE_DISABLE", "1").arg("czf").arg(&out).arg("-C").arg(&dir).arg("Copilot.app").status().unwrap();
    assert!(status.success());
    let bytes = std::fs::read(&out).unwrap();
    let _ = std::fs::remove_dir_all(&dir);
    bytes
}

/// The "installed" app the updater is pointed at: `<tmp>/Copilot.app/Contents/MacOS/app`, or on
/// Linux the AppImage file `<tmp>/Copilot.AppImage` (what `$APPIMAGE` names in a real run).
struct Installed { root: std::path::PathBuf }
impl Installed {
    fn new(tag: &str) -> Installed {
        static N: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        let root = std::env::temp_dir().join(format!("icu-upd-app-{}-{tag}-{}", std::process::id(), N.fetch_add(1, std::sync::atomic::Ordering::Relaxed)));
        let _ = std::fs::remove_dir_all(&root);
        #[cfg(not(target_os = "linux"))]
        { std::fs::create_dir_all(root.join("Copilot.app/Contents/MacOS")).unwrap(); }
        #[cfg(target_os = "linux")]
        { std::fs::create_dir_all(&root).unwrap(); }
        let me = Installed { root };
        std::fs::write(me.exe(), "OLD 0.1.0").unwrap();
        #[cfg(target_os = "linux")]
        { use std::os::unix::fs::PermissionsExt; std::fs::set_permissions(me.exe(), std::fs::Permissions::from_mode(0o755)).unwrap(); }
        me
    }
    #[cfg(not(target_os = "linux"))]
    fn exe(&self) -> std::path::PathBuf { self.root.join("Copilot.app/Contents/MacOS/app") }
    #[cfg(target_os = "linux")]
    fn exe(&self) -> std::path::PathBuf { self.root.join("Copilot.AppImage") }
    fn content(&self) -> String { std::fs::read_to_string(self.exe()).unwrap_or_else(|_| "<missing>".into()) }
}
impl Drop for Installed { fn drop(&mut self) { let _ = std::fs::remove_dir_all(&self.root); } }

#[derive(Default)]
struct Serving {
    manifest: String,
    archive: Vec<u8>,
    /// Send the archive's headers with its whole length, then only this many bytes, then close.
    cut_after: Option<usize>,
    /// Omit Content-Length (a close-delimited body) and send only this many bytes.
    cut_without_length: Option<usize>,
    archive_status: Option<u16>,
    manifest_status: Option<u16>,
    requests: Vec<String>,
}

/// A local server for `/latest.json` and `/Copilot.app.tar.gz`. `base` is its URL.
fn serve() -> (String, Arc<Mutex<Serving>>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let state = Arc::new(Mutex::new(Serving::default()));
    let shared = state.clone();
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(stream) = stream else { continue };
            let shared = shared.clone();
            std::thread::spawn(move || handle(stream, shared));
        }
    });
    (base, state)
}

fn handle(mut stream: TcpStream, state: Arc<Mutex<Serving>>) {
    let mut head = Vec::new();
    let mut byte = [0u8; 1];
    while !head.ends_with(b"\r\n\r\n") {
        if stream.read(&mut byte).unwrap_or(0) == 0 { return; }
        head.push(byte[0]);
    }
    let path = String::from_utf8_lossy(&head).lines().next().unwrap_or("").split(' ').nth(1).unwrap_or("").to_string();
    let mut s = state.lock().unwrap();
    s.requests.push(path.clone());
    let reply = |stream: &mut TcpStream, status: u16, length: Option<usize>, body: &[u8]| {
        let mut h = format!("HTTP/1.1 {status} X\r\nConnection: close\r\n");
        if let Some(n) = length { h.push_str(&format!("Content-Length: {n}\r\n")); }
        h.push_str("\r\n");
        let _ = stream.write_all(h.as_bytes());
        let _ = stream.write_all(body);
        let _ = stream.flush();
    };
    if path.starts_with("/latest.json") {
        let body = s.manifest.clone().into_bytes();
        reply(&mut stream, s.manifest_status.unwrap_or(200), Some(body.len()), &body);
    } else if path.starts_with("/Copilot.app.tar.gz") {
        let all = s.archive.clone();
        let status = s.archive_status.unwrap_or(200);
        match (s.cut_after, s.cut_without_length) {
            (Some(n), _) => reply(&mut stream, status, Some(all.len()), &all[..n.min(all.len())]),
            (_, Some(n)) => reply(&mut stream, status, None, &all[..n.min(all.len())]),
            _ => reply(&mut stream, status, Some(all.len()), &all),
        }
    } else {
        reply(&mut stream, 404, Some(0), b"");
    }
}

fn manifest(base: &str, version: &str, signature: &str) -> String {
    let target = tauri_plugin_updater::target().expect("a supported platform");
    json!({"version": version, "notes": "throwaway", "pub_date": "2026-10-04T00:00:00Z",
        "platforms": {target: {"signature": signature, "url": format!("{base}/Copilot.app.tar.gz")}}}).to_string()
}

/// The shipped `plugins.updater` block, aimed at a local http server and a throwaway key.
fn shipped() -> Value {
    let cfg = serde_json::from_str::<Value>(SHIPPED_CONFIG).unwrap()["plugins"]["updater"].clone();
    #[cfg(target_os = "linux")]
    let cfg = {
        let mut cfg = cfg;
        let linux: Value = serde_json::from_str(include_str!("../tauri.linux.conf.json")).unwrap();
        cfg.as_object_mut().unwrap().extend(linux["plugins"]["updater"].as_object().unwrap().clone());
        cfg
    };
    cfg
}
fn local_config(base: &str, k: &Keys) -> Value { local_config_for(base, &pubkey_of(k)) }
fn local_config_for(base: &str, pubkey: &str) -> Value {
    let mut cfg = shipped();
    cfg["endpoints"] = json!([format!("{base}/latest.json")]);
    cfg["pubkey"] = json!(pubkey);
    cfg["dangerousInsecureTransportProtocol"] = json!(true);
    cfg
}

fn app_with(cfg: Value) -> Result<tauri::App<Mock>, tauri::Error> {
    let mut ctx = tauri::test::mock_context(tauri::test::noop_assets());
    ctx.config_mut().plugins.0.insert("updater".into(), cfg);
    tauri::test::mock_builder().plugin(tauri_plugin_updater::Builder::new().build()).build(ctx)
}

/// Check, download and (on macOS) install through the shipped updater. `Ok(None)` is "no update".
enum Outcome { NoUpdate, CheckFailed(String), DownloadFailed(String), Installed(String) }
fn run(app: &tauri::App<Mock>, installed: &Installed) -> Outcome {
    let updater = app.handle().updater_builder().executable_path(installed.exe()).timeout(std::time::Duration::from_secs(10)).build().unwrap();
    tauri::async_runtime::block_on(async {
        let update = match updater.check().await {
            Ok(Some(u)) => u,
            Ok(None) => return Outcome::NoUpdate,
            Err(e) => return Outcome::CheckFailed(e.to_string()),
        };
        let bytes = match update.download(|_, _| {}, || {}).await {
            Ok(b) => b,
            Err(e) => return Outcome::DownloadFailed(e.to_string()),
        };
        #[cfg(any(target_os = "macos", target_os = "linux"))]
        if let Err(e) = update.install(&bytes) { return Outcome::DownloadFailed(format!("install: {e}")); }
        let _ = bytes;
        Outcome::Installed(update.version.clone())
    })
}

fn rig(version: &str, signed_for: Option<&str>) -> (tauri::App<Mock>, Arc<Mutex<Serving>>, Keys, Installed, String) {
    let k = keys();
    let (base, state) = serve();
    let bytes = archive("NEW");
    {
        let mut s = state.lock().unwrap();
        s.manifest = manifest(&base, version, &sign(&k, &bytes, signed_for));
        s.archive = bytes;
    }
    let app = app_with(local_config(&base, &k)).unwrap();
    (app, state, k, Installed::new(version), base)
}

#[test]
fn the_shipped_configuration_is_valid_and_binds_each_signature_to_its_version() {
    let cfg = shipped();
    // The shipped block loads as it is: an https endpoint and a minisign public key.
    assert!(app_with(cfg.clone()).is_ok(), "the shipped updater block does not load");
    assert!(cfg["endpoints"][0].as_str().unwrap().starts_with("https://"));
    // A manifest is not signed, so an old release's archive can be offered under a new number.
    // The signature is what must say which release it is.
    assert_eq!(cfg["requireSignedVersion"], json!(true), "plugins.updater.requireSignedVersion is not set");
    // A release build refuses an http endpoint unless the dangerous flag is on (a debug build, which
    // runs this, only warns), so what ships must have no dangerous flag and an https endpoint.
    assert!(cfg.get("dangerousInsecureTransportProtocol").is_none() && cfg.get("dangerousAcceptInvalidCerts").is_none()
        && cfg.get("dangerousAcceptInvalidHostnames").is_none());
}

#[test]
fn a_newer_release_signed_by_the_release_key_is_found_downloaded_and_installed() {
    let (app, state, _k, installed, _base) = rig("0.2.0", Some("0.2.0"));
    match run(&app, &installed) {
        Outcome::Installed(v) => assert_eq!(v, "0.2.0"),
        Outcome::NoUpdate => panic!("no update was offered"),
        Outcome::CheckFailed(e) | Outcome::DownloadFailed(e) => panic!("{e}"),
    }
    if cfg!(any(target_os = "macos", target_os = "linux")) { assert_eq!(installed.content(), "NEW", "the app was not replaced"); }
    assert_eq!(state.lock().unwrap().requests, ["/latest.json", "/Copilot.app.tar.gz"]);
}

/// What `tauri build` (tauri-cli 2.12, `createUpdaterArtifacts: true`) writes for an AppImage is the AppImage itself plus its `.sig`,
/// not a tar.gz (measured on a real Linux build, gaps-linux-3). The plugin writes those bytes over the installed file and keeps its mode.
#[cfg(target_os = "linux")]
#[test]
fn a_raw_appimage_as_tauri_build_writes_it_replaces_the_installed_one_and_stays_executable() {
    use std::os::unix::fs::PermissionsExt;
    let k = keys();
    let (base, state) = serve();
    let bytes = b"\x7fELF raw appimage, not a gzip: NEW".to_vec();
    {
        let mut s = state.lock().unwrap();
        s.manifest = manifest(&base, "0.2.0", &sign(&k, &bytes, Some("0.2.0")));
        s.archive = bytes.clone();
    }
    let app = app_with(local_config(&base, &k)).unwrap();
    let installed = Installed::new("raw");
    match run(&app, &installed) {
        Outcome::Installed(v) => assert_eq!(v, "0.2.0"),
        Outcome::NoUpdate => panic!("no update was offered"),
        Outcome::CheckFailed(e) | Outcome::DownloadFailed(e) => panic!("{e}"),
    }
    assert_eq!(std::fs::read(installed.exe()).unwrap(), bytes, "the AppImage was not replaced by the downloaded bytes");
    assert_eq!(std::fs::metadata(installed.exe()).unwrap().permissions().mode() & 0o777, 0o755, "the replaced AppImage lost its executable mode");
}

#[test]
fn an_archive_signed_with_another_key_is_refused_and_the_app_is_untouched() {
    let (app, state, _k, installed, base) = rig("0.2.0", Some("0.2.0"));
    let other = keys();
    {
        let mut s = state.lock().unwrap();
        let archive = s.archive.clone();
        s.manifest = manifest(&base, "0.2.0", &sign(&other, &archive, Some("0.2.0")));
    }
    match run(&app, &installed) {
        Outcome::DownloadFailed(e) => assert!(e.to_lowercase().contains("signature") || e.contains("key"), "{e}"),
        _ => panic!("an archive signed by another key was accepted"),
    }
    assert_eq!(installed.content(), "OLD 0.1.0");
}

#[test]
fn a_tampered_archive_is_refused_and_the_app_is_untouched() {
    let (app, state, _k, installed, _base) = rig("0.2.0", Some("0.2.0"));
    {
        let mut s = state.lock().unwrap();
        let last = s.archive.len() - 1;
        s.archive[last] ^= 0x01; // one bit, after signing
    }
    match run(&app, &installed) {
        Outcome::DownloadFailed(e) => assert!(e.to_lowercase().contains("signature"), "{e}"),
        _ => panic!("a tampered archive was accepted"),
    }
    assert_eq!(installed.content(), "OLD 0.1.0");
}

#[test]
fn a_server_that_is_down_or_answers_wrongly_is_an_error_not_an_update() {
    // Nothing listens on the port (the listener is dropped before the check).
    let k = keys();
    let dead = { let l = TcpListener::bind("127.0.0.1:0").unwrap(); format!("http://{}", l.local_addr().unwrap()) };
    let installed = Installed::new("down");
    let app = app_with(local_config(&dead, &k)).unwrap();
    assert!(matches!(run(&app, &installed), Outcome::CheckFailed(_)), "a closed port is a failed check");
    // A manifest that is 404, 500, not JSON, or has no entry for this platform.
    let (base, state) = serve();
    let app = app_with(local_config(&base, &k)).unwrap();
    for (status, body) in [(404u16, "{}"), (500, "{}"), (200, "<html>nope</html>"), (200, r#"{"version":"0.2.0","platforms":{"plan9-mips":{"signature":"x","url":"http://x/y"}}}"#)] {
        { let mut s = state.lock().unwrap(); s.manifest = body.into(); s.manifest_status = Some(status); }
        assert!(matches!(run(&app, &installed), Outcome::CheckFailed(_)), "{status} {body}");
    }
    // The manifest is fine but the archive is gone.
    let bytes = archive("NEW");
    {
        let mut s = state.lock().unwrap();
        s.manifest = manifest(&base, "0.2.0", &sign(&k, &bytes, Some("0.2.0")));
        s.manifest_status = None; s.archive = bytes; s.archive_status = Some(404);
    }
    assert!(matches!(run(&app, &installed), Outcome::DownloadFailed(_)));
    assert_eq!(installed.content(), "OLD 0.1.0");
}

#[test]
fn an_older_or_equal_release_is_never_offered() {
    for version in ["0.0.9", "0.1.0"] {
        let (app, _s, _k, installed, _b) = rig(version, Some(version));
        assert!(matches!(run(&app, &installed), Outcome::NoUpdate), "{version} was offered to 0.1.0");
        assert_eq!(installed.content(), "OLD 0.1.0");
    }
}

#[test]
fn an_old_signed_release_announced_under_a_new_version_is_refused() {
    // The manifest is plain JSON. Whoever can change it can announce 9.9.9 and point at the
    // archive of 0.0.9, which the release key really did sign. Only the signature's own record
    // of its version tells them apart.
    let (app, state, k, installed, base) = rig("9.9.9", Some("0.0.9"));
    let _ = (&state, &k, &base);
    match run(&app, &installed) {
        Outcome::DownloadFailed(e) => assert!(e.to_lowercase().contains("version"), "{e}"),
        Outcome::Installed(v) => panic!("an old release was installed as {v}"),
        _ => panic!("expected the download to be refused"),
    }
    assert_eq!(installed.content(), "OLD 0.1.0");
}

#[test]
fn a_signature_that_does_not_name_its_version_is_refused() {
    // What a CLI older than 2.12 writes. With the shipped requireSignedVersion on, it cannot be
    // used to replay an old release, and so cannot be used for a real one either.
    let (app, _s, _k, installed, _b) = rig("0.2.0", None);
    match run(&app, &installed) {
        Outcome::DownloadFailed(e) => assert!(e.to_lowercase().contains("version"), "{e}"),
        _ => panic!("an unversioned signature was accepted"),
    }
    assert_eq!(installed.content(), "OLD 0.1.0");
}

#[test]
fn a_partial_download_is_refused_and_the_app_is_untouched() {
    let (app, state, _k, installed, _b) = rig("0.2.0", Some("0.2.0"));
    let total = state.lock().unwrap().archive.len();
    // The server promises the whole archive and stops half way.
    state.lock().unwrap().cut_after = Some(total / 2);
    assert!(matches!(run(&app, &installed), Outcome::DownloadFailed(_)), "a cut body with a length was accepted");
    assert_eq!(installed.content(), "OLD 0.1.0");
    // A close-delimited body that stops half way looks complete to HTTP; the signature is what refuses it.
    {
        let mut s = state.lock().unwrap();
        s.cut_after = None; s.cut_without_length = Some(total / 2);
    }
    match run(&app, &installed) {
        Outcome::DownloadFailed(e) => assert!(e.to_lowercase().contains("signature"), "{e}"),
        _ => panic!("a truncated archive was accepted"),
    }
    assert_eq!(installed.content(), "OLD 0.1.0");
    // Cut to nothing.
    state.lock().unwrap().cut_without_length = Some(0);
    assert!(matches!(run(&app, &installed), Outcome::DownloadFailed(_)));
    assert_eq!(installed.content(), "OLD 0.1.0");
}

/// Opt-in (`ICU_TAURI_CLI=<path to tauri-cli 2.12 or later>`): the signature and key come from the
/// real `tauri signer`, not from this file's signer, and the archive is signed the way `tauri
/// build` signs it, with the version it was released as.
#[test]
fn the_real_tauri_signer_output_is_accepted_and_bound_to_its_version() {
    let Some(cli) = std::env::var_os("ICU_TAURI_CLI") else {
        eprintln!("skipped (set ICU_TAURI_CLI to a tauri CLI of 2.12 or later)");
        return;
    };
    let dir = std::env::temp_dir().join(format!("icu-upd-cli-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let tauri = |args: &[&str]| {
        let out = std::process::Command::new(&cli).args(args).output().unwrap();
        assert!(out.status.success(), "{args:?}: {}{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr));
    };
    let key = dir.join("throwaway.key");
    tauri(&["signer", "generate", "--ci", "-p", "", "-w", key.to_str().unwrap()]);
    let pubkey = std::fs::read_to_string(dir.join("throwaway.key.pub")).unwrap().trim().to_string();
    let bytes = archive("NEW");
    let file = dir.join("Copilot.app.tar.gz");
    std::fs::write(&file, &bytes).unwrap();
    let sign_as = |version: &str| -> String {
        let _ = std::fs::remove_file(dir.join("Copilot.app.tar.gz.sig"));
        tauri(&["signer", "sign", "-f", key.to_str().unwrap(), "-p", "", "--app-version", version, file.to_str().unwrap()]);
        std::fs::read_to_string(dir.join("Copilot.app.tar.gz.sig")).unwrap().trim().to_string()
    };
    for (announced, signed_for, installs) in [("0.2.0", "0.2.0", true), ("9.9.9", "0.0.9", false)] {
        let (base, state) = serve();
        { let mut s = state.lock().unwrap(); s.manifest = manifest(&base, announced, &sign_as(signed_for)); s.archive = bytes.clone(); }
        let app = app_with(local_config_for(&base, &pubkey)).unwrap();
        let installed = Installed::new("cli");
        match run(&app, &installed) {
            Outcome::Installed(v) => assert!(installs && v == announced, "{announced} signed for {signed_for} was installed"),
            Outcome::DownloadFailed(e) => assert!(!installs && e.to_lowercase().contains("version"), "{announced}: {e}"),
            _ => panic!("{announced} signed for {signed_for}: no update"),
        }
        if !installs { assert_eq!(installed.content(), "OLD 0.1.0"); }
    }
    let _ = std::fs::remove_dir_all(&dir);
}

// ── .deb: the plugin installs an update of a .deb install with `dpkg -i` ─────────────────────────
//
// The plugin picks that installer from a marker `tauri build` patches into the binary
// (`__TAURI_BUNDLE_TYPE_VAR_UNK` becomes `..._DEB`). A test binary has no marker, so the host
// script (updater-evidence/run-deb.sh) copies it, patches the marker the way the bundler does and
// runs this test as root. It asks for the `linux-<arch>-deb` entry only, so a binary that did not
// take the deb route finds no update at all. The package is a throwaway `icu-upd-fixture`.
#[cfg(target_os = "linux")]
mod deb {
    use super::*;
    use std::process::Command;

    fn build_deb(version: &str) -> Vec<u8> {
        let dir = std::env::temp_dir().join(format!("icu-upd-deb-{}-{version}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("pkg/DEBIAN")).unwrap();
        std::fs::create_dir_all(dir.join("pkg/usr/share/icu-upd-fixture")).unwrap();
        std::fs::write(dir.join("pkg/DEBIAN/control"), format!("Package: icu-upd-fixture\nVersion: {version}\nArchitecture: all\nMaintainer: throwaway <t@example.invalid>\nDescription: updater test fixture\n")).unwrap();
        std::fs::write(dir.join("pkg/usr/share/icu-upd-fixture/version"), version).unwrap();
        let out = dir.join("f.deb");
        let st = Command::new("dpkg-deb").args(["--build", "--root-owner-group"]).arg(dir.join("pkg")).arg(&out).status().unwrap();
        assert!(st.success());
        let b = std::fs::read(&out).unwrap();
        let _ = std::fs::remove_dir_all(&dir);
        b
    }
    fn installed_version() -> String {
        std::fs::read_to_string("/usr/share/icu-upd-fixture/version").unwrap_or_else(|_| "<none>".into())
    }
    fn deb_manifest(base: &str, version: &str, signature: &str) -> String {
        let target = format!("{}-deb", tauri_plugin_updater::target().unwrap());
        json!({"version": version, "notes": "throwaway", "pub_date": "2026-10-04T00:00:00Z",
            "platforms": {target: {"signature": signature, "url": format!("{base}/Copilot.app.tar.gz")}}}).to_string()
    }
    /// A rig whose manifest announces `version` for the deb of `deb_version`, signed for `signed_for`.
    fn rig(version: &str, deb_version: &str, signed_for: &str) -> (tauri::App<Mock>, Arc<Mutex<Serving>>, Keys, String) {
        let k = keys();
        let (base, state) = serve();
        let bytes = build_deb(deb_version);
        { let mut s = state.lock().unwrap(); s.manifest = deb_manifest(&base, version, &sign(&k, &bytes, Some(signed_for))); s.archive = bytes; }
        (app_with(local_config(&base, &k)).unwrap(), state, k, base)
    }
    fn go(app: &tauri::App<Mock>) -> Outcome { run(app, &Installed::new("deb")) }

    #[test]
    fn a_deb_install_is_updated_by_dpkg_and_refuses_what_the_signature_refuses() {
        if std::env::var_os("ICU_UPDATER_DEB").is_none() { eprintln!("skipped (set ICU_UPDATER_DEB=1 on a patched, root run: see updater-evidence/run-deb.sh)"); return; }
        assert_eq!(installed_version(), "0.1.0", "start from fixture 0.1.0 installed with dpkg -i");
        // another key, one flipped bit, an old release announced as new: nothing is installed
        let (app, state, _k, base) = rig("0.2.0", "0.2.0", "0.2.0");
        let other = keys();
        { let mut s = state.lock().unwrap(); let a = s.archive.clone(); s.manifest = deb_manifest(&base, "0.2.0", &sign(&other, &a, Some("0.2.0"))); }
        match go(&app) { Outcome::DownloadFailed(e) => assert!(e.to_lowercase().contains("signature"), "{e}"), _ => panic!("another key was accepted") }
        assert_eq!(installed_version(), "0.1.0");
        let (app, state, _k, _b) = rig("0.2.0", "0.2.0", "0.2.0");
        { let mut s = state.lock().unwrap(); let l = s.archive.len() - 1; s.archive[l] ^= 1; }
        match go(&app) { Outcome::DownloadFailed(e) => assert!(e.to_lowercase().contains("signature"), "{e}"), _ => panic!("a tampered deb was accepted") }
        assert_eq!(installed_version(), "0.1.0");
        let (app, _s, _k, _b) = rig("9.9.9", "0.0.9", "0.0.9");
        match go(&app) { Outcome::DownloadFailed(e) => assert!(e.to_lowercase().contains("version"), "{e}"), _ => panic!("an old release announced as 9.9.9 was accepted") }
        assert_eq!(installed_version(), "0.1.0");
        for v in ["0.0.9", "0.1.0"] {
            let (app, _s, _k, _b) = rig(v, v, v);
            assert!(matches!(go(&app), Outcome::NoUpdate), "{v} was offered to 0.1.0");
            assert_eq!(installed_version(), "0.1.0");
        }
        // the good one: found under the -deb target, verified, installed by dpkg
        let (app, _s, _k, _b) = rig("0.2.0", "0.2.0", "0.2.0");
        match go(&app) { Outcome::Installed(v) => assert_eq!(v, "0.2.0"), Outcome::NoUpdate => panic!("no update under the -deb target: the binary did not take the deb route"), Outcome::CheckFailed(e) | Outcome::DownloadFailed(e) => panic!("{e}") }
        assert_eq!(installed_version(), "0.2.0", "dpkg did not install the update");
    }
}

/// The `.deb` declares `at-spi2-core` (the driver reads the accessibility tree through it) and the `.rpm` built from the same
/// config did not, so dnf would not pull it in on a Fedora or Rocky machine that lacks it. Every Linux
/// package `tauri build` makes must declare it. (gaps-linux-4: the bundled `.rpm` had Requires of only libwebkit2gtk-4.1 and libgtk-3.)
#[test]
fn every_linux_package_declares_the_accessibility_dependency() {
    let cfg: Value = serde_json::from_str(include_str!("../tauri.linux.conf.json")).unwrap();
    for kind in ["deb", "rpm"] {
        let depends = cfg["bundle"]["linux"][kind]["depends"].as_array().unwrap_or_else(|| panic!("bundle.linux.{kind}.depends is missing"));
        assert!(depends.iter().any(|d| d == "at-spi2-core"), "the {kind} does not depend on at-spi2-core: {depends:?}");
    }
}

/// The release binary is built on Ubuntu 24.04 (glibc 2.39) and needs that glibc, which RHEL 9 and Ubuntu 22.04 lack. The `.rpm`'s
/// auto-generated Requires name `libwebkit2gtk-4.1.so.0` and `libgtk-3.so.0` (RHEL 9 has no WebKitGTK 4.1 either) but not glibc,
/// so on a machine whose libraries are too old the package manager must say so by name: a refusal that lists the glibc version
/// (`libc.so.6(GLIBC_2.39)(64bit)` for the rpm, `libc6 (>= 2.39)` for the deb) instead of a binary that fails to start.
/// Both lines move together with the build host's glibc.
#[test]
fn every_linux_package_declares_the_glibc_it_was_built_against() {
    let cfg: Value = serde_json::from_str(include_str!("../tauri.linux.conf.json")).unwrap();
    let has = |kind: &str, want: &str| cfg["bundle"]["linux"][kind]["depends"].as_array().map_or(false, |d| d.iter().any(|x| x == want));
    assert!(has("rpm", "libc.so.6(GLIBC_2.39)(64bit)"), "the rpm does not require glibc 2.39");
    assert!(has("deb", "libc6 (>= 2.39)"), "the deb does not depend on libc6 (>= 2.39)");
}

/// Release-chain acceptance on actual signed package bytes. No install is attempted here:
/// GUI/package-manager lifecycle checks are separate. The input file contains only public
/// artifact paths, signatures and SHA-256 hashes, never a signing key.
#[cfg(target_os = "linux")]
#[test]
#[ignore = "requires ICU_LINUX_RELEASE_ARTIFACTS from the remote signed release build"]
fn linux_release_artifacts_pass_the_updater_and_refuse_tamper_wrong_key_and_version() {
    use sha2::{Digest, Sha256};
    let path = std::env::var("ICU_LINUX_RELEASE_ARTIFACTS").expect("release artifact manifest");
    let release: Value = serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
    let pubkey = shipped()["pubkey"].as_str().unwrap().to_string();
    let version = release["version"].as_str().unwrap();
    assert_eq!(release["pubkey"], pubkey, "probe must use the packaged release key");
    for (target, artifact) in release["artifacts"].as_object().unwrap() {
        let bytes = std::fs::read(artifact["path"].as_str().unwrap()).unwrap();
        let digest = format!("{:x}", Sha256::digest(&bytes));
        assert_eq!(artifact["sha256"], digest);
        let signature = artifact["signature"].as_str().unwrap();
        let other = keys();
        let (base, state) = serve();
        for case in ["valid", "tampered", "wrong-key", "version-mismatch"] {
            let announced = if case == "version-mismatch" { "99.0.0" } else { version };
            let key = if case == "wrong-key" { pubkey_of(&other) } else { pubkey.clone() };
            {
                let mut s = state.lock().unwrap();
                s.archive = bytes.clone();
                if case == "tampered" {
                    let last = s.archive.len() - 1;
                    s.archive[last] ^= 1;
                }
                s.manifest = json!({"version": announced, "platforms": {target: {
                    "signature": signature, "url": format!("{base}/Copilot.app.tar.gz")
                }}}).to_string();
            }
            let app = app_with(local_config_for(&base, &key)).unwrap();
            let updater = app.handle().updater_builder().target(target)
                .timeout(std::time::Duration::from_secs(120)).build().unwrap();
            let result = tauri::async_runtime::block_on(async {
                updater.check().await.unwrap().expect("new release")
                    .download(|_, _| {}, || {}).await
            });
            let outcome = match result {
                Ok(downloaded) => {
                    assert_eq!(case, "valid", "{target}: {case} accepted");
                    assert_eq!(format!("{:x}", Sha256::digest(&downloaded)), digest);
                    "accepted exact bytes".to_string()
                }
                Err(error) => {
                    let error = error.to_string();
                    assert_ne!(case, "valid", "{target}: {error}");
                    let lower = error.to_lowercase();
                    assert!(if case == "version-mismatch" { lower.contains("version") }
                        else { lower.contains("signature") || lower.contains("key") }, "{target}: {error}");
                    error
                }
            };
            println!("RELEASE_CONTROL {}", json!({"target": target, "case": case,
                "sha256": digest, "announced": announced, "result": outcome}));
        }
    }
}
