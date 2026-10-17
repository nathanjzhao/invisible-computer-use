//! Prepare immutable payloads, review them, then consume one commit. No native send is implicit.
use crate::{
    Error, Result,
    consent::Destination,
    contacts::{Person, is_address},
    contract::Contacts,
    database::{Chat, normalize_handle},
    invalid,
    worker::{Query, Worker},
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeSet,
    fs::{File, OpenOptions},
    io::{Read, Write},
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Target {
    Existing {
        chat_guid: String,
        name: Option<String>,
        service: String,
        recipients: Vec<String>,
    },
    New {
        recipients: Vec<String>,
    },
}
impl Target {
    pub fn consent(&self) -> Option<Destination> {
        match self {
            Self::Existing {
                chat_guid,
                service,
                recipients,
                ..
            } => Some(Destination {
                chat_guid: chat_guid.clone(),
                participants: recipients.clone(),
                service: service.clone(),
            }),
            _ => None,
        }
    }
    pub fn recipients(&self) -> &[String] {
        match self {
            Self::Existing { recipients, .. } | Self::New { recipients } => recipients,
        }
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Request {
    pub chat_guid: Option<String>,
    pub recipients: Option<Vec<String>>,
    pub text: Option<String>,
    pub attachments: Option<Vec<PathBuf>>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct FileSnapshot {
    pub original: PathBuf,
    pub canonical: PathBuf,
    pub staged: PathBuf,
    pub size: u64,
    pub sha256: String,
    pub device: u64,
    pub inode: u64,
}
#[derive(Clone, Debug, Serialize)]
pub struct FilePreview {
    pub path: PathBuf,
    pub resolved_path: PathBuf,
    pub size: u64,
    pub sha256: String,
}
#[derive(Clone, Debug, Serialize)]
pub struct Preview {
    pub target: Target,
    pub text: Option<String>,
    pub attachments: Vec<FilePreview>,
}
#[derive(Clone)]
struct Binding {
    query: String,
    person: Person,
}
pub struct Prepared {
    target: Target,
    text: Option<String>,
    files: Vec<FileSnapshot>,
    bindings: Vec<Binding>,
    _staging: tempfile::TempDir,
}
pub struct Approved {
    plan: Prepared,
    text: Option<String>,
    user_edited: bool,
}
#[derive(Debug)]
pub struct Outbound<'a> {
    pub target: &'a Target,
    pub text: Option<&'a str>,
    pub attachments: &'a [FileSnapshot],
}
#[derive(Clone, Debug, Serialize)]
pub struct Sent {
    pub chat_guid: Option<String>,
    pub user_edited: bool,
    pub attachment_count: usize,
}
pub enum Delivery {
    Accepted { chat_guid: Option<String> },
    NotSent(Error),
    Uncertain(String),
}
/// A transport must consume attachment bytes or take durable ownership before returning.
/// Staging paths are valid only during this call. Never retry an uncertain operation.
pub trait Transport: Send {
    fn deliver(&mut self, payload: Outbound<'_>, cancel: &Arc<AtomicBool>) -> Delivery;
}

impl Prepared {
    pub fn create(
        request: Request,
        worker: &Worker,
        contacts: &dyn Contacts,
        cancel: Arc<AtomicBool>,
    ) -> Result<Self> {
        canceled(&cancel)?;
        content(
            &request.text,
            request.attachments.as_deref().unwrap_or(&[]).len(),
        )?;
        if request.chat_guid.is_some() == request.recipients.is_some() {
            return Err(invalid("Provide exactly one of chat_guid or recipients"));
        }
        let mut bindings = Vec::new();
        let resolved = if let Some(guid) = request.chat_guid {
            let mut response = worker.query(
                Query::Describe {
                    chat_guids: vec![guid],
                },
                cancel.clone(),
            )?;
            target(response.chats.pop().ok_or_else(unavailable)?)?
        } else {
            let recipients = request.recipients.unwrap();
            if recipients.is_empty() || recipients.len() > 20 {
                return Err(invalid("Provide 1..20 recipients"));
            }
            let mut people = Vec::new();
            let mut seen = BTreeSet::new();
            for query in recipients {
                canceled(&cancel)?;
                let mut candidates = contacts.candidates(&query)?;
                if candidates.len() != 1 {
                    return Err(Error::new(
                        "messagesAmbiguousDestination",
                        "More than one contact matched a recipient. Use a full name, phone number, email address or exact chat_guid.",
                    ));
                }
                let person = candidates.pop().unwrap();
                if seen.insert(person.identifier.clone()) {
                    bindings.push(Binding {
                        query,
                        person: person.clone(),
                    });
                    people.push(person);
                }
            }
            let groups = people
                .iter()
                .map(|p| {
                    p.handles
                        .iter()
                        .filter(|h| is_address(h))
                        .cloned()
                        .collect::<Vec<_>>()
                })
                .collect::<Vec<_>>();
            if groups.iter().any(Vec::is_empty) {
                return Err(unavailable());
            }
            let mut candidates = worker
                .query(
                    Query::Destinations {
                        groups: groups.clone(),
                    },
                    cancel.clone(),
                )?
                .chats;
            candidates.retain(|c| exact_members(&c.participants, &groups));
            // Prefer the most recently used exact conversation, with deterministic service/GUID ties.
            candidates.sort_by(|a, b| {
                last_date(b)
                    .cmp(&last_date(a))
                    .then_with(|| service_rank(&a.service).cmp(&service_rank(&b.service)))
                    .then_with(|| a.chat_guid.cmp(&b.chat_guid))
            });
            if let Some(chat) = candidates.into_iter().next() {
                target(chat)?
            } else {
                let mut handles = Vec::new();
                for group in groups {
                    let preferred = group
                        .iter()
                        .find(|h| !h.contains('@'))
                        .or(group.first())
                        .ok_or_else(unavailable)?;
                    let handle = if preferred.trim().starts_with('+') {
                        format!("+{}", normalize_handle(preferred))
                    } else {
                        normalize_handle(preferred)
                    };
                    if !handles.contains(&handle) {
                        handles.push(handle);
                    }
                }
                Target::New {
                    recipients: handles,
                }
            }
        };
        let staging = tempfile::Builder::new()
            .prefix("icu-message-send-")
            .tempdir()
            .map_err(|_| unavailable())?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(staging.path(), std::fs::Permissions::from_mode(0o700))
                .map_err(|_| unavailable())?;
        }
        let paths = request.attachments.unwrap_or_default();
        let files = if paths.is_empty() {
            vec![]
        } else {
            let mut stager = worker.clone();
            stager.limits.wall = Duration::from_secs(30);
            let data = stager
                .query(
                    Query::StageFiles {
                        paths,
                        directory: staging.path().to_path_buf(),
                    },
                    cancel.clone(),
                )?
                .data;
            serde_json::from_value(data).map_err(|_| unavailable())?
        };
        canceled(&cancel)?;
        Ok(Self {
            target: resolved,
            text: request.text,
            files,
            bindings,
            _staging: staging,
        })
    }
    pub fn preview(&self) -> Preview {
        Preview {
            target: self.target.clone(),
            text: self.text.clone(),
            attachments: self
                .files
                .iter()
                .map(|f| FilePreview {
                    path: f.original.clone(),
                    resolved_path: f.canonical.clone(),
                    size: f.size,
                    sha256: f.sha256.clone(),
                })
                .collect(),
        }
    }
    pub fn consent(&self) -> Option<Destination> {
        self.target.consent()
    }
    /// Only the trusted host's accepted, editable review produces an approved payload.
    pub fn approve(self, text: String) -> Result<Approved> {
        let text = if text.is_empty() { None } else { Some(text) };
        content(&text, self.files.len())?;
        let user_edited = text != self.text;
        Ok(Approved {
            plan: self,
            text,
            user_edited,
        })
    }
}
impl Approved {
    pub fn consent(&self) -> Option<Destination> {
        self.plan.consent()
    }
    pub fn fingerprint(&self) -> Result<String> {
        fingerprint(&Outbound {
            target: &self.plan.target,
            text: self.text.as_deref(),
            attachments: &self.plan.files,
        })
    }
    pub fn revalidate(
        &self,
        worker: &Worker,
        contacts: &dyn Contacts,
        cancel: Arc<AtomicBool>,
    ) -> Result<()> {
        canceled(&cancel)?;
        for binding in &self.plan.bindings {
            let current = contacts.candidates(&binding.query)?;
            if current.len() != 1
                || current[0].identifier != binding.person.identifier
                || normalized(&current[0].handles) != normalized(&binding.person.handles)
            {
                return Err(changed(
                    "The recipient's Contacts record changed during review",
                ));
            }
        }
        if let Some(original) = self.consent() {
            let mut current = worker
                .query(
                    Query::Describe {
                        chat_guids: vec![original.chat_guid.clone()],
                    },
                    cancel.clone(),
                )?
                .chats;
            let now = target(current.pop().ok_or_else(unavailable)?)?
                .consent()
                .ok_or_else(unavailable)?;
            if original.normalized()? != now.normalized()? {
                return Err(changed(
                    "The chat's service or membership changed during review",
                ));
            }
        }
        if !self.plan.files.is_empty() {
            let mut validator = worker.clone();
            validator.limits.wall = Duration::from_secs(30);
            validator.query(
                Query::ValidateFiles {
                    files: self.plan.files.clone(),
                },
                cancel.clone(),
            )?;
        }
        canceled(&cancel)
    }
    /// Consuming self makes the same approved plan impossible to commit a second time.
    pub fn commit(
        self,
        worker: &Worker,
        contacts: &dyn Contacts,
        transport: &mut dyn Transport,
        cancel: Arc<AtomicBool>,
    ) -> Result<Sent> {
        self.commit_authorized(worker, contacts, transport, cancel, || Ok(()))
    }
    pub fn commit_authorized(
        self,
        worker: &Worker,
        contacts: &dyn Contacts,
        transport: &mut dyn Transport,
        cancel: Arc<AtomicBool>,
        authorize: impl FnOnce() -> Result<()>,
    ) -> Result<Sent> {
        self.revalidate(worker, contacts, cancel.clone())?;
        authorize()?;
        canceled(&cancel)?;
        match transport.deliver(
            Outbound {
                target: &self.plan.target,
                text: self.text.as_deref(),
                attachments: &self.plan.files,
            },
            &cancel,
        ) {
            Delivery::Accepted { chat_guid } => Ok(Sent {
                chat_guid,
                user_edited: self.user_edited,
                attachment_count: self.plan.files.len(),
            }),
            Delivery::NotSent(error) => Err(error),
            Delivery::Uncertain(reason) => Err(Error::new(
                "messagesSendOutcomeUncertain",
                format!("The send may have taken effect. Do not retry automatically. {reason}"),
            )),
        }
    }
}
pub fn fingerprint(payload: &Outbound<'_>) -> Result<String> {
    let destination = match payload.target {
        Target::Existing {
            chat_guid,
            service,
            recipients,
            ..
        } => {
            serde_json::json!({"chat_guid":chat_guid,"service":service,"recipients":normalized(recipients)})
        }
        Target::New { recipients } => serde_json::json!({"recipients":normalized(recipients)}),
    };
    let bytes = serde_json::to_vec(&(
        destination,
        payload.text,
        payload
            .attachments
            .iter()
            .map(|f| (f.size, &f.sha256))
            .collect::<Vec<_>>(),
    ))
    .map_err(|_| unavailable())?;
    Ok(format!("{:x}", Sha256::digest(bytes)))
}
fn content(text: &Option<String>, files: usize) -> Result<()> {
    if files > 10
        || text
            .as_ref()
            .is_some_and(|t| t.is_empty() || t.chars().count() > 10_000)
        || (files == 0 && text.is_none())
    {
        return Err(invalid(
            "A send needs text, up to ten files, or both; text is limited to 10000 characters",
        ));
    }
    Ok(())
}
fn target(chat: Chat) -> Result<Target> {
    if chat.participants.is_empty() {
        return Err(unavailable());
    }
    let service = chat
        .service
        .filter(|s| !s.is_empty())
        .ok_or_else(unavailable)?;
    Ok(Target::Existing {
        chat_guid: chat.chat_guid,
        name: chat.name,
        service,
        recipients: chat.participants,
    })
}
fn service_rank(service: &Option<String>) -> usize {
    match service.as_deref() {
        Some("iMessage") => 0,
        Some("RCS") => 1,
        Some("SMS") => 2,
        _ => 3,
    }
}
fn last_date(chat: &Chat) -> i64 {
    chat.last_message_date
        .as_deref()
        .and_then(|s| crate::dates::parse(s).ok())
        .unwrap_or(i64::MIN)
}
fn normalized(handles: &[String]) -> BTreeSet<String> {
    handles.iter().map(|h| normalize_handle(h)).collect()
}
fn exact_members(handles: &[String], groups: &[Vec<String>]) -> bool {
    let handles: Vec<_> = normalized(handles).into_iter().collect();
    if handles.len() != groups.len() {
        return false;
    }
    fn assign(
        i: usize,
        groups: &[Vec<String>],
        handles: &[String],
        seen: &mut [bool],
        owners: &mut [Option<usize>],
    ) -> bool {
        for j in 0..handles.len() {
            if seen[j] || !groups[i].iter().any(|h| normalize_handle(h) == handles[j]) {
                continue;
            }
            seen[j] = true;
            if owners[j].is_none() || assign(owners[j].unwrap(), groups, handles, seen, owners) {
                owners[j] = Some(i);
                return true;
            }
        }
        false
    }
    let mut owners = vec![None; handles.len()];
    (0..groups.len()).all(|i| {
        assign(
            i,
            groups,
            &handles,
            &mut vec![false; handles.len()],
            &mut owners,
        )
    })
}
fn canceled(cancel: &AtomicBool) -> Result<()> {
    if cancel.load(Ordering::SeqCst) {
        Err(Error::new(
            "messagesSendCanceled",
            "The send was canceled before the transport was called",
        ))
    } else {
        Ok(())
    }
}
fn unavailable() -> Error {
    Error::new(
        "messagesSendPlanUnavailable",
        "A complete send plan could not be prepared",
    )
}
fn changed(reason: &str) -> Error {
    Error::new(
        "messagesSendPlanChanged",
        format!("{reason}. No message was sent; prepare a new review."),
    )
}

#[cfg(unix)]
fn source(path: &Path) -> Result<(File, PathBuf)> {
    use std::os::unix::fs::OpenOptionsExt;
    if !path.is_absolute() {
        return Err(invalid("Attachment paths must be absolute"));
    }
    let canonical = path
        .canonicalize()
        .map_err(|_| changed("An attachment is missing"))?;
    let file = OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK | libc::O_CLOEXEC)
        .open(&canonical)
        .map_err(|_| changed("An attachment cannot be opened"))?;
    if !file.metadata().map_err(|_| unavailable())?.is_file() {
        return Err(invalid(
            "Attachments must be regular files; directories and special files are unsupported",
        ));
    }
    Ok((file, canonical))
}
#[cfg(not(unix))]
fn source(_: &Path) -> Result<(File, PathBuf)> {
    Err(unavailable())
}
pub fn stage_files(paths: &[PathBuf], directory: &Path) -> Result<Vec<FileSnapshot>> {
    #[cfg(unix)]
    use std::os::unix::fs::{DirBuilderExt, MetadataExt, OpenOptionsExt};
    if paths.is_empty() || paths.len() > 10 || !directory.is_absolute() {
        return Err(invalid("Invalid attachment staging request"));
    }
    let metadata = std::fs::symlink_metadata(directory).map_err(|_| unavailable())?;
    #[cfg(unix)]
    if !metadata.is_dir()
        || metadata.uid() != unsafe { libc::geteuid() }
        || metadata.mode() & 0o077 != 0
    {
        return Err(unavailable());
    }
    let mut result = Vec::new();
    for (index, path) in paths.iter().enumerate() {
        let (mut input, canonical) = source(path)?;
        let before = input.metadata().map_err(|_| unavailable())?;
        let available = fs2::available_space(directory).map_err(|_| unavailable())?;
        if before.len() > available.saturating_sub(64 * 1024 * 1024) {
            return Err(Error::new(
                "messagesAttachmentStorage",
                "There is not enough space to prepare an immutable copy of this attachment",
            ));
        }
        let sub = directory.join(index.to_string());
        let mut builder = std::fs::DirBuilder::new();
        #[cfg(unix)]
        builder.mode(0o700);
        builder.create(&sub).map_err(|_| unavailable())?;
        let staged = sub.join(path.file_name().ok_or_else(unavailable)?);
        let mut options = OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        options.mode(0o600);
        let mut output = options.open(&staged).map_err(|_| unavailable())?;
        let mut hash = Sha256::new();
        let mut buffer = [0u8; 65536];
        let mut size = 0u64;
        loop {
            let n = input
                .read(&mut buffer)
                .map_err(|_| changed("An attachment could not be read"))?;
            if n == 0 {
                break;
            }
            size += n as u64;
            if size > before.len() {
                return Err(changed("An attachment grew while being prepared"));
            }
            output.write_all(&buffer[..n]).map_err(|_| {
                Error::new(
                    "messagesAttachmentStorage",
                    "Attachment staging failed; check available disk space",
                )
            })?;
            hash.update(&buffer[..n]);
        }
        output.sync_all().map_err(|_| unavailable())?;
        let after = input.metadata().map_err(|_| unavailable())?;
        if size != before.len()
            || after.len() != before.len()
            || after.modified().ok() != before.modified().ok()
        {
            return Err(changed("An attachment changed while being prepared"));
        }
        #[cfg(unix)]
        let (device, inode) = (before.dev(), before.ino());
        #[cfg(not(unix))]
        let (device, inode) = (0, 0);
        result.push(FileSnapshot {
            original: path.clone(),
            canonical,
            staged,
            size,
            sha256: format!("{:x}", hash.finalize()),
            device,
            inode,
        });
    }
    Ok(result)
}
pub fn validate_files(files: &[FileSnapshot]) -> Result<()> {
    #[cfg(unix)]
    use std::os::unix::fs::MetadataExt;
    for file in files {
        for (path, is_original) in [(&file.original, true), (&file.staged, false)] {
            let (mut current, canonical) = source(path)?;
            let metadata = current.metadata().map_err(|_| unavailable())?;
            if is_original && canonical != file.canonical {
                return Err(changed("An attachment now resolves to a different file"));
            }
            #[cfg(unix)]
            if is_original && (metadata.dev() != file.device || metadata.ino() != file.inode) {
                return Err(changed("An attachment was replaced during review"));
            }
            if metadata.len() != file.size {
                return Err(changed("An attachment changed size during review"));
            }
            let mut hash = Sha256::new();
            let mut size = 0u64;
            let mut buffer = [0u8; 65536];
            loop {
                let n = current
                    .read(&mut buffer)
                    .map_err(|_| changed("An attachment cannot be re-read"))?;
                if n == 0 {
                    break;
                }
                size += n as u64;
                if size > file.size {
                    return Err(changed("An attachment changed during validation"));
                }
                hash.update(&buffer[..n]);
            }
            if size != file.size || format!("{:x}", hash.finalize()) != file.sha256 {
                return Err(changed("An attachment's contents changed during review"));
            }
        }
    }
    Ok(())
}
