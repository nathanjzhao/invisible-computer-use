//! Shared observation controls. A pause/settings acknowledgement is a barrier against stale writes.
pub mod browser;
pub mod capture;
#[cfg(target_os = "linux")]
pub mod linux;
#[cfg(target_os = "linux")]
mod linux_atspi;
#[cfg(target_os = "linux")]
pub mod linux_input;
#[cfg(target_os = "linux")]
pub mod linux_gnome_input;
#[cfg(target_os = "macos")]
pub mod macos;
pub mod mcp;
pub mod policy;
pub mod store;
pub mod value_buffer;
#[cfg(target_os = "windows")]
pub mod windows;
#[cfg(target_os = "windows")]
mod windows_uia;
#[cfg(target_os = "windows")]
pub mod windows_input;
use serde::Serialize;
#[derive(Debug, Clone, Serialize)]
pub struct Error {
    pub code: String,
    pub message: String,
}
pub type Result<T> = std::result::Result<T, Error>;
impl Error {
    pub fn new(code: &str, message: impl Into<String>) -> Self {
        Self {
            code: code.into(),
            message: message.into(),
        }
    }
    pub fn invalid(message: impl Into<String>) -> Self {
        Self::new("invalidArguments", message)
    }
}
impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {}", self.code, self.message)
    }
}
impl std::error::Error for Error {}
pub fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .min(u64::MAX as u128) as u64
}
