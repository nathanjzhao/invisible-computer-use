//! Durable retry metadata. No prompts, provider replies or remembered text live here.
use super::{MemoryService, Ts};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

const FILE: &str = "retry-state.json";
const UNREADABLE: &str = "Memory retry state could not be read. Check storage access and retry.";
const UNWRITABLE: &str = "Memory retry state could not be saved. Check storage access and retry.";

#[derive(Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct State {
    generation: u64,
    #[serde(default)]
    last_seen: Option<Ts>,
    failures: BTreeMap<String, u32>,
    backoff: BTreeMap<String, (u32, Ts)>,
    rate_paused_until: Option<Ts>,
}

impl MemoryService {
    fn read_retry(&self) -> Result<State, String> {
        let path = self.root.join(FILE);
        let generation = self.generation();
        let mut state = match std::fs::symlink_metadata(&path) {
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => State { generation, ..State::default() },
            Ok(meta) if meta.is_file() && !meta.file_type().is_symlink() && meta.len() <= 1_048_576 => {
                let bytes = std::fs::read(&path).map_err(|_| UNREADABLE.to_owned())?;
                serde_json::from_slice::<State>(&bytes).map_err(|_| UNREADABLE.to_owned())?
            }
            _ => return Err(UNREADABLE.into()),
        };
        if state.failures.len() > 4096 || state.backoff.len() > 4096
            || state.failures.keys().chain(state.backoff.keys()).any(|k| k.is_empty() || k.len() > 256) {
            return Err(UNREADABLE.into());
        }
        // Deletion invalidates session-specific attempts without bypassing a
        // provider cooldown shared by the remaining sessions.
        if state.generation != generation {
            state.generation = generation;
            state.failures.clear();
            state.backoff.clear();
        }
        Ok(state)
    }

    fn cache_retry(&self, state: State, saved: bool) {
        let mut inner = self.lock();
        inner.failures = state.failures;
        inner.backoff = state.backoff;
        inner.rate_paused_until = state.rate_paused_until;
        if saved || inner.retry_error.as_deref() != Some(UNWRITABLE) { inner.retry_error = None; }
    }

    /// Atomic reads are also safe inside a commit's store guard. Do not take
    /// another store lock here. A failed write stays blocked until preparation
    /// successfully writes the state again.
    pub(super) fn refresh_retry(&self) {
        match self.read_retry() {
            Ok(state) => self.cache_retry(state, false),
            Err(error) => self.lock().retry_error = Some(error),
        }
    }

    fn edit_retry_locked<T>(&self, edit: impl FnOnce(&mut State) -> T) -> Result<T, String> {
        let mut state = self.read_retry()?;
        let value = edit(&mut state);
        super::write_json(&self.root.join(FILE), &state).map_err(|_| UNWRITABLE.to_owned())?;
        self.cache_retry(state, true);
        Ok(value)
    }

    fn edit_retry<T>(&self, edit: impl FnOnce(&mut State) -> T) -> Result<T, String> {
        let result = (|| {
            let _guard = self.store()?;
            self.edit_retry_locked(edit)
        })();
        if let Err(error) = &result { self.lock().retry_error = Some(error.clone()); }
        result
    }

    /// Verify durable coordination before a model call, including after restart.
    pub(super) fn prepare_retry(&self) -> Result<(), String> { self.prepare_retry_at(super::now()) }

    pub(super) fn prepare_retry_at(&self, now: Ts) -> Result<(), String> {
        self.edit_retry(|state| {
            // A backward wall-clock correction must not turn a bounded wait
            // into hours or days. Rebase the remaining wait once, under lock.
            if let Some(previous) = state.last_seen.filter(|previous| now < *previous) {
                state.rate_paused_until = state.rate_paused_until.map(|until|
                    now.saturating_add(until.saturating_sub(previous).clamp(0, super::RATE_PAUSE_SECS)));
                for (_, until) in state.backoff.values_mut() {
                    *until = now.saturating_add(until.saturating_sub(previous).clamp(0, super::BACKOFF_CAP_SECS));
                }
            }
            state.last_seen = Some(now);
        })
    }

    pub(super) fn retry_count(&self, key: &str) -> u32 {
        self.refresh_retry();
        self.lock().failures.get(key).copied().unwrap_or(0)
    }

    pub(super) fn retry_refused(&self, key: &str, generation: u64) -> u32 {
        self.edit_retry(|state| {
            if state.generation != generation { return 0; }
            let count = state.failures.entry(key.to_owned()).or_default();
            *count = count.saturating_add(1);
            *count
        }).unwrap_or(u32::MAX)
    }

    pub(super) fn retry_backoff(&self, key: &str, now: Ts, generation: u64) {
        let _ = self.edit_retry(|state| {
            if state.generation != generation { return; }
            let n = state.backoff.get(key).map_or(0, |v| v.0).saturating_add(1);
            state.backoff.insert(key.to_owned(), (n, now.saturating_add(super::backoff_secs(n))));
            state.last_seen = Some(state.last_seen.map_or(now, |old| old.max(now)));
        });
    }

    pub(super) fn retry_clear(&self, key: &str, generation: u64) {
        let _ = self.edit_retry(|state| {
            if state.generation != generation { return; }
            state.failures.remove(key);
            state.backoff.remove(key);
        });
    }

    pub(super) fn retry_rate_limit(&self, now: Ts) {
        let _ = self.edit_retry(|state| {
            let until = now.saturating_add(super::RATE_PAUSE_SECS);
            state.rate_paused_until = Some(state.rate_paused_until.map_or(until, |old| old.max(until)));
            state.last_seen = Some(state.last_seen.map_or(now, |old| old.max(now)));
        });
    }

    /// The deletion/revision writer already owns the store lock. Drop identifiers from the file
    /// synchronously before acknowledging deletion, retaining only the cooldown.
    pub(super) fn clear_retry_locked(&self) -> Result<(), String> {
        // Explicit deletion can repair malformed retry metadata as well.
        let mut state = self.read_retry().unwrap_or_else(|_| State {
            generation: self.generation(), rate_paused_until: self.lock().rate_paused_until,
            ..State::default()
        });
        state.failures.clear();
        state.backoff.clear();
        super::write_json(&self.root.join(FILE), &state).map_err(|_| UNWRITABLE.to_owned())?;
        self.cache_retry(state, true);
        Ok(())
    }
}
