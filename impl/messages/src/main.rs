use icu_messages::{
    Error, Result,
    consent::{Operation, Store},
    contract::{AddressesOnly, Contacts},
    engine::Engine,
    mcp,
    worker::Worker,
};
use std::{ffi::OsString, path::PathBuf};

fn main() {
    let args: Vec<OsString> = std::env::args_os().skip(1).collect();
    if args.len() == 1 && args[0] == "--query-worker" {
        std::process::exit(icu_messages::worker::entry());
    }
    if args.is_empty() || args.iter().any(|s| s == "--help" || s == "-h") {
        help();
        std::process::exit(if args.is_empty() { 2 } else { 0 });
    }
    if let Err(error) = run(args) {
        eprintln!("{error}");
        std::process::exit(1);
    }
}
fn help() {
    println!(
        "Copilot Messages\n\nUsage:\n  icu-messages messages mcp [options]\n  icu-messages --stdio [options]\n  icu-messages contacts status|authorize [--contacts-helper PATH]\n  icu-messages delivery status|authorize [--delivery-helper PATH]\n  icu-messages sends pending [--state-dir PATH]\n  icu-messages sends reconcile --id ID --outcome sent|not_sent [--state-dir PATH]\n  icu-messages sends forget-attachments --id ID [--state-dir PATH]\n  icu-messages permissions list [--state-dir PATH]\n  icu-messages permissions revoke --chat-guid GUID [--operation read|send] [--state-dir PATH]\n\nOptions:\n  --database PATH   Absolute Messages database path\n  --state-dir PATH  Private approval storage directory\n  --time-zone ZONE  IANA calendar zone (defaults to the system zone)\n  --contacts-helper PATH  Use the native Contacts helper with an exported database\n\nRead approvals use MCP host forms. Hosts without forms cannot approve new content reads.\nNative delivery requires macOS Messages and Automation consent. Unknown outcomes require explicit reconciliation before retrying."
    );
}
fn bad() -> Error {
    Error::new(
        "invalidArguments",
        "Invalid command line; use --help for usage",
    )
}
fn run(mut args: Vec<OsString>) -> Result<()> {
    let mode = if args.first().is_some_and(|a| a == "--stdio") {
        args.remove(0);
        "mcp"
    } else if args.first().is_some_and(|a| a == "messages")
        && args.get(1).is_some_and(|a| a == "mcp")
    {
        args.drain(..2);
        "mcp"
    } else if args.first().is_some_and(|a| a == "permissions")
        && args.get(1).is_some_and(|a| a == "list")
    {
        args.drain(..2);
        "list"
    } else if args.first().is_some_and(|a| a == "permissions")
        && args.get(1).is_some_and(|a| a == "revoke")
    {
        args.drain(..2);
        "revoke"
    } else if args.first().is_some_and(|a| a == "contacts")
        && args
            .get(1)
            .is_some_and(|a| a == "status" || a == "authorize")
    {
        let mode = if args[1] == "status" {
            "contacts-status"
        } else {
            "contacts-authorize"
        };
        args.drain(..2);
        mode
    } else if args.first().is_some_and(|a| a == "delivery")
        && args
            .get(1)
            .is_some_and(|a| a == "status" || a == "authorize")
    {
        let mode = if args[1] == "status" {
            "delivery-status"
        } else {
            "delivery-authorize"
        };
        args.drain(..2);
        mode
    } else if args.first().is_some_and(|a| a == "sends")
        && args
            .get(1)
            .is_some_and(|a| a == "pending" || a == "reconcile" || a == "forget-attachments")
    {
        let mode = match args[1].to_str() {
            Some("pending") => "sends-pending",
            Some("reconcile") => "sends-reconcile",
            _ => "sends-forget",
        };
        args.drain(..2);
        mode
    } else {
        return Err(bad());
    };
    let mut database = None;
    let mut state = None;
    let mut zone = None;
    let mut chat = None;
    let mut operation = Operation::Read;
    let mut contacts_helper = None;
    let mut delivery_helper = None;
    let mut send_id = None;
    let mut send_outcome = None;
    let mut args = args.into_iter();
    while let Some(option) = args.next() {
        let value = args.next().ok_or_else(bad)?;
        match option.to_str() {
            Some("--database") if mode == "mcp" => database = Some(PathBuf::from(value)),
            Some("--state-dir") => state = Some(PathBuf::from(value)),
            Some("--contacts-helper") => contacts_helper = Some(PathBuf::from(value)),
            Some("--delivery-helper") => delivery_helper = Some(PathBuf::from(value)),
            Some("--id") if mode.starts_with("sends-") => {
                send_id = Some(value.into_string().map_err(|_| bad())?)
            }
            Some("--outcome") if mode == "sends-reconcile" => {
                send_outcome = Some(match value.to_str() {
                    Some("sent") => icu_messages::send_journal::Outcome::Accepted,
                    Some("not_sent") => icu_messages::send_journal::Outcome::NotSent,
                    _ => return Err(bad()),
                })
            }
            Some("--time-zone") if mode == "mcp" => {
                zone = Some(value.into_string().map_err(|_| bad())?)
            }
            Some("--chat-guid") if mode == "revoke" => {
                chat = Some(value.into_string().map_err(|_| bad())?)
            }
            Some("--operation") if mode == "revoke" => {
                operation = match value.to_str() {
                    Some("read") => Operation::Read,
                    Some("send") => Operation::Send,
                    _ => return Err(bad()),
                }
            }
            _ => return Err(bad()),
        }
    }
    let user_root = std::env::var_os("HOME").map(PathBuf::from);
    let executable = std::env::current_exe().map_err(|_| bad())?;
    let native_helper = contacts_helper.clone().unwrap_or_else(|| {
        executable
            .parent()
            .unwrap()
            .join("Copilot Contacts.app/Contents/MacOS/icu-messages-contacts")
    });
    let send_helper = delivery_helper.clone().unwrap_or_else(|| {
        executable
            .parent()
            .unwrap()
            .join("Copilot Messages.app/Contents/MacOS/icu-messages-delivery")
    });
    if mode.starts_with("contacts-") {
        let result = icu_messages::contacts::invoke(
            &native_helper,
            serde_json::json!({"operation":if mode=="contacts-status"{"status"}else{"authorize"}}),
            std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false)),
            std::time::Duration::from_secs(if mode == "contacts-status" { 8 } else { 130 }),
        )?;
        println!("{result}");
        return Ok(());
    }
    if mode.starts_with("delivery-") {
        let response = icu_messages::native_bridge::invoke(
            &send_helper,
            serde_json::json!({"operation":if mode=="delivery-status"{"status"}else{"authorize"}}),
            std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false)),
            std::time::Duration::from_secs(if mode == "delivery-status" { 15 } else { 130 }),
        )?;
        println!("{response}");
        return Ok(());
    }
    let state = state
        .or_else(|| {
            user_root.as_ref().map(|p| {
                p.join(if cfg!(target_os = "macos") {
                    "Library/Application Support/Copilot/Messages"
                } else {
                    ".local/state/copilot/messages"
                })
            })
        })
        .ok_or_else(bad)?;
    let send_state = state.join("delivery");
    if mode.starts_with("sends-") {
        let journal = icu_messages::send_journal::Journal::open(send_state)?;
        match mode {
            "sends-pending" => println!(
                "{}",
                serde_json::to_string(&journal.pending()?).map_err(|_| bad())?
            ),
            "sends-reconcile" => {
                journal.reconcile(&send_id.ok_or_else(bad)?, send_outcome.ok_or_else(bad)?)?;
                println!("{{\"reconciled\":true}}");
            }
            _ => {
                journal.forget_attachments(&send_id.ok_or_else(bad)?)?;
                println!("{{\"attachments_removed\":true}}");
            }
        }
        return Ok(());
    }
    let mut store = Store::open(state)?;
    if mode == "list" {
        println!(
            "{}",
            serde_json::to_string(&store.persistent_destinations()?).map_err(|_| bad())?
        );
        return Ok(());
    }
    if mode == "revoke" {
        let chat = chat.filter(|s| !s.is_empty()).ok_or_else(bad)?;
        let mut removed = 0;
        for (op, destination) in store.persistent_destinations()? {
            if op == operation && destination.chat_guid == chat {
                store.revoke(op, &destination)?;
                removed += 1;
            }
        }
        println!("{}", serde_json::json!({"revoked":removed}));
        return Ok(());
    }
    let exported = database.is_some();
    #[cfg(target_os = "macos")]
    let database = database.or_else(|| {
        user_root
            .as_ref()
            .map(|p| p.join("Library/Messages/chat.db"))
    });
    let database = database.ok_or_else(|| {
        Error::new(
            "messagesPlatformUnsupported",
            "Provide an explicit exported fixture database when running outside macOS",
        )
    })?;
    let zone = zone
        .or_else(|| iana_time_zone::get_timezone().ok())
        .ok_or_else(|| {
            Error::new(
                "invalidArguments",
                "Could not determine the calendar zone; provide --time-zone",
            )
        })?;
    let worker = Worker::new(executable, database)?;
    let contacts: Box<dyn Contacts + Send> = if !exported || contacts_helper.is_some() {
        Box::new(icu_messages::contacts::Native::new(native_helper)?)
    } else {
        Box::new(AddressesOnly)
    };
    let mut engine = Engine::new(worker, store, contacts, zone)?;
    if cfg!(target_os = "macos") && (!exported || delivery_helper.is_some()) {
        engine = engine.with_sender(Box::new(icu_messages::native_delivery::Native::new(
            send_helper,
            send_state,
        )?));
    }
    mcp::serve(
        std::io::stdin(),
        std::io::stdout(),
        engine,
        mcp::Options::default(),
    )
}
