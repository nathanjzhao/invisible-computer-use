//! Native transport retains attachment copies and a durable dispatch receipt.
use crate::{
    Error, Result, native_bridge,
    send::{self, Delivery, FileSnapshot, Outbound, Transport},
    send_journal::{Journal, Outcome},
};
use serde_json::json;
use std::{
    fs,
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};

pub struct Native {
    helper: PathBuf,
    journal: Journal,
}
impl Native {
    pub fn new(helper: PathBuf, directory: PathBuf) -> Result<Self> {
        if !helper.is_absolute() {
            return Err(crate::invalid(
                "Native Messages helper path must be absolute",
            ));
        }
        Ok(Self {
            helper,
            journal: Journal::open(directory)?,
        })
    }
    fn copies(
        &self,
        id: &str,
        files: &[FileSnapshot],
        cancel: &AtomicBool,
    ) -> Result<Vec<FileSnapshot>> {
        use std::io::{Read, Write};
        #[cfg(unix)]
        use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt};
        let spool = self.journal.directory().join("attachments").join(id);
        let mut builder = fs::DirBuilder::new();
        builder.recursive(true);
        #[cfg(unix)]
        builder.mode(0o700);
        builder.create(&spool).map_err(|_| storage())?;
        let mut copies = Vec::new();
        for (i, file) in files.iter().enumerate() {
            if cancel.load(Ordering::SeqCst) {
                return Err(Error::new(
                    "messagesSendCanceled",
                    "The send was canceled while preparing retained attachments",
                ));
            }
            if file.size
                > fs2::available_space(&spool)
                    .map_err(|_| storage())?
                    .saturating_sub(64 * 1024 * 1024)
            {
                return Err(storage());
            }
            let sub = spool.join(i.to_string());
            let mut builder = fs::DirBuilder::new();
            #[cfg(unix)]
            builder.mode(0o700);
            builder.create(&sub).map_err(|_| storage())?;
            let path = sub.join(file.original.file_name().ok_or_else(storage)?);
            let mut input_options = fs::OpenOptions::new();
            input_options.read(true);
            #[cfg(unix)]
            input_options.custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK | libc::O_CLOEXEC);
            let mut input = input_options.open(&file.staged).map_err(|_| storage())?;
            let metadata = input.metadata().map_err(|_| storage())?;
            if !metadata.is_file() || metadata.len() != file.size {
                return Err(storage());
            }
            let mut options = fs::OpenOptions::new();
            options.write(true).create_new(true);
            #[cfg(unix)]
            options.mode(0o600);
            let mut output = options.open(&path).map_err(|_| storage())?;
            let mut buffer = [0u8; 65536];
            let mut size = 0u64;
            let started = std::time::Instant::now();
            loop {
                if cancel.load(Ordering::SeqCst) {
                    return Err(Error::new(
                        "messagesSendCanceled",
                        "The send was canceled before native dispatch",
                    ));
                }
                if started.elapsed() > Duration::from_secs(30) {
                    return Err(storage());
                }
                let n = input.read(&mut buffer).map_err(|_| storage())?;
                if n == 0 {
                    break;
                }
                size += n as u64;
                if size > file.size {
                    return Err(storage());
                }
                output.write_all(&buffer[..n]).map_err(|_| storage())?;
            }
            if size != file.size {
                return Err(storage());
            }
            output.sync_all().map_err(|_| storage())?;
            let mut copy = file.clone();
            copy.staged = path;
            copies.push(copy);
        }
        send::validate_files(&copies)?;
        Ok(copies)
    }
    fn remove_copies(&self, id: &str) {
        let _ = fs::remove_dir_all(self.journal.directory().join("attachments").join(id));
    }
}
fn storage() -> Error {
    Error::new(
        "messagesAttachmentStorage",
        "The native transport could not retain a verified copy of the attachments; no message was sent",
    )
}
impl Transport for Native {
    fn deliver(&mut self, payload: Outbound<'_>, cancel: &Arc<AtomicBool>) -> Delivery {
        if cancel.load(Ordering::SeqCst) {
            return Delivery::NotSent(Error::new(
                "messagesSendCanceled",
                "The send was canceled before native dispatch",
            ));
        }
        if !self.helper.is_file() {
            return Delivery::NotSent(Error::new(
                "messagesSendUnavailable",
                "The native Messages helper is missing",
            ));
        }
        let fingerprint = match send::fingerprint(&payload) {
            Ok(v) => v,
            Err(e) => return Delivery::NotSent(e),
        };
        let attempt =
            match self
                .journal
                .begin_for(&fingerprint, Some(json!(payload.target)), cancel)
            {
                Ok(v) => v,
                Err(e) => return Delivery::NotSent(e),
            };
        let id = attempt.id().to_string();
        let copies = match self.copies(&id, payload.attachments, cancel) {
            Ok(v) => v,
            Err(e) => {
                let _ = self
                    .journal
                    .finish(attempt, Outcome::NotSent, Some(e.code.clone()));
                self.remove_copies(&id);
                return Delivery::NotSent(e);
            }
        };
        if cancel.load(Ordering::SeqCst) {
            let _ = self.journal.finish(
                attempt,
                Outcome::NotSent,
                Some("canceled before dispatch".into()),
            );
            self.remove_copies(&id);
            return Delivery::NotSent(Error::new(
                "messagesSendCanceled",
                "The send was canceled before native dispatch",
            ));
        }
        // From this point onward, loss of the helper reply is uncertain even when no send occurred.
        let reply = native_bridge::invoke(
            &self.helper,
            json!({"operation":"send","attempt_id":id,"target":payload.target,"text":payload.text,"files":copies}),
            cancel.clone(),
            Duration::from_secs(35),
        );
        let (outcome, delivery, reason) = match reply {
            Ok(data) if data["status"] == "accepted" => (
                Outcome::Accepted,
                Delivery::Accepted {
                    chat_guid: data["chat_guid"].as_str().map(str::to_owned),
                },
                None,
            ),
            Ok(data) if data["status"] == "not_running" => (
                Outcome::NotSent,
                Delivery::NotSent(Error::new(
                    "messagesApplicationUnavailable",
                    "Messages is not running. Open it and check the account before sending.",
                )),
                Some("not_running".into()),
            ),
            Ok(_) => (
                Outcome::Uncertain,
                Delivery::Uncertain(format!(
                    "The helper returned an unrecognized result. Attempt {id}."
                )),
                Some("invalid_result".into()),
            ),
            Err(error)
                if matches!(
                    error.code.as_str(),
                    "invalidArguments"
                        | "messagesAutomationPermissionRequired"
                        | "messagesAccountUnavailable"
                        | "messagesSendPlanChanged"
                        | "messagesSendPlanUnavailable"
                        | "messagesAmbiguousDestination"
                        | "messagesSendCanceled"
                        | "messagesApplicationUnavailable"
                ) =>
            {
                (
                    Outcome::NotSent,
                    Delivery::NotSent(error.clone()),
                    Some(format!("{}: {}", error.code, error.message)),
                )
            }
            Err(error) => (
                Outcome::Uncertain,
                Delivery::Uncertain(format!("Native acknowledgement was lost. Attempt {id}.")),
                Some(format!("{}: {}", error.code, error.message)),
            ),
        };
        let definitely_not_sent = outcome == Outcome::NotSent;
        if self.journal.finish(attempt, outcome, reason).is_err() {
            return Delivery::Uncertain(format!(
                "Could not save the final send outcome. Attempt {id}."
            ));
        }
        if definitely_not_sent {
            self.remove_copies(&id);
        }
        // Accepted/uncertain files remain until explicit cleanup because Messages may consume files asynchronously.
        delivery
    }
}
