//! Durable companion; independent public and device listeners.
pub mod auth;
pub mod db;
pub mod link;
pub mod reader;
pub mod server;
pub mod upstream;
use axum::{
    Json,
    http::StatusCode,
    response::{IntoResponse, Response},
};
use sha2::{Digest, Sha256};
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Error(pub u16, pub &'static str);
pub type Result<T> = std::result::Result<T, Error>;
impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{} {}", self.0, self.1)
    }
}
impl std::error::Error for Error {}
impl IntoResponse for Error {
    fn into_response(self) -> Response {
        (
            StatusCode::from_u16(self.0).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR),
            Json(serde_json::json!({"error":self.1})),
        )
            .into_response()
    }
}
impl From<rusqlite::Error> for Error {
    fn from(e: rusqlite::Error) -> Self {
        if matches!(
            e.sqlite_error_code(),
            Some(rusqlite::ErrorCode::DiskFull | rusqlite::ErrorCode::SystemIoFailure)
        ) {
            Self(507, "storage_full")
        } else {
            Self(503, "unavailable")
        }
    }
}
pub fn now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|t| t.as_secs() as i64)
        .unwrap_or(0)
}
pub fn hash(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}
pub fn invalid() -> Error {
    Error(400, "invalid_request")
}
