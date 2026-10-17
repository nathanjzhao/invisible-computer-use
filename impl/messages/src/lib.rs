//! Native Messages data and review operations. Transport consent is a separate layer.
pub mod activity;
pub mod attachment_image;
pub mod body;
pub mod consent;
pub mod contacts;
pub mod contract;
pub mod cursor;
pub mod database;
pub mod dates;
pub mod engine;
pub mod mcp;
pub mod native_bridge;
pub mod native_delivery;
pub mod send;
pub mod send_journal;
pub mod worker;

use serde::{Deserialize, Serialize};
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Error {
    pub code: String,
    pub message: String,
}
pub type Result<T> = std::result::Result<T, Error>;
impl Error {
    pub fn new(code: &'static str, message: impl Into<String>) -> Self {
        Self {
            code: code.into(),
            message: message.into(),
        }
    }
}
impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {}", self.code, self.message)
    }
}
impl std::error::Error for Error {}
impl From<rusqlite::Error> for Error {
    fn from(error: rusqlite::Error) -> Self {
        let timeout = matches!(&error,rusqlite::Error::SqliteFailure(e,_) if e.code==rusqlite::ErrorCode::OperationInterrupted);
        Self::new(
            if timeout {
                "messagesQueryTimedOut"
            } else {
                "messagesDatabaseError"
            },
            if timeout {
                "The Messages query exceeded its deadline.".into()
            } else {
                error.to_string()
            },
        )
    }
}
pub(crate) fn invalid(message: impl Into<String>) -> Error {
    Error::new("invalidArguments", message)
}
