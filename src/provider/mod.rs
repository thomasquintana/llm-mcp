//! Bounded OpenAI HTTP requests, preserving provider JSON.

use crate::{
    config::{AuthMode, Config},
    error::{Error, Result},
};
use reqwest::{Client, Response, header};
use serde_json::Value;
use std::sync::Arc;
use tokio::sync::Semaphore;
use tokio_util::sync::CancellationToken;

/// Which provider endpoint handles a tool call.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Operation {
    /// OpenAI Responses.
    Generate,
    /// OpenAI Decisions.
    Classify,
}

/// Reusable connection pool with bounded execution.
#[derive(Clone)]
pub struct Provider {
    /// Validated startup settings.
    pub config: Arc<Config>,
    client: Client,
    slots: Arc<Semaphore>,
}

impl Provider {
    /// Construct a provider. No network request is made.
    pub fn new(config: Config) -> Result<Self> {
        let client = Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .connect_timeout(std::time::Duration::from_secs(10))
            .build()
            .map_err(Error::transport)?;
        let slots = Arc::new(Semaphore::new(config.concurrency));
        Ok(Self {
            config: Arc::new(config),
            client,
            slots,
        })
    }

    /// Validate, queue and execute exactly one request. Dropping this future aborts local I/O.
    pub async fn call(
        &self,
        operation: Operation,
        arguments: Value,
        cancel: CancellationToken,
    ) -> Result<Value> {
        validate(&arguments)?;
        if serde_json::to_vec(&arguments)
            .map_err(|_| Error::new("invalid_request", "invalid JSON"))?
            .len()
            > self.config.max_request_bytes
        {
            return Err(Error::new(
                "request_too_large",
                "tool arguments exceed configured byte limit",
            ));
        }
        let execute = async {
            let _permit = self
                .slots
                .acquire()
                .await
                .map_err(|_| Error::new("shutdown", "server is shutting down"))?;
            self.execute(operation, arguments).await
        };
        tokio::select! {
            biased;
            () = cancel.cancelled() => Err(Error::new("cancelled", "tool call cancelled")),
            result = tokio::time::timeout(self.config.timeout, execute) => result.unwrap_or_else(|_| {
                let mut error = Error::new("timeout", "tool deadline exceeded"); error.retryable = true; Err(error)
            }),
        }
    }

    async fn execute(&self, operation: Operation, arguments: Value) -> Result<Value> {
        if operation == Operation::Generate && self.config.generate_auth == AuthMode::OAuth {
            return Err(Error::new(
                "authentication",
                "OAuth credentials are not initialized",
            ));
        }
        let key = self.config.api_key.as_deref().ok_or_else(|| {
            Error::new(
                "authentication",
                "OPENAI_API_KEY is required for this operation",
            )
        })?;
        let endpoint = if operation == Operation::Generate {
            "responses"
        } else {
            "decisions"
        };
        let url = self
            .config
            .base_url
            .join(endpoint)
            .map_err(|_| Error::new("configuration", "invalid provider endpoint"))?;
        let mut request = self
            .client
            .post(url)
            .bearer_auth(key)
            .header(header::ACCEPT, "application/json")
            .json(&arguments);
        if let Some(value) = &self.config.organization {
            request = request.header("OpenAI-Organization", value);
        }
        if let Some(value) = &self.config.project {
            request = request.header("OpenAI-Project", value);
        }
        let response = request.send().await.map_err(Error::transport)?;
        let status = response.status();
        let request_id = response
            .headers()
            .get("x-request-id")
            .and_then(|h| h.to_str().ok())
            .map(str::to_owned);
        if !status.is_success() {
            let mut error = Error::new(
                match status.as_u16() {
                    401 | 403 => "authentication",
                    429 => "rate_limited",
                    400 | 422 => "provider_invalid_request",
                    _ => "provider",
                },
                format!("OpenAI returned HTTP {}", status.as_u16()),
            );
            error.status = Some(status.as_u16());
            error.request_id = request_id;
            error.retryable = status.as_u16() == 429 || status.is_server_error();
            return Err(error);
        }
        let bytes = read_bounded(response, self.config.max_response_bytes).await?;
        let value: Value = serde_json::from_slice(&bytes)
            .map_err(|_| Error::new("provider_protocol", "OpenAI returned invalid JSON"))?;
        if !value.is_object() {
            return Err(Error::new(
                "provider_protocol",
                "OpenAI response must be an object",
            ));
        }
        if operation == Operation::Generate
            && value
                .get("status")
                .and_then(Value::as_str)
                .is_some_and(|s| {
                    matches!(
                        s,
                        "failed" | "incomplete" | "queued" | "in_progress" | "cancelled"
                    )
                })
        {
            return Err(Error::new(
                "provider_incomplete",
                "generation did not complete",
            ));
        }
        Ok(value)
    }
}

/// Validate server-level restrictions without restricting future provider fields.
pub fn validate(arguments: &Value) -> Result<()> {
    let object = arguments
        .as_object()
        .ok_or_else(|| Error::new("invalid_request", "tool arguments must be an object"))?;
    for field in ["stream", "background"] {
        if let Some(value) = object.get(field)
            && value != &Value::Bool(false)
        {
            return Err(Error::new(
                "invalid_request",
                format!("{field} must be false or omitted"),
            ));
        }
    }
    Ok(())
}

/// Read a response without trusting Content-Length or allocating an unbounded body.
pub async fn read_bounded(mut response: Response, limit: usize) -> Result<Vec<u8>> {
    let mut bytes = Vec::new();
    while let Some(chunk) = response.chunk().await.map_err(Error::transport)? {
        if chunk.len() > limit.saturating_sub(bytes.len()) {
            return Err(Error::new(
                "response_too_large",
                "provider response exceeds configured byte limit",
            ));
        }
        bytes.extend_from_slice(&chunk);
    }
    Ok(bytes)
}
