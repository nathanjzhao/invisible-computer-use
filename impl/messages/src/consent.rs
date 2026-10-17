//! Host-only decisions. No method in this module is exposed as a model tool.
//! A granted lease is single-use, membership-bound, revocable, and not serializable.
use crate::{Error, Result, cursor::Cursors, database::normalize_handle, invalid};
use fs2::FileExt;
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, HashMap, HashSet},
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    path::{Path, PathBuf},
    time::{Duration, Instant},
};

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum Operation {
    Read,
    Send,
}
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Scope {
    Once,
    Turn,
    Session,
    Always,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Destination {
    pub chat_guid: String,
    pub participants: Vec<String>,
    pub service: String,
}
impl Destination {
    pub fn normalized(&self) -> Result<Self> {
        if self.chat_guid.trim().is_empty()
            || self.chat_guid.len() > 1000
            || self.participants.len() > 100
        {
            return Err(invalid("Approval needs a resolved chat and its membership"));
        }
        let mut d = self.clone();
        d.participants = d.participants.iter().map(|s| normalize_handle(s)).collect();
        d.participants.sort();
        d.participants.dedup();
        if d.participants
            .iter()
            .any(|p| p.is_empty() || p.len() > 1000)
        {
            return Err(invalid("Invalid approved participant"));
        }
        if d.service.is_empty() || d.service.len() > 100 {
            return Err(invalid("Approval needs a resolved messaging service"));
        }
        Ok(d)
    }
    fn key(&self, operation: Operation) -> Result<String> {
        if operation == Operation::Send && self.participants.is_empty() {
            return Err(invalid("Sending needs resolved chat membership"));
        }
        Ok(Cursors::query_key(&(operation, self.normalized()?))?
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect())
    }
}
fn random_id() -> Result<String> {
    let mut b = [0; 24];
    getrandom::fill(&mut b)
        .map_err(|_| Error::new("internalError", "Could not generate an approval identifier"))?;
    Ok(b.iter().map(|v| format!("{v:02x}")).collect())
}

/// Constructed by the trusted host, never accepted from tool arguments.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Context {
    session: String,
    turn: String,
}
impl Context {
    pub fn new_session() -> Result<Self> {
        Ok(Self {
            session: random_id()?,
            turn: random_id()?,
        })
    }
    pub fn next_turn(&self) -> Result<Self> {
        Ok(Self {
            session: self.session.clone(),
            turn: random_id()?,
        })
    }
}
#[derive(Serialize, Deserialize)]
struct PersistentGrant {
    operation: Operation,
    destination: Destination,
}
#[derive(Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Persistent {
    version: u32,
    #[serde(default)]
    generation: Option<String>,
    revocation: u64,
    grants: BTreeMap<String, PersistentGrant>,
}
struct Ephemeral {
    key: String,
    context: Context,
    scope: Scope,
    revocation: u64,
}
struct Pending {
    key: String,
    operation: Operation,
    context: Context,
    revocation: u64,
    from_persistent: bool,
    created: Instant,
}
pub struct Lease {
    id: String,
}
pub struct Store {
    directory: PathBuf,
    generation: String,
    ephemeral: Vec<Ephemeral>,
    canceled: HashSet<(Context, Operation)>,
    ended: HashSet<String>,
    ended_turns: HashSet<Context>,
    denied: HashSet<(Context, Operation, String)>,
    pending: HashMap<String, Pending>,
}

impl Store {
    pub fn open(directory: PathBuf) -> Result<Self> {
        if !directory.is_absolute() {
            return Err(invalid("Approval directory must be absolute"));
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::{DirBuilderExt, MetadataExt};
            let mut builder = fs::DirBuilder::new();
            builder.recursive(true).mode(0o700);
            builder.create(&directory).map_err(|_| storage())?;
            let metadata = fs::symlink_metadata(&directory).map_err(|_| storage())?;
            if !metadata.is_dir()
                || metadata.uid() != unsafe { libc::geteuid() }
                || metadata.mode() & 0o077 != 0
            {
                return Err(storage());
            }
        }
        #[cfg(not(unix))]
        {
            return Err(Error::new(
                "messagesPlatformUnsupported",
                "Approval persistence requires a private Unix directory",
            ));
        }
        let mut store = Self {
            directory,
            generation: String::new(),
            ephemeral: Vec::new(),
            canceled: HashSet::new(),
            ended: HashSet::new(),
            ended_turns: HashSet::new(),
            denied: HashSet::new(),
            pending: HashMap::new(),
        };
        store.generation =
            store.with_disk(false, |disk| disk.generation.clone().ok_or_else(storage))?;
        Ok(store)
    }
    fn with_disk<T>(&self, write: bool, f: impl FnOnce(&mut Persistent) -> Result<T>) -> Result<T> {
        let lock = secure_file(&self.directory.join("approvals.lock"), true)?;
        // Every read takes the same short lock as writes, so another process's revocation is visible.
        let started = Instant::now();
        loop {
            match lock.try_lock_exclusive() {
                Ok(()) => break,
                Err(error)
                    if error.kind() == std::io::ErrorKind::WouldBlock
                        && started.elapsed() < Duration::from_secs(1) =>
                {
                    std::thread::sleep(Duration::from_millis(10))
                }
                Err(_) => return Err(storage()),
            }
        }
        let path = self.directory.join("approvals.json");
        let mut disk = match secure_file(&path, false) {
            Ok(file) => {
                let mut bytes = Vec::new();
                file.take(1024 * 1024 + 1)
                    .read_to_end(&mut bytes)
                    .map_err(|_| storage())?;
                if bytes.len() > 1024 * 1024 {
                    return Err(storage());
                }
                serde_json::from_slice::<Persistent>(&bytes).map_err(|_| storage())?
            }
            Err(e) if e.code == "messagesApprovalFileMissing" && self.generation.is_empty() => {
                Persistent {
                    version: 1,
                    ..Default::default()
                }
            }
            Err(e) => return Err(e),
        };
        let initialize = disk.version == 1 && self.generation.is_empty();
        if initialize {
            disk.version = 2;
            disk.generation = Some(random_id()?);
        }
        if disk.version != 2 || disk.grants.len() > 512 {
            return Err(storage());
        }
        let generation = disk
            .generation
            .as_deref()
            .filter(|s| s.len() == 48 && s.bytes().all(|b| b.is_ascii_hexdigit()))
            .ok_or_else(storage)?;
        if !self.generation.is_empty() && self.generation != generation {
            return Err(storage());
        }
        for (key, g) in &disk.grants {
            if g.destination.key(g.operation)? != *key {
                return Err(storage());
            }
        }
        let result = f(&mut disk)?;
        if write || initialize {
            let bytes = serde_json::to_vec(&disk).map_err(|_| storage())?;
            if bytes.len() > 1024 * 1024 || disk.grants.len() > 512 {
                return Err(storage());
            }
            let mut tmp =
                tempfile::NamedTempFile::new_in(&self.directory).map_err(|_| storage())?;
            tmp.write_all(&bytes).map_err(|_| storage())?;
            tmp.as_file().sync_all().map_err(|_| storage())?;
            tmp.persist(&path).map_err(|_| storage())?;
            File::open(&self.directory)
                .and_then(|d| d.sync_all())
                .map_err(|_| storage())?;
        }
        Ok(result)
    }
    fn active(&self, context: &Context, operation: Operation) -> Result<()> {
        if self.ended.contains(&context.session) {
            return Err(Error::new(
                "messagesSessionEnded",
                "The approval session ended",
            ));
        }
        if self.ended_turns.contains(context) {
            return Err(Error::new(
                "messagesTurnEnded",
                "This Messages approval turn has ended",
            ));
        }
        if self.canceled.contains(&(context.clone(), operation)) {
            return Err(Error::new(
                "messagesApprovalCanceled",
                "Approval was canceled for this turn. Do not ask again or retry.",
            ));
        }
        Ok(())
    }
    pub fn check_context(&self, context: &Context, operation: Operation) -> Result<()> {
        self.active(context, operation)
    }
    pub fn discard_pending(&mut self, context: &Context) {
        self.pending.retain(|_, p| p.context != *context);
    }
    fn not_declined(&self, context: &Context, operation: Operation, key: &str) -> Result<()> {
        if self
            .denied
            .contains(&(context.clone(), operation, key.into()))
        {
            Err(Error::new(
                "messagesReadDeclined",
                "Access to this chat was declined for this turn. Do not ask again or retry.",
            ))
        } else {
            Ok(())
        }
    }
    /// Called only by the host after its human approval event, never by tool JSON.
    pub fn approve(
        &mut self,
        context: &Context,
        operation: Operation,
        destination: &Destination,
        scope: Scope,
    ) -> Result<()> {
        self.active(context, operation)?;
        let destination = destination.normalized()?;
        let key = destination.key(operation)?;
        self.not_declined(context, operation, &key)?;
        if scope == Scope::Always {
            self.with_disk(true, |d| {
                d.grants.insert(
                    key,
                    PersistentGrant {
                        operation,
                        destination,
                    },
                );
                Ok(())
            })
        } else {
            if self.ephemeral.len() >= 1024 {
                return Err(Error::new(
                    "messagesApprovalCapacity",
                    "Too many active approvals",
                ));
            }
            let revocation = self.with_disk(false, |d| Ok(d.revocation))?;
            self.ephemeral.push(Ephemeral {
                key,
                context: context.clone(),
                scope,
                revocation,
            });
            Ok(())
        }
    }
    pub fn acquire(
        &mut self,
        context: &Context,
        operation: Operation,
        destination: &Destination,
    ) -> Result<Option<Lease>> {
        self.active(context, operation)?;
        let key = destination.key(operation)?;
        self.not_declined(context, operation, &key)?;
        self.pending
            .retain(|_, p| p.created.elapsed() < Duration::from_secs(600));
        if self.pending.len() >= 256 {
            return Err(Error::new(
                "messagesApprovalCapacity",
                "Too many pending approved operations",
            ));
        }
        let (persistent, revocation) =
            self.with_disk(false, |d| Ok((d.grants.contains_key(&key), d.revocation)))?;
        let grant = self.ephemeral.iter().position(|g| {
            g.key == key
                && g.revocation == revocation
                && g.context.session == context.session
                && (g.scope == Scope::Session || g.context.turn == context.turn)
        });
        if !persistent && grant.is_none() {
            return Ok(None);
        }
        if !persistent
            && let Some(i) = grant
            && self.ephemeral[i].scope == Scope::Once
        {
            self.ephemeral.remove(i);
        }
        let id = random_id()?;
        self.pending.insert(
            id.clone(),
            Pending {
                key,
                context: context.clone(),
                operation,
                revocation,
                from_persistent: persistent,
                created: Instant::now(),
            },
        );
        Ok(Some(Lease { id }))
    }
    /// Check immediately before the read/send side effect. The lease is consumed even on failure.
    pub fn consume(
        &mut self,
        lease: Lease,
        context: &Context,
        operation: Operation,
        destination: &Destination,
    ) -> Result<()> {
        let pending = self.pending.remove(&lease.id).ok_or_else(|| {
            Error::new(
                "messagesApprovalExpired",
                "Approval is missing, expired or already consumed",
            )
        })?;
        self.active(context, operation)?;
        self.not_declined(context, operation, &pending.key)?;
        let (current, still_persisted) = self.with_disk(false, |d| {
            Ok((d.revocation, d.grants.contains_key(&pending.key)))
        })?;
        if pending.context != *context
            || pending.operation != operation
            || pending.key != destination.key(operation)?
            || pending.revocation != current
            || (pending.from_persistent && !still_persisted)
            || pending.created.elapsed() >= Duration::from_secs(600)
        {
            return Err(Error::new(
                "messagesApprovalExpired",
                "Approval changed, expired or no longer matches the chat membership",
            ));
        }
        Ok(())
    }
    pub fn cancel(&mut self, context: &Context, operation: Operation) {
        self.canceled.insert((context.clone(), operation));
        self.pending
            .retain(|_, p| p.context != *context || p.operation != operation);
    }
    pub fn decline(
        &mut self,
        context: &Context,
        operation: Operation,
        destination: &Destination,
    ) -> Result<()> {
        let key = destination.key(operation)?;
        self.denied
            .insert((context.clone(), operation, key.clone()));
        self.pending
            .retain(|_, p| p.context != *context || p.operation != operation || p.key != key);
        Ok(())
    }
    pub fn end_turn(&mut self, context: &Context) {
        self.ended_turns.insert(context.clone());
        self.ephemeral
            .retain(|g| g.context != *context || g.scope == Scope::Session);
        self.pending.retain(|_, p| p.context != *context);
        self.canceled.retain(|(c, _)| c != context);
        self.denied.retain(|(c, _, _)| c != context);
    }
    pub fn revoke(&mut self, operation: Operation, destination: &Destination) -> Result<()> {
        let key = destination.key(operation)?;
        self.with_disk(true, |d| {
            d.grants.remove(&key);
            d.revocation = d.revocation.checked_add(1).ok_or_else(storage)?;
            Ok(())
        })?;
        self.ephemeral.retain(|g| g.key != key);
        self.pending.retain(|_, p| p.key != key);
        Ok(())
    }
    pub fn end_session(&mut self, context: &Context) {
        self.ended.insert(context.session.clone());
        self.ephemeral
            .retain(|g| g.context.session != context.session);
        self.pending
            .retain(|_, p| p.context.session != context.session);
        self.canceled.retain(|(c, _)| c.session != context.session);
        self.ended_turns.retain(|c| c.session != context.session);
        self.denied.retain(|(c, _, _)| c.session != context.session);
    }
    pub fn persistent_destinations(&self) -> Result<Vec<(Operation, Destination)>> {
        self.with_disk(false, |d| {
            Ok(d.grants
                .values()
                .map(|g| (g.operation, g.destination.clone()))
                .collect())
        })
    }
}

fn storage() -> Error {
    Error::new(
        "messagesApprovalStorage",
        "Approval storage could not be checked or saved; no message contents were returned",
    )
}
fn secure_file(path: &Path, create: bool) -> Result<File> {
    let mut options = OpenOptions::new();
    options.read(true).write(create).create(create);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options
            .mode(0o600)
            .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC | libc::O_NONBLOCK);
    }
    let file = options.open(path).map_err(|e| {
        if !create && e.kind() == std::io::ErrorKind::NotFound {
            Error::new("messagesApprovalFileMissing", "No persistent approvals")
        } else {
            storage()
        }
    })?;
    let metadata = file.metadata().map_err(|_| storage())?;
    if !metadata.is_file() {
        return Err(storage());
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        if metadata.uid() != unsafe { libc::geteuid() } || metadata.mode() & 0o077 != 0 {
            return Err(storage());
        }
    }
    Ok(file)
}
