use serde::Serialize;

pub type Result<T> = std::result::Result<T, Error>;

/// Excludes raw requests, URLs and headers to avoid leaking credentials.
#[derive(Debug, thiserror::Error, Serialize)]
#[error("{code}: {message}")]
pub struct Error {
    pub code: String,
    pub message: String,
    pub status: Option<u16>,
    pub request_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub task_id: Option<Box<str>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub idempotency_key: Option<Box<str>>,
    pub retryable: bool,
}
impl Error {
    pub fn new(code: &str, message: impl Into<String>) -> Self {
        Self {
            code: code.into(),
            message: message.into(),
            status: None,
            request_id: None,
            task_id: None,
            idempotency_key: None,
            retryable: false,
        }
    }
    pub(crate) fn transport() -> Self {
        Self {
            retryable: true,
            ..Self::new(
                "TRANSPORT_ERROR",
                "Request failed; the server may have accepted it",
            )
        }
    }
    pub(crate) fn protocol() -> Self {
        Self::new("INVALID_RESPONSE", "Server returned an unexpected response")
    }
}
