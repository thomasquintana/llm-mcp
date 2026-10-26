//! Stable, credential-free application errors.

use serde::Serialize;

/// A structured tool failure. Provider bodies are deliberately not echoed.
#[derive(Debug, Clone, thiserror::Error, Serialize)]
#[error("{code}: {message}")]
pub struct Error {
    /// Stable machine-readable failure category.
    pub code: String,
    /// Diagnostic that does not contain request contents or credentials.
    pub message: String,
    /// Whether another attempt may succeed; never implies replay is safe.
    pub retryable: bool,
    /// Provider HTTP status, when available.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status: Option<u16>,
    /// Provider correlation identifier, when available.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub request_id: Option<String>,
}

impl Error {
    /// Construct an application error.
    pub fn new(code: &str, message: impl Into<String>) -> Self {
        Self {
            code: code.into(),
            message: message.into(),
            retryable: false,
            status: None,
            request_id: None,
        }
    }

    /// Convert an HTTP transport failure without exposing its URL or headers.
    pub fn transport(error: reqwest::Error) -> Self {
        let mut result = Self::new(
            if error.is_timeout() {
                "timeout"
            } else {
                "transport"
            },
            "OpenAI transport failed",
        );
        result.retryable = true;
        result
    }
}

/// Application result type.
pub type Result<T> = std::result::Result<T, Error>;
