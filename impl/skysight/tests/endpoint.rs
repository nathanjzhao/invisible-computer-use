//! Live: drive ModelSummarizer against a real in-process OpenAI-compatible /chat/completions
//! server over a real TCP socket, with no Anthropic and no key. Proves the endpoint config is
//! honored end to end (curl posts the OpenAI shape and parses its response), so a local model
//! on the host or cloud box can back the summarizer without an Anthropic key.
use skysight::*;
use std::io::{Read, Write};
use std::net::TcpListener;
use std::sync::{Arc, Mutex};

#[test]
fn model_summarizer_drives_a_non_anthropic_openai_endpoint_over_a_real_socket() {
    // the valid summary document the mock "model" returns in the OpenAI response shape
    let good = ReferenceSummarizer
        .summarize_10min(&TenMinInput {
            window: Window { cadence: Cadence::TenMin, start: 0 },
            events: &[EventRecord { ts: format_ts(1), kind: "app_focus".into(), bundle_id: Some("com.apple.clock".into()), ..Default::default() }],
            prior: &[],
        })
        .unwrap()
        .render();

    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    let response_body = serde_json::json!({"choices": [{"message": {"role": "assistant", "content": good}}]}).to_string();
    let captured = Arc::new(Mutex::new(String::new()));
    let cap = captured.clone();
    let server = std::thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        // curl may send the headers and the body in separate writes: read until the whole
        // content-length body is in (a single read made this test flaky)
        let mut got = Vec::new();
        let mut buf = vec![0u8; 1 << 16];
        loop {
            let n = stream.read(&mut buf).unwrap();
            got.extend_from_slice(&buf[..n]);
            let text = String::from_utf8_lossy(&got).to_string();
            let done = text.split_once("\r\n\r\n").is_some_and(|(head, body)| {
                let len = head.lines().find_map(|l| l.to_ascii_lowercase().strip_prefix("content-length:").map(|v| v.trim().parse::<usize>().unwrap_or(0)));
                body.len() >= len.unwrap_or(0)
            });
            if n == 0 || done { break; }
        }
        *cap.lock().unwrap() = String::from_utf8_lossy(&got).to_string();
        let resp = format!(
            "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{}",
            response_body.len(),
            response_body
        );
        stream.write_all(resp.as_bytes()).unwrap();
        stream.flush().ok();
    });

    // point the summarizer at the mock endpoint; no Anthropic, no key (a local model)
    for k in ["ANTHROPIC_API_KEY", "ICU_SUMMARIZER_API_KEY", "OPENAI_API_KEY"] {
        std::env::remove_var(k);
    }
    std::env::set_var("ICU_SUMMARIZER_BASE_URL", format!("http://{addr}/v1"));
    std::env::set_var("ICU_SUMMARIZER_MODEL", "mock-local");

    let summarizer = ModelSummarizer::from_env().expect("ICU_SUMMARIZER_BASE_URL configures the backend");
    let out = summarizer
        .summarize_10min(&TenMinInput { window: Window { cadence: Cadence::TenMin, start: 0 }, events: &[EventRecord { ts: format_ts(1), kind: "app_focus".into(), bundle_id: Some("com.apple.clock".into()), ..Default::default() }], prior: &[] })
        .unwrap();
    assert_eq!(out.frontmatter.title, "Activity in com.apple.clock");

    server.join().unwrap();
    let request = captured.lock().unwrap().clone();
    assert!(request.starts_with("POST /v1/chat/completions "), "wrong path over the socket:\n{request}");
    assert!(request.contains("\"role\":\"system\""), "not the OpenAI messages shape:\n{request}");
    let lower = request.to_lowercase();
    assert!(!lower.contains("x-api-key") && !lower.contains("anthropic"), "must not use Anthropic auth/version headers");
    assert!(!lower.contains("authorization:"), "a keyless local endpoint must send no auth header");

    std::env::remove_var("ICU_SUMMARIZER_BASE_URL");
    std::env::remove_var("ICU_SUMMARIZER_MODEL");
}
