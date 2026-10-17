//! memory_summarize through the public Layer with a model endpoint configured: the
//! result must name the summarizer that actually wrote the file. Its own test binary,
//! because it sets process environment the summarizer reads.
use icu::cua::{CuaError, UnavailableCua};
use icu::{Config, Layer};
use serde_json::json;
use std::io::{Read, Write};

#[test]
fn memory_summarize_reports_the_summarizer_that_ran() {
    let tmp = tempfile::tempdir().unwrap();
    std::env::set_var("ICU_HOME", tmp.path());
    std::env::set_var("ICU_MEMORY_ROOT", tmp.path().join("memory"));
    let layer = Layer::with_cua(Config::from_env(), Box::new(UnavailableCua(CuaError { message: "no driver".into(), structured: None })));
    let events = json!([{"ts": "2026-09-11T13:41:00Z", "kind": "app_focus", "bundle_id": "com.apple.Terminal"}]);

    // A configured endpoint that is never contacted: model:false must use the reference.
    std::env::set_var("ICU_SUMMARIZER_BASE_URL", "http://127.0.0.1:9/v1");
    let r = layer.dispatch("memory_summarize", &json!({"window": "2026-09-11T13:41:00Z", "events": events, "model": false})).unwrap();
    assert_eq!(r["summarizer"], "reference", "{r}");

    // With the model allowed, a real HTTP endpoint writes the summary and is reported.
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    let doc = "---\ntitle: Terminal work\ndescription: You worked in a terminal.\napplications: [com.apple.Terminal]\n---\n\n## Memory summary\n\nThe user worked in Terminal.\n\n### Relevant prior context\n\nNone.\n\n### Important non-obvious context about the user\n\nNone.\n\n## Recording summary\n\nThe user had com.apple.Terminal in view\n\n## Citations\n\n";
    let body = json!({"choices": [{"message": {"role": "assistant", "content": doc}}]}).to_string();
    let server = std::thread::spawn(move || {
        let (mut s, _) = listener.accept().unwrap();
        let mut buf = vec![0u8; 1 << 17];
        let _ = s.read(&mut buf).unwrap();
        write!(s, "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}", body.len()).unwrap();
    });
    std::env::set_var("ICU_SUMMARIZER_BASE_URL", format!("http://{addr}/v1"));
    let r = layer.dispatch("memory_summarize", &json!({"window": "2026-09-11T13:51:00Z", "events": [{"ts": "2026-09-11T13:51:00Z", "kind": "app_focus", "bundle_id": "com.apple.Terminal"}]})).unwrap();
    server.join().unwrap();
    assert_eq!(r["summarizer"], "model", "{r}");
    assert!(r["file"].as_str().unwrap().ends_with("-10min-terminal-work.md"), "{r}");
    std::env::remove_var("ICU_SUMMARIZER_BASE_URL");
}

/// A memory folder an older build left 0755/0644 is brought to 0700/0600 by the next memory
/// tool call, and whatever that call writes is 0600.
#[cfg(unix)]
#[test]
fn memory_tools_tighten_an_older_root_and_write_private_files() {
    use std::os::unix::fs::PermissionsExt;
    let mode = |p: &std::path::Path| std::fs::metadata(p).unwrap().permissions().mode() & 0o777;
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("memory");
    std::fs::create_dir(&root).unwrap();
    std::fs::set_permissions(&root, std::fs::Permissions::from_mode(0o755)).unwrap();
    let old = root.join("events.jsonl");
    std::fs::write(&old, "").unwrap();
    std::fs::set_permissions(&old, std::fs::Permissions::from_mode(0o644)).unwrap();
    let mut cfg = Config::from_env();
    cfg.memory_root = root.clone();
    let layer = Layer::with_cua(cfg, Box::new(UnavailableCua(CuaError { message: "no driver".into(), structured: None })));

    layer.dispatch("memory_list", &json!({})).unwrap();
    assert_eq!((mode(&root), mode(&old)), (0o700, 0o600), "memory_list tightens the folder it opens");

    let r = layer.dispatch("memory_summarize", &json!({"window": "2026-09-11T14:01:00Z", "model": false,
        "events": [{"ts": "2026-09-11T14:01:00Z", "kind": "app_focus", "bundle_id": "com.apple.Terminal"}]})).unwrap();
    assert_eq!(mode(std::path::Path::new(r["path"].as_str().unwrap())), 0o600, "{r}");
    let c = layer.dispatch("memory_consolidate", &json!({})).unwrap();
    for k in ["memory_md", "user_profile"] {
        assert_eq!(mode(std::path::Path::new(c[k].as_str().unwrap())), 0o600, "{k}: {c}");
    }
}
