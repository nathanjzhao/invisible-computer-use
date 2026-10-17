//! The crate's command-line verbs as a library entry point, so the `icu` binary can host
//! them (`icu app-instructions ...`) and the standalone binary stays a thin shim.
//! CLI over the pack table, for the conformance harness and for evidence runs.
//!
//!   app-instructions --bundle-id <ID>   print the pack body verbatim (exit 3 if no pack)
//!   app-instructions --list             print `bundle_id\tfile\tconfirmed` per pack

use crate::{pack_for_bundle_id, PACKS};
use std::io::Write;

pub fn run(a: Vec<String>) {
    match a.get(1).map(String::as_str) {
        Some("--list") => {
            for p in PACKS {
                println!("{}\t{}\t{}", p.bundle_id, p.file, if p.bundle_id_confirmed { "confirmed" } else { "inferred" });
            }
        }
        Some("--bundle-id") if a.len() >= 3 => match pack_for_bundle_id(&a[2]) {
            Some(p) => {
                // verbatim bytes: no added newline
                std::io::stdout().write_all(p.body.as_bytes()).unwrap();
            }
            None => {
                eprintln!("[app-instructions] no pack for bundle id {:?}", a[2]);
                std::process::exit(3);
            }
        },
        _ => {
            eprintln!("usage: app-instructions --bundle-id <ID> | --list");
            std::process::exit(2);
        }
    }
}
