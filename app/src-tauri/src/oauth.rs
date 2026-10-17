//! Read the Codex login's credentials without rotating its tokens. (Claude is
//! reached by running Claude Code itself; see `brain::ClaudeCode`.)
//! The owning client remains responsible for refresh and persistence. This app
//! never writes auth.json or Keychain entries and never consumes refresh tokens.

use serde_json::Value;
use std::ffi::OsString;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

fn now_ms() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_millis() as u64
}

// ── Codex (OpenAI / ChatGPT) ────────────────────────────────────────────────
pub const CODEX_BASE_URL: &str = "https://chatgpt.com/backend-api";

fn codex_auth_json() -> Option<Value> {
    let path = codex_auth_path(std::env::var_os("CODEX_HOME"),
        std::env::var_os("HOME"), std::env::var_os("USERPROFILE"))?;
    let s = std::fs::read_to_string(path).ok()?;
    serde_json::from_str::<Value>(&s).ok()
}

fn codex_auth_path(codex: Option<OsString>, home: Option<OsString>, profile: Option<OsString>) -> Option<PathBuf> {
    let present = |value: &OsString| !value.is_empty();
    let base = if let Some(path) = codex.filter(present) {
        PathBuf::from(path)
    } else {
        PathBuf::from(home.filter(present).or_else(|| profile.filter(present))?).join(".codex")
    };
    Some(base.join("auth.json"))
}

/// The ChatGPT account id, from tokens.account_id or the access-token JWT claim.
fn codex_account_id(tokens: &Value, access: &str) -> Option<String> {
    if let Some(a) = tokens.get("account_id").and_then(Value::as_str) {
        return Some(a.to_string());
    }
    // decode the JWT payload (middle segment) and read the auth claim
    let payload = access.split('.').nth(1)?;
    let bytes = b64url(payload)?;
    let claims: Value = serde_json::from_slice(&bytes).ok()?;
    claims
        .get("https://api.openai.com/auth")
        .and_then(|a| a.get("chatgpt_account_id"))
        .and_then(Value::as_str)
        .map(String::from)
}

fn b64url(s: &str) -> Option<Vec<u8>> {
    use base64::Engine;
    let pad = (4 - s.len() % 4) % 4;
    let s = format!("{s}{}", "=".repeat(pad));
    base64::engine::general_purpose::URL_SAFE.decode(s).ok()
}

fn fresh_codex_token(root: &Value, now: u64) -> Option<(String, Option<String>)> {
    let tokens = root.get("tokens").unwrap_or(root);
    let access = tokens.get("access_token")?.as_str()?;
    let payload = b64url(access.split('.').nth(1)?)?;
    let claims: Value = serde_json::from_slice(&payload).ok()?;
    let expires = claims.get("exp")?.as_u64()?.checked_mul(1000)?;
    if expires <= now.saturating_add(60_000) { return None; }
    Some((access.to_owned(), codex_account_id(tokens, access)))
}

/// Borrow a fresh access token. Do not refresh another client's credentials:
/// rotation without persisting its replacement could strand that client's login.
pub fn codex_token() -> Option<(String, Option<String>)> {
    fresh_codex_token(&codex_auth_json()?, now_ms())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    #[test]
    fn login_path_uses_windows_profile_and_respects_explicit_codex_home() {
        let profile = OsString::from("profile");
        assert_eq!(codex_auth_path(None, None, Some(profile.clone())), Some(PathBuf::from("profile/.codex/auth.json")));
        assert_eq!(codex_auth_path(None, Some(OsString::new()), Some(profile.clone())), Some(PathBuf::from("profile/.codex/auth.json")));
        assert_eq!(codex_auth_path(Some("custom".into()), Some("home".into()), Some(profile)), Some(PathBuf::from("custom/auth.json")));
        assert_eq!(codex_auth_path(None, Some("home".into()), None), Some(PathBuf::from("home/.codex/auth.json")));
        assert_eq!(codex_auth_path(None, None, None), None);
    }
    #[test]
    fn expired_or_unknown_expiry_codex_tokens_are_never_reused() {
        assert!(fresh_codex_token(&json!({"tokens":{"access_token":"opaque","refresh_token":"do-not-use"}}), 0).is_none());
    }
    #[test]
    fn codex_expiry_and_account_come_from_the_same_current_token() {
        use base64::Engine;
        let claims = json!({"exp":1000,"https://api.openai.com/auth":{"chatgpt_account_id":"account"}});
        let token = format!("header.{}.signature", base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(claims.to_string()));
        let root = json!({"tokens":{"access_token":token,"refresh_token":"unused"}});
        assert_eq!(fresh_codex_token(&root, 0).unwrap().1.as_deref(), Some("account"));
        assert!(fresh_codex_token(&root, 950000).is_none());
    }
}
