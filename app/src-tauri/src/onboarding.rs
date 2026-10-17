//! In-app model onboarding: lets a Finder-launched `.app` be given an API key
//! without a terminal. Keys live in the app's own data dir
//! (`app_local_data_dir()/onboarding.json`, mode 0600 on unix), never the user's
//! personal Keychain. A process-global cache lets `brain.rs` read a stored key
//! without an `AppHandle`; an `ANTHROPIC_API_KEY` / `OPENAI_API_KEY` in the
//! environment still wins, so a developer's setup is unchanged. (ONBOARD-APIKEY.)
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::sync::RwLock;

#[derive(Debug, Default, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StoredKeys {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub anthropic: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub openai: Option<String>,
}

static CACHE: RwLock<Option<StoredKeys>> = RwLock::new(None);

/// A trimmed, non-empty value, else `None`.
fn clean(value: &str) -> Option<String> {
    let trimmed = value.trim();
    (!trimmed.is_empty()).then(|| trimmed.to_string())
}

/// The key to use: the environment value when present, otherwise the stored one.
/// Blank/whitespace counts as absent, and both are trimmed. (The env var wins so
/// a developer's `ANTHROPIC_API_KEY` is never overridden by a stored key.)
pub fn resolve(env_value: Option<String>, stored: Option<String>) -> Option<String> {
    env_value.as_deref().and_then(clean).or_else(|| stored.as_deref().and_then(clean))
}

fn read_from(path: &Path) -> StoredKeys {
    std::fs::read(path)
        .ok()
        .and_then(|bytes| serde_json::from_slice::<StoredKeys>(&bytes).ok())
        .unwrap_or_default()
}

fn write_to(path: &Path, keys: &StoredKeys) -> Result<(), String> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    }
    let bytes = serde_json::to_vec_pretty(keys).map_err(|e| e.to_string())?;
    std::fs::write(path, bytes).map_err(|e| e.to_string())?;
    // The file holds a secret: keep it readable only by the user who owns the app.
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600));
    }
    Ok(())
}

fn file_path<R: tauri::Runtime>(app: &tauri::AppHandle<R>) -> Result<PathBuf, String> {
    use tauri::Manager;
    Ok(app.path().app_local_data_dir().map_err(|e| e.to_string())?.join("onboarding.json"))
}

/// Load the stored keys into the process-global cache. Call once at startup.
pub fn load_into_cache<R: tauri::Runtime>(app: &tauri::AppHandle<R>) {
    let keys = file_path(app).map(|p| read_from(&p)).unwrap_or_default();
    *CACHE.write().unwrap() = Some(keys);
}

fn cached() -> StoredKeys {
    CACHE.read().unwrap().clone().unwrap_or_default()
}

/// The stored Anthropic key, if any. `brain.rs` prefers the env var over this.
pub fn stored_anthropic_key() -> Option<String> {
    cached().anthropic
}

/// The stored OpenAI key, if any. `brain.rs` prefers the env var over this.
pub fn stored_openai_key() -> Option<String> {
    cached().openai
}

/// Set (or, when blank, clear) one provider's key: persist it and update the cache.
pub fn set_key<R: tauri::Runtime>(app: &tauri::AppHandle<R>, provider: &str, key: &str) -> Result<(), String> {
    let mut keys = cached();
    let value = clean(key);
    match provider {
        "anthropic" => keys.anthropic = value,
        "openai" => keys.openai = value,
        other => return Err(format!("unknown provider: {other}")),
    }
    write_to(&file_path(app)?, &keys)?;
    *CACHE.write().unwrap() = Some(keys);
    Ok(())
}

/// Whether a fresh user still has no usable model. A usable model is an Anthropic
/// key (env or stored), the `claude` CLI (a subscription login), or an OpenAI key
/// (env or stored). The keyless local model needs Ollama installed, so it does
/// not count as "configured" for someone who just opened the app.
pub fn needs_onboarding(anthropic: bool, claude_cli: bool, openai: bool) -> bool {
    !(anthropic || claude_cli || openai)
}

/// The onboarding status the UI branches on, folding env + stored + CLI together.
pub fn status(env_anthropic: bool, env_openai: bool, claude_cli: bool) -> Value {
    let stored = cached();
    let anthropic = env_anthropic || stored.anthropic.is_some();
    let openai = env_openai || stored.openai.is_some();
    json!({
        "needsOnboarding": needs_onboarding(anthropic, claude_cli, openai),
        "anthropic": anthropic,
        "openai": openai,
        "claudeCli": claude_cli,
        "storedAnthropic": stored.anthropic.is_some(),
        "storedOpenai": stored.openai.is_some(),
        "envAnthropic": env_anthropic,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn env_key_wins_over_stored_and_blanks_fall_back() {
        // No key anywhere.
        assert_eq!(resolve(None, None), None);
        // The stored key is used when the env var is absent (the core requirement).
        assert_eq!(resolve(None, Some("sk-stored".into())), Some("sk-stored".into()));
        // The env var wins when both are present.
        assert_eq!(resolve(Some("sk-env".into()), Some("sk-stored".into())), Some("sk-env".into()));
        // A blank/whitespace env var counts as absent: the stored key is used.
        assert_eq!(resolve(Some("   ".into()), Some("sk-stored".into())), Some("sk-stored".into()));
        // Both sides are trimmed.
        assert_eq!(resolve(Some("  sk-env \n".into()), None), Some("sk-env".into()));
        assert_eq!(resolve(None, Some(" sk-stored ".into())), Some("sk-stored".into()));
    }

    #[test]
    fn keys_round_trip_through_the_store_file_and_stay_private() {
        let dir = std::env::temp_dir().join(format!("icu-onboard-test-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        let path = dir.join("onboarding.json");
        let _ = std::fs::remove_file(&path);
        // A missing file reads as empty, not an error.
        assert_eq!(read_from(&path), StoredKeys::default());
        let keys = StoredKeys { anthropic: Some("sk-abc".into()), openai: None };
        write_to(&path, &keys).unwrap();
        assert_eq!(read_from(&path), keys);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(&path).unwrap().permissions().mode();
            assert_eq!(mode & 0o077, 0, "the stored key must not be group/other readable");
        }
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn needs_onboarding_only_when_no_usable_model_exists() {
        assert!(needs_onboarding(false, false, false));
        assert!(!needs_onboarding(true, false, false)); // anthropic key
        assert!(!needs_onboarding(false, true, false)); // claude CLI
        assert!(!needs_onboarding(false, false, true)); // openai key
    }
}
