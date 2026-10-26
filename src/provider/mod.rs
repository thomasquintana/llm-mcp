//! Bounded OpenAI HTTP requests, preserving provider JSON.

mod sse;

use crate::{
    auth::OAuthSession,
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
    oauth: Option<Arc<OAuthSession>>,
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
        let oauth = if config.generate_auth == AuthMode::OAuth {
            Some(Arc::new(OAuthSession::open(
                &config.credentials_path,
                client.clone(),
            )?))
        } else {
            None
        };
        Ok(Self {
            config: Arc::new(config),
            client,
            slots,
            oauth,
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

    async fn execute(&self, operation: Operation, mut arguments: Value) -> Result<Value> {
        let oauth =
            operation == Operation::Generate && self.config.generate_auth == AuthMode::OAuth;
        let key = if oauth {
            prepare_oauth(&mut arguments)?;
            self.oauth
                .as_ref()
                .expect("OAuth initialized")
                .access_token()
                .await?
        } else {
            self.config.api_key.clone().ok_or_else(|| {
                Error::new(
                    "authentication",
                    "OPENAI_API_KEY is required for this operation",
                )
            })?
        };
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
            .header(
                header::ACCEPT,
                if oauth {
                    "text/event-stream"
                } else {
                    "application/json"
                },
            )
            .json(&arguments);
        if !oauth && let Some(value) = &self.config.organization {
            request = request.header("OpenAI-Organization", value);
        }
        if !oauth && let Some(value) = &self.config.project {
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
        if oauth {
            return sse::collect(response, self.config.max_response_bytes).await;
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

fn prepare_oauth(arguments: &mut Value) -> Result<()> {
    for field in [
        "conversation",
        "max_output_tokens",
        "max_tool_calls",
        "metadata",
        "moderation",
        "multi_agent",
        "prompt",
        "prompt_cache_retention",
        "safety_identifier",
        "temperature",
        "top_logprobs",
        "top_p",
        "truncation",
        "user",
        "previous_response_id",
    ] {
        if arguments.get(field).is_some() {
            return Err(Error::new(
                "unsupported_oauth_option",
                format!(
                    "{field} is not supported by ChatGPT plan OAuth; select API-key authentication to use standard API options"
                ),
            ));
        }
    }
    if arguments
        .get("store")
        .is_some_and(|v| v != &Value::Bool(false))
    {
        return Err(Error::new(
            "unsupported_oauth_option",
            "ChatGPT OAuth requires store:false",
        ));
    }
    if !arguments.get("input").is_some_and(Value::is_array) {
        return Err(Error::new(
            "unsupported_oauth_option",
            "ChatGPT OAuth requires input as an array containing full context",
        ));
    }
    let object = arguments.as_object_mut().expect("validated object");
    object.remove("background");
    object.insert("store".into(), Value::Bool(false));
    object.insert("stream".into(), Value::Bool(true));
    Ok(())
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
