//! The Windows named pipe half of `icu sky-pipe`: names, the access list, the connection loop on
//! any byte stream (so it runs on every platform), and the live pipe (Windows only).
use super::*;
use crate::skypipe::*;
use crate::tests::layer_with_log;
use crate::winpipe::{default_pipe_name, pipe_path, pipe_sddl};

fn pipe() -> (SkyPipe, tempfile::TempDir) {
    let (l, t, _log) = layer_with_log(false, false);
    (SkyPipe::new(l), t)
}

#[test]
fn pipe_names_are_per_user_and_refuse_anything_that_is_not_a_pipe_name() {
    assert_eq!(default_pipe_name("ICUAudit"), r"\\.\pipe\icu-computeruse-ICUAudit");
    assert_eq!(default_pipe_name(r"a b\c/d"), r"\\.\pipe\icu-computeruse-a_b_c_d", "no separator survives into the name");
    assert_eq!(pipe_path("sky1").unwrap(), r"\\.\pipe\sky1", "a bare name gets the prefix");
    assert_eq!(pipe_path(r"\\.\pipe\sky1").unwrap(), r"\\.\pipe\sky1");
    assert_eq!(pipe_path(r"\\.\PIPE\sky1").unwrap(), r"\\.\pipe\sky1", "the prefix is case-insensitive");
    for bad in ["", r"\\.\pipe\", r"\\.\pipe\a\b", r"C:\temp\x.sock", "/tmp/x.sock", "a:b", "\u{e9}\u{e9}\u{e9}\u{e9}\u{e9}\\x"] {
        assert!(pipe_path(bad).is_err(), "{bad:?}");
    }
}

#[test]
fn the_pipe_names_this_user_and_system_and_nobody_else() {
    let sddl = pipe_sddl("S-1-5-21-1-2-3-1001");
    assert_eq!(sddl, "D:P(A;;GA;;;S-1-5-21-1-2-3-1001)(A;;GA;;;SY)");
    assert!(sddl.starts_with("D:P"), "protected: nothing is inherited");
    for everyone in [";WD)", ";BA)", ";AU)", ";BU)", ";IU)"] {
        assert!(!sddl.contains(everyone), "not Everyone, Administrators, Authenticated Users, Users or Interactive: {everyone}");
    }
}

/// A byte stream that hands out its input `chunk` bytes at a time and records what is written,
/// the way a pipe does: reads return what has arrived, writes go to the peer.
struct Duplex { input: std::io::Cursor<Vec<u8>>, chunk: usize, output: Vec<u8> }
impl std::io::Read for Duplex {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> { let n = buf.len().min(self.chunk); std::io::Read::read(&mut self.input, &mut buf[..n]) }
}
impl std::io::Write for Duplex {
    fn write(&mut self, b: &[u8]) -> std::io::Result<usize> { self.output.extend_from_slice(b); Ok(b.len()) }
    fn flush(&mut self) -> std::io::Result<()> { Ok(()) }
}

fn frames_out(bytes: &[u8]) -> Vec<Value> {
    FrameDecoder::default().push(bytes).unwrap().iter().map(|m| serde_json::from_str(m).unwrap()).collect()
}

#[test]
fn the_connection_loop_answers_each_frame_in_order_even_when_the_bytes_trickle() {
    let (p, _t) = pipe();
    let ping = |id: u64, version: &str| encode_frame(json!({"id": id, "jsonrpc": "2.0", "method": "ping", "params": {"clientApiVersion": version}}).to_string().as_bytes()).unwrap();
    let mut input = ping(1, API_VERSION);
    input.extend(ping(2, "CodexComputerUseIPC-3"));
    input.extend(ping(3, API_VERSION));
    for chunk in [1usize, 3, 7, 4096] {
        let mut stream = Duplex { input: std::io::Cursor::new(input.clone()), chunk, output: vec![] };
        serve_connection(&p, &mut stream);
        let out = frames_out(&stream.output);
        assert_eq!(out.iter().map(|m| m["id"].as_u64().unwrap()).collect::<Vec<_>>(), vec![1, 2, 3], "chunk {chunk}");
        assert_eq!(out[0]["result"]["serverApiVersion"], API_VERSION);
        assert_eq!(out[1]["error"]["code"], -10013, "the version gate");
    }
}

#[test]
fn the_connection_loop_ends_on_an_oversize_frame_and_on_a_message_that_is_not_json() {
    let (p, _t) = pipe();
    let mut big = Duplex { input: std::io::Cursor::new((MAX_FRAME_BYTES as u32 + 1).to_le_bytes().to_vec()), chunk: 4096, output: vec![] };
    serve_connection(&p, &mut big);
    assert!(big.output.is_empty(), "no reply to a frame over 8 MiB");
    let mut junk = Duplex { input: std::io::Cursor::new(encode_frame(b"abc").unwrap()), chunk: 4096, output: vec![] };
    serve_connection(&p, &mut junk);
    assert!(junk.output.is_empty(), "no reply to what is not JSON");
}

#[cfg(windows)]
mod live_pipe {
    use super::*;
    use crate::winpipe::{bind_pipe, serve_pipe};
    use std::io::{Read, Write};

    fn unique(tag: &str) -> String { format!(r"\\.\pipe\icu-test-{tag}-{}", std::process::id()) }

    fn exchange(c: &mut std::fs::File, msg: Value) -> Value {
        c.write_all(&encode_frame(msg.to_string().as_bytes()).unwrap()).unwrap();
        let mut d = FrameDecoder::default();
        let mut buf = [0u8; 4096];
        loop {
            let n = c.read(&mut buf).expect("the server answers");
            assert!(n > 0, "the server hung up");
            if let Some(m) = d.push(&buf[..n]).unwrap().pop() { return serde_json::from_str(&m).unwrap(); }
        }
    }

    fn open(name: &str) -> std::io::Result<std::fs::File> {
        for _ in 0..50 {
            match std::fs::OpenOptions::new().read(true).write(true).open(name) {
                Ok(f) => return Ok(f),
                Err(e) if e.kind() == std::io::ErrorKind::NotFound || e.raw_os_error() == Some(231) => std::thread::sleep(std::time::Duration::from_millis(100)),
                Err(e) => return Err(e),
            }
        }
        std::fs::OpenOptions::new().read(true).write(true).open(name)
    }

    #[test]
    fn a_client_pings_and_requests_over_a_real_named_pipe_and_two_clients_at_once_both_work() {
        let name = unique("rt");
        let (l, _t, _) = layer_with_log(false, false);
        let pipe = std::sync::Arc::new(SkyPipe::new(l));
        { let (pipe, name) = (pipe.clone(), name.clone()); std::thread::spawn(move || { let _ = serve_pipe(pipe, &name); }); }
        let mut a = open(&name).unwrap();
        // no retry on busy here: a client that finds every instance busy is the bug
        let mut b = std::fs::OpenOptions::new().read(true).write(true).open(&name).expect("a second client right after the first connected");
        let mut c3 = open(&name).expect("a third client right after the second (retrying on busy, as Codex's client does)");
        let ping = |id: u64| json!({"id": id, "jsonrpc": "2.0", "method": "ping", "params": {"clientApiVersion": API_VERSION}});
        assert_eq!(exchange(&mut a, ping(1))["result"]["serverApiVersion"], API_VERSION);
        assert_eq!(exchange(&mut b, ping(2))["result"]["serverApiVersion"], API_VERSION);
        let apps = exchange(&mut a, json!({"id": 3, "jsonrpc": "2.0", "method": "request", "params": {"clientApiVersion": API_VERSION, "request": {}, "requestType": "ComputerUseIPCListAppsRequest"}}));
        assert!(apps["result"].as_array().is_some_and(|x| !x.is_empty()), "{apps}");
        drop(a);
        assert_eq!(exchange(&mut b, ping(4))["id"], 4, "one client leaving does not end the other's connection");
        assert_eq!(exchange(&mut c3, ping(6))["id"], 6);
        let mut c = open(&name).expect("a client after another left");
        assert_eq!(exchange(&mut c, ping(5))["id"], 5);
    }

    #[test]
    fn a_pipe_that_already_exists_under_the_name_is_not_served_over() {
        let name = unique("squat");
        let squatter = bind_pipe(&name, true).expect("the first instance");
        let err = bind_pipe(&name, true).err().expect("a second first-instance is refused");
        assert_eq!(err.kind(), std::io::ErrorKind::AddrInUse, "{err}");
        // serve_pipe takes the same road: it must not come up on a name somebody else holds
        let (l, _t, _) = layer_with_log(false, false);
        let served = serve_pipe(std::sync::Arc::new(SkyPipe::new(l)), &name);
        assert_eq!(served.err().map(|e| e.kind()), Some(std::io::ErrorKind::AddrInUse));
        drop(squatter);
    }
}
