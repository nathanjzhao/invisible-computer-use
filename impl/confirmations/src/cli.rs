//! The crate's command-line verbs as a library entry point, so the `icu` binary can host
//! them (`icu confirmations ...`) and the standalone binary stays a thin shim.
//! CLI over the confirmations layer, for the conformance harness and evidence runs.
//!
//!   confirmations policy [--platform windows|macos]        print the verbatim policy text
//!   confirmations mode <tag>                               print the mode title for a category tag
//!   confirmations decide <tag> [--pre-approved user|third-party]   HandOff | Confirm | Proceed
//!   confirmations elicit --bundle-id B --display-name N --risk R [--persistent] [--tool T] [--call-id C] [--subtitle S]
//!   confirmations resolve --display-name N --result JSON   -> session | always (exit 6 with the refusal if not accepted)
//!   confirmations store --session FILE --always FILE (allowed <bid> | approve <bid> session|always | list)

use crate::*;
use std::io::Write;

fn arg(a: &[String], flag: &str) -> Option<String> {
    a.iter().position(|x| x == flag).and_then(|i| a.get(i + 1).cloned())
}
fn has(a: &[String], flag: &str) -> bool { a.iter().any(|x| x == flag) }
fn fail(msg: &str, code: i32) -> ! { eprintln!("[confirmations] {msg}"); std::process::exit(code); }

pub fn run(a: Vec<String>) {
    match a.get(1).map(String::as_str) {
        Some("policy") => {
            let text = match arg(&a, "--platform").as_deref() {
                None | Some("windows") => POLICY_WINDOWS,
                Some("macos") => POLICY_MACOS,
                Some(p) => fail(&format!("unknown platform {p:?}"), 2),
            };
            std::io::stdout().write_all(text.as_bytes()).unwrap(); // verbatim bytes
        }
        Some("mode") => {
            let tag = a.get(2).unwrap_or_else(|| fail("usage: confirmations mode <tag>", 2));
            let m = mode_for(tag);
            println!("{}) {}", m.number(), m.title());
        }
        Some("decide") => {
            let tag = a.get(2).unwrap_or_else(|| fail("usage: confirmations decide <tag> [--pre-approved user|third-party]", 2));
            let pa = match arg(&a, "--pre-approved").as_deref() {
                None => None,
                Some("user") => Some(InstructionSource::UserAuthored),
                Some("third-party") => Some(InstructionSource::ThirdPartyContent),
                Some(x) => fail(&format!("unknown instruction source {x:?}"), 2),
            };
            println!("{:?}", decide(tag, pa));
        }
        Some("elicit") => {
            let t = AppTarget {
                bundle_identifier: arg(&a, "--bundle-id").unwrap_or_else(|| fail("--bundle-id required", 2)),
                display_name: arg(&a, "--display-name").unwrap_or_else(|| fail("--display-name required", 2)),
                risk: arg(&a, "--risk").unwrap_or_else(|| fail("--risk required", 2)),
                warning_subtitle: arg(&a, "--subtitle"),
            };
            let tool = arg(&a, "--tool").unwrap_or_else(|| "get_window_state".into());
            let call_id = arg(&a, "--call-id");
            println!("{}", serde_json::to_string_pretty(&elicitation(&t, has(&a, "--persistent"), &tool, call_id.as_deref())).unwrap());
        }
        Some("resolve") => {
            let name = arg(&a, "--display-name").unwrap_or_else(|| fail("--display-name required", 2));
            let res: serde_json::Value = serde_json::from_str(&arg(&a, "--result").unwrap_or_else(|| fail("--result JSON required", 2)))
                .unwrap_or_else(|e| fail(&e.to_string(), 2));
            if is_persisted_state(&res) { eprintln!("[confirmations] persisted-state result: do not record or emit telemetry"); }
            match resolve_elicitation(&res, &name) {
                Ok(Persist::Session) => println!("session"),
                Ok(Persist::Always) => println!("always"),
                Err(e) => fail(&e, 6),
            }
        }
        Some("store") => {
            let s = AppApprovalStore::new(
                arg(&a, "--session").unwrap_or_else(|| fail("--session FILE required", 2)),
                arg(&a, "--always").unwrap_or_else(|| fail("--always FILE required", 2)),
            );
            let rest: Vec<&String> = a.iter().skip(2).filter(|x| !x.starts_with("--")).collect();
            // strip the two flag values from the positional view
            let sess = s.session_path.display().to_string();
            let alw = s.always_path.display().to_string();
            let pos: Vec<&str> = rest.iter().map(|x| x.as_str()).filter(|x| *x != sess && *x != alw).collect();
            match pos.as_slice() {
                ["allowed", bid] => { println!("{}", s.is_allowed(bid)); }
                ["approve", bid, scope] => {
                    let p = match *scope { "session" => Persist::Session, "always" => Persist::Always, x => fail(&format!("scope must be session|always, got {x:?}"), 2) };
                    s.approve(bid, p).unwrap_or_else(|e| fail(&e, 1));
                    println!("ok");
                }
                ["list"] => {
                    let sess = s.session_allowed().unwrap_or_else(|e| fail(&e, 1));
                    let alw = s.always_allowed().unwrap_or_else(|e| fail(&e, 1));
                    println!("{}", serde_json::to_string_pretty(&serde_json::json!({"session": sess, "always": alw})).unwrap());
                }
                _ => fail("usage: confirmations store --session FILE --always FILE (allowed <bid> | approve <bid> session|always | list)", 2),
            }
        }
        _ => fail("usage: confirmations <policy|mode|decide|elicit|resolve|store> ...", 2),
    }
}
