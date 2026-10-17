mod common;
use common::Fixture;
use serde_json::{Value, json};
use std::{
    io::{BufRead, BufReader, Write},
    process::{Child, ChildStdin, Command, Stdio},
    sync::mpsc::{self, Receiver},
    time::{Duration, Instant},
};

struct Client {
    child: Child,
    input: Option<ChildStdin>,
    output: Receiver<Value>,
    events: Vec<Value>,
}
impl Client {
    fn new(f: &Fixture, forms: bool) -> Self {
        let mut child = Command::new(env!("CARGO_BIN_EXE_icu-messages"))
            .args(["--stdio", "--database"])
            .arg(f.dir.path().join("chat.db"))
            .arg("--state-dir")
            .arg(f.dir.path().join("approvals"))
            .args(["--time-zone", "UTC"])
            .env_clear()
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()
            .unwrap();
        let input = child.stdin.take();
        let output = child.stdout.take().unwrap();
        let (tx, rx) = mpsc::channel();
        std::thread::spawn(move || {
            for line in BufReader::new(output).lines() {
                let Ok(line) = line else { break };
                let value = serde_json::from_str(&line).unwrap();
                if tx.send(value).is_err() {
                    break;
                }
            }
        });
        let mut client = Self {
            child,
            input,
            output: rx,
            events: Vec::new(),
        };
        let mut capabilities = json!({"experimental":{"icuTurnLifecycle":true}});
        if forms {
            capabilities["elicitation"] = json!({});
        }
        client.send(json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-06-18","capabilities":capabilities,"clientInfo":{"name":"synthetic-host","version":"1"}}}));
        assert_eq!(
            client.response(1)["result"]["protocolVersion"],
            "2025-06-18"
        );
        client.send(json!({"jsonrpc":"2.0","method":"notifications/initialized"}));
        client
    }
    fn send(&mut self, value: Value) {
        let input = self.input.as_mut().unwrap();
        writeln!(input, "{value}").unwrap();
        input.flush().unwrap();
    }
    fn read(&mut self) -> Value {
        let value = self
            .output
            .recv_timeout(Duration::from_secs(8))
            .expect("MCP response timeout");
        self.events.push(value.clone());
        value
    }
    fn response(&mut self, id: i64) -> Value {
        let start = Instant::now();
        loop {
            assert!(start.elapsed() < Duration::from_secs(10));
            let value = self.read();
            if value["id"] == id && value.get("method").is_none() {
                return value;
            }
        }
    }
    fn call(&mut self, id: i64, name: &str, args: Value) {
        self.send(json!({"jsonrpc":"2.0","id":id,"method":"tools/call","params":{"name":name,"arguments":args}}));
    }
    fn approve(&mut self, request: &Value, scope: &str) {
        self.send(json!({"jsonrpc":"2.0","id":request["id"],"result":{"action":"accept","content":{"scope":scope,"chat_1":true}}}));
    }
    fn close(&mut self) {
        self.input.take();
        let start = Instant::now();
        loop {
            if let Some(status) = self.child.try_wait().unwrap() {
                assert!(status.success());
                break;
            }
            if start.elapsed() > Duration::from_secs(3) {
                let _ = self.child.kill();
                let _ = self.child.wait();
                panic!("MCP server did not stop after stdin closed");
            }
            std::thread::sleep(Duration::from_millis(10));
        }
    }
}
impl Drop for Client {
    fn drop(&mut self) {
        self.input.take();
        let start = Instant::now();
        while self.child.try_wait().ok().flatten().is_none()
            && start.elapsed() < Duration::from_secs(2)
        {
            std::thread::sleep(Duration::from_millis(10));
        }
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}
fn fixture() -> Fixture {
    let f = Fixture::new();
    f.add(
        1,
        "Sensitive synthetic body",
        icu_messages::dates::parse("2026-10-01T00:00:00Z").unwrap(),
    );
    f
}

#[test]
fn actual_stdio_server_lists_schemas_and_requires_a_capable_approval_host() {
    let f = fixture();
    let mut client = Client::new(&f, false);
    client.send(json!({"jsonrpc":"2.0","id":2,"method":"tools/list"}));
    assert_eq!(
        client.response(2)["result"]["tools"]
            .as_array()
            .unwrap()
            .len(),
        6
    );
    client.call(3, "find_chats", json!({}));
    assert_eq!(client.response(3)["result"]["isError"], false);
    client.call(4, "read_messages", json!({"chat_guid":"direct"}));
    let response = client.response(4);
    assert_eq!(
        response["result"]["_meta"]["icu.error_code"],
        "messagesApprovalUnavailable"
    );
    assert!(!response.to_string().contains("Sensitive synthetic body"));
    client.call(
        5,
        "read_messages",
        json!({"chat_guid":"direct","approved":true}),
    );
    assert_eq!(
        client.response(5)["result"]["_meta"]["icu.error_code"],
        "invalidArguments"
    );
    client.call(6, "approve", json!({}));
    assert_eq!(client.response(6)["error"]["code"], -32602);
    assert!(
        !client
            .events
            .iter()
            .any(|e| e["method"] == "elicitation/create")
    );
    client.close();
}
#[test]
fn approval_form_releases_contents_and_persistent_permission_is_revocable_by_cli() {
    let f = fixture();
    let mut client = Client::new(&f, true);
    client.call(2, "read_messages", json!({"chat_guid":"direct"}));
    let form = client.read();
    assert_eq!(form["method"], "elicitation/create");
    assert!(!form.to_string().contains("Sensitive synthetic body"));
    assert_eq!(
        form["params"]["requestedSchema"]["properties"]["chat_1"]["title"],
        "Fixture direct"
    );
    client.approve(&form, "once");
    let result = client.response(2);
    assert_eq!(
        result["result"]["structuredContent"]["messages"][0]["body"],
        "Sensitive synthetic body"
    );
    client.call(3, "read_messages", json!({"chat_guid":"direct"}));
    let form = client.read();
    assert_eq!(form["method"], "elicitation/create");
    client.approve(&form, "always");
    assert_eq!(client.response(3)["result"]["isError"], false);
    client.close();
    let mut client = Client::new(&f, false);
    client.call(2, "read_messages", json!({"chat_guid":"direct"}));
    assert_eq!(client.response(2)["result"]["isError"], false);
    let revoke = Command::new(env!("CARGO_BIN_EXE_icu-messages"))
        .args([
            "permissions",
            "revoke",
            "--chat-guid",
            "direct",
            "--state-dir",
        ])
        .arg(f.dir.path().join("approvals"))
        .output()
        .unwrap();
    assert!(revoke.status.success());
    assert_eq!(
        serde_json::from_slice::<Value>(&revoke.stdout).unwrap()["revoked"],
        1
    );
    client.call(3, "read_messages", json!({"chat_guid":"direct"}));
    assert_eq!(
        client.response(3)["result"]["_meta"]["icu.error_code"],
        "messagesApprovalUnavailable"
    );
    client.close();
}
#[test]
fn late_elicitation_reply_after_cancel_cannot_grant_and_host_turn_can_recover() {
    let f = fixture();
    let mut client = Client::new(&f, true);
    client.call(2, "read_messages", json!({"chat_guid":"direct"}));
    let form = client.read();
    assert_eq!(form["method"], "elicitation/create");
    client
        .send(json!({"jsonrpc":"2.0","method":"notifications/cancelled","params":{"requestId":2}}));
    client.approve(&form, "always");
    assert_eq!(
        client.response(2)["result"]["_meta"]["icu.error_code"],
        "messagesRequestCanceled"
    );
    client.call(3, "read_messages", json!({"chat_guid":"direct"}));
    assert_eq!(
        client.response(3)["result"]["_meta"]["icu.error_code"],
        "messagesApprovalCanceled"
    );
    client.send(json!({"jsonrpc":"2.0","method":"notifications/icu/turn_started","params":{"turnId":"next-user-turn"}}));
    client.call(4, "read_messages", json!({"chat_guid":"direct"}));
    let form = client.read();
    assert_eq!(form["method"], "elicitation/create");
    client.approve(&form, "once");
    assert_eq!(client.response(4)["result"]["isError"], false);
    assert!(
        icu_messages::consent::Store::open(f.dir.path().join("approvals"))
            .unwrap()
            .persistent_destinations()
            .unwrap()
            .is_empty()
    );
    client.close();
}
#[test]
fn search_can_return_an_empty_permission_filtered_page_with_a_live_cursor() {
    let f = fixture();
    f.add(
        2,
        "Sensitive withheld body",
        icu_messages::dates::parse("2026-10-02T00:00:00Z").unwrap(),
    );
    let mut client = Client::new(&f, true);
    client.call(2, "search_messages", json!({"text":"Sensitive","limit":1}));
    let form = client.read();
    client.send(json!({"jsonrpc":"2.0","id":form["id"],"result":{"action":"decline"}}));
    let first = client.response(2)["result"]["structuredContent"].clone();
    assert_eq!(first["messages"], json!([]));
    assert_eq!(first["permission_filtered"], true);
    assert!(!first.to_string().contains("withheld"));
    client.call(
        3,
        "search_messages",
        json!({"text":"Sensitive","limit":1,"cursor":first["next_cursor"]}),
    );
    let form = client.read();
    client.approve(&form, "once");
    assert_eq!(
        client.response(3)["result"]["structuredContent"]["messages"][0]["body"],
        "Sensitive synthetic body"
    );
    client.close();
}
#[test]
fn untrusted_tool_calls_cannot_answer_a_host_approval_and_disconnect_cleans_up() {
    let f = fixture();
    let mut client = Client::new(&f, true);
    client.call(2, "read_messages", json!({"chat_guid":"direct"}));
    let form = client.read();
    client.call(3, "approve", json!({"id":form["id"],"approved":true}));
    client.send(json!({"jsonrpc":"2.0","id":"not-the-pending-form","result":{"action":"accept","content":{"chat_1":true,"scope":"always"}}}));
    assert!(
        client
            .output
            .recv_timeout(Duration::from_millis(100))
            .is_err(),
        "a model call resolved the host approval"
    );
    client.send(json!({"jsonrpc":"2.0","id":form["id"],"result":{"action":"decline"}}));
    assert_eq!(
        client.response(2)["result"]["_meta"]["icu.error_code"],
        "messagesReadDeclined"
    );
    assert_eq!(client.response(3)["error"]["code"], -32602);
    client.close();
    let mut client = Client::new(&f, true);
    client.call(2, "read_messages", json!({"chat_guid":"direct"}));
    assert_eq!(client.read()["method"], "elicitation/create");
    client.close();
    assert!(
        icu_messages::consent::Store::open(f.dir.path().join("approvals"))
            .unwrap()
            .persistent_destinations()
            .unwrap()
            .is_empty()
    );
}
#[test]
fn approval_timeout_is_terminal_and_oversized_input_is_bounded() {
    use std::os::unix::net::UnixStream;
    let f = fixture();
    let engine = f.engine();
    let (mut client, server) = UnixStream::pair().unwrap();
    client
        .set_read_timeout(Some(Duration::from_secs(3)))
        .unwrap();
    let server_output = server.try_clone().unwrap();
    let server = std::thread::spawn(move || {
        icu_messages::mcp::serve(
            server,
            server_output,
            engine,
            icu_messages::mcp::Options {
                approval_timeout: Duration::from_millis(60),
            },
        )
    });
    let mut reader = BufReader::new(client.try_clone().unwrap());
    let read = |reader: &mut BufReader<UnixStream>| {
        let mut line = String::new();
        reader.read_line(&mut line).unwrap();
        serde_json::from_str::<Value>(&line).unwrap()
    };
    writeln!(client,"{}",json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-06-18","capabilities":{"elicitation":{}},"clientInfo":{"name":"fixture"}}})).unwrap();
    read(&mut reader);
    writeln!(
        client,
        "{}",
        json!({"jsonrpc":"2.0","method":"notifications/initialized"})
    )
    .unwrap();
    writeln!(client,"{}",json!({"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"read_messages","arguments":{"chat_guid":"direct"}}})).unwrap();
    let form = read(&mut reader);
    assert_eq!(form["method"], "elicitation/create");
    assert!(
        !form["params"]["requestedSchema"]["properties"]["scope"]["enum"]
            .as_array()
            .unwrap()
            .contains(&json!("turn"))
    );
    let result = loop {
        let r = read(&mut reader);
        if r["id"] == 2 {
            break r;
        }
    };
    assert_eq!(
        result["result"]["_meta"]["icu.error_code"],
        "messagesApprovalCanceled"
    );
    writeln!(client,"{}",json!({"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"read_messages","arguments":{"chat_guid":"direct"}}})).unwrap();
    assert_eq!(
        read(&mut reader)["result"]["_meta"]["icu.error_code"],
        "messagesApprovalCanceled"
    );
    client.shutdown(std::net::Shutdown::Both).unwrap();
    server.join().unwrap().unwrap();
    let mut client = Client::new(&f, false);
    let oversized = vec![b'x'; 1024 * 1024 + 1];
    let _ = client.input.as_mut().unwrap().write_all(&oversized);
    assert_eq!(client.read()["error"]["code"], -32700);
    client.close();
}

#[test]
fn read_image_returns_a_real_mcp_image_block_after_approved_message_lookup() {
    use base64::Engine;
    let f = fixture();
    f.attach("image-id", &f.png());
    let mut client = Client::new(&f, true);
    client.call(2, "read_messages", json!({"chat_guid":"direct"}));
    let form = client.read();
    client.approve(&form, "session");
    assert_eq!(client.response(2)["result"]["isError"], false);
    client.call(3, "read_image", json!({"id":"image-id"}));
    let result = client.response(3);
    assert_eq!(result["result"]["isError"], false);
    let image = &result["result"]["content"][0];
    assert_eq!(image["type"], "image");
    assert_eq!(image["mimeType"], "image/png");
    let data = base64::engine::general_purpose::STANDARD
        .decode(image["data"].as_str().unwrap())
        .unwrap();
    let decoder = png::Decoder::new(std::io::Cursor::new(data));
    let reader = decoder.read_info().unwrap();
    assert_eq!(reader.info().width, 2);
    assert_eq!(reader.info().height, 1);
    client.close();
}

#[test]
fn cancellation_recovers_when_another_process_holds_approval_storage() {
    use fs2::FileExt;
    let f = fixture();
    let mut client = Client::new(&f, true);
    let lock = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(f.dir.path().join("approvals/approvals.lock"))
        .unwrap();
    lock.lock_exclusive().unwrap();
    client.call(2, "read_messages", json!({"chat_guid":"direct"}));
    std::thread::sleep(Duration::from_millis(40));
    client
        .send(json!({"jsonrpc":"2.0","method":"notifications/cancelled","params":{"requestId":2}}));
    let start = Instant::now();
    let result = client.response(2);
    assert!(start.elapsed() < Duration::from_secs(2));
    assert_eq!(
        result["result"]["_meta"]["icu.error_code"],
        "messagesRequestCanceled"
    );
    assert!(!result.to_string().contains("Sensitive synthetic body"));
    drop(lock);
    client.close();
}
