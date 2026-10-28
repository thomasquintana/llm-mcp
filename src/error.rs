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
    /// Failed input item in a multi-item preparation request, when available.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub input_index: Option<usize>,
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
            input_index: None,
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

    pub(crate) fn http(response: &reqwest::Response, authentication: bool) -> Self {
        let status = response.status();
        let mut error = Self::new(
            match status.as_u16() {
                429 => "rate_limited",
                500..=599 => "provider",
                _ if authentication || matches!(status.as_u16(), 401 | 403) => "authentication",
                400 | 422 => "provider_invalid_request",
                _ => "provider",
            },
            format!("OpenAI returned HTTP {}", status.as_u16()),
        );
        error.retryable = status.as_u16() == 429 || status.is_server_error();
        error.status = Some(status.as_u16());
        error.request_id = response
            .headers()
            .get("x-request-id")
            .and_then(|h| h.to_str().ok())
            .map(str::to_owned);
        error
    }

    pub(crate) fn context(mut self, status: u16, request_id: Option<String>) -> Self {
        if self.status.is_none() {
            self.status = Some(status);
        }
        if self.request_id.is_none() {
            self.request_id = request_id;
        }
        self
    }

    pub(crate) fn at_input(mut self, index: usize) -> Self {
        self.input_index = Some(index);
        self
    }
}

/// Application result type.
pub type Result<T> = std::result::Result<T, Error>;
