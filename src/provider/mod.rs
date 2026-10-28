//! Bounded OpenAI HTTP requests, preserving provider JSON.

mod embed;
mod media;
mod sse;
mod transcribe;

use crate::{
    auth::OAuthSession,
    config::{AuthMode, Config},
    error::{Error, Result},
};
use reqwest::{Client, Response, header};
use serde_json::Value;
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};
use tokio::sync::Semaphore;
use tokio_util::sync::CancellationToken;
use tokio_util::task::{TaskTracker, task_tracker::TaskTrackerToken};

/// Which provider endpoint handles a tool call.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Operation {
    /// OpenAI Responses.
    Generate,
    /// OpenAI Decisions.
    Classify,
    /// OpenAI text embeddings, optionally prepared from image descriptions.
    Embed,
    /// OpenAI audio transcription via a base64-to-multipart upload.
    Transcribe,
}

/// Reusable connection pool with bounded execution.
#[derive(Clone)]
pub struct Provider {
    /// Validated startup settings.
    pub config: Arc<Config>,
    client: Client,
    slots: Arc<Semaphore>,
    cpu_slots: Arc<Semaphore>,
    stopping: Arc<AtomicBool>,
    active: TaskTracker,
    oauth: Option<Arc<OAuthSession>>,
}

impl Provider {
    /// Construct a provider. No network request is made.
    pub fn new(config: Config) -> Result<Self> {
        match config.auth {
            AuthMode::ApiKey if config.api_key.is_none() => {
                return Err(Error::new(
                    "configuration",
                    "API-key mode requires OPENAI_API_KEY",
                ));
            }
            AuthMode::OAuth if config.api_key.is_some() => {
                return Err(Error::new(
                    "configuration",
                    "OAuth mode conflicts with OPENAI_API_KEY",
                ));
            }
            _ => {}
        }
        let client = client_builder().build().map_err(Error::transport)?;
        let slots = Arc::new(Semaphore::new(config.concurrency));
        let cpu_slots = Arc::new(Semaphore::new(config.concurrency));
        let oauth = if config.auth == AuthMode::OAuth {
            Some(Arc::new(OAuthSession::open(&config.credentials_path)?))
        } else {
            None
        };
        Ok(Self {
            config: Arc::new(config),
            client,
            cpu_slots,
            slots,
            stopping: Arc::new(AtomicBool::new(false)),
            active: TaskTracker::new(),
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
        self.call_until(
            operation,
            arguments,
            cancel,
            tokio::time::Instant::now() + self.config.timeout,
        )
        .await
    }

    pub(crate) async fn call_until(
        &self,
        operation: Operation,
        arguments: Value,
        cancel: CancellationToken,
        deadline: tokio::time::Instant,
    ) -> Result<Value> {
        let _activity = self.activity();
        if self.config.auth == AuthMode::OAuth && operation != Operation::Generate {
            return Err(Error::new(
                "tool_disabled",
                "this tool is disabled in OAuth mode",
            ));
        }
        if self.stopping.load(Ordering::Acquire) {
            return Err(Error::new("shutdown", "server is shutting down"));
        }
        let execute = async {
            let _permit = self
                .slots
                .acquire()
                .await
                .map_err(|_| Error::new("shutdown", "server is shutting down"))?;
            self.execute(operation, arguments).await
        };
        let result = tokio::select! {
            biased;
            () = cancel.cancelled() => Err(Error::new("cancelled", "tool call cancelled")),
            result = tokio::time::timeout_at(deadline, execute) => result.unwrap_or_else(|_| Err(deadline_error())),
        };
        if cancel.is_cancelled() {
            return Err(Error::new("cancelled", "tool call cancelled"));
        }
        if tokio::time::Instant::now() >= deadline {
            return Err(deadline_error());
        }
        result
    }

    pub(crate) async fn cpu<T: Send + 'static>(
        &self,
        work: impl FnOnce() -> T + Send + 'static,
    ) -> Result<T> {
        let permit = self
            .cpu_slots
            .clone()
            .acquire_owned()
            .await
            .map_err(|_| Error::new("shutdown", "server is shutting down"))?;
        tokio::task::spawn_blocking(move || {
            let _permit = permit;
            work()
        })
        .await
        .map_err(|_| Error::new("internal", "JSON worker failed"))
    }

    /// Stop admitting work and stop starting new OAuth refresh operations.
    pub fn stop(&self) {
        self.stopping.store(true, Ordering::Release);
        self.slots.close();
        if let Some(oauth) = &self.oauth {
            oauth.stop();
        }
        self.active.close();
    }

    /// Join any rotating credential work before the runtime exits.
    pub async fn shutdown(&self) {
        self.stop();
        let _ = tokio::time::timeout(
            self.config.timeout + std::time::Duration::from_secs(5),
            self.active.wait(),
        )
        .await;
        if let Some(oauth) = &self.oauth {
            oauth.shutdown().await;
        }
    }

    pub(crate) fn activity(&self) -> TaskTrackerToken {
        self.active.token()
    }

    async fn execute(&self, operation: Operation, mut arguments: Value) -> Result<Value> {
        let limit = self.config.max_request_bytes;
        arguments = self
            .cpu(move || {
                validate(&arguments)?;
                bounded_json(&arguments, limit, true)?;
                if matches!(operation, Operation::Generate | Operation::Classify) {
                    media::validate_images(&arguments, limit)?;
                }
                Ok(arguments)
            })
            .await??;
        let value = match operation {
            Operation::Embed => self.embed(arguments).await?,
            Operation::Transcribe => self.transcribe(arguments).await?,
            _ => self.json(operation, arguments).await?,
        };
        let limit = self.config.max_response_bytes;
        self.cpu(move || {
            bounded_json(&value, limit, false)?;
            Ok(value)
        })
        .await?
    }

    async fn json(&self, operation: Operation, mut arguments: Value) -> Result<Value> {
        let oauth = self.config.auth == AuthMode::OAuth;
        let limit = self.config.max_request_bytes;
        let bytes = self
            .cpu(move || {
                if oauth {
                    prepare_oauth(&mut arguments)?;
                }
                bounded_json(&arguments, limit, true)
            })
            .await??;
        let endpoint = match operation {
            Operation::Generate => "responses",
            Operation::Classify => "decisions",
            Operation::Embed => "embeddings",
            Operation::Transcribe => unreachable!("transcription uses multipart"),
        };
        let request = self
            .request(endpoint, oauth)
            .await?
            .header(
                header::ACCEPT,
                if oauth {
                    "text/event-stream"
                } else {
                    "application/json"
                },
            )
            .header(header::CONTENT_TYPE, "application/json")
            .body(bytes);
        let response = self.send(request).await?;
        let status = response.status();
        let request_id = response
            .headers()
            .get("x-request-id")
            .and_then(|h| h.to_str().ok())
            .map(str::to_owned);
        self.decode_response(operation, response, oauth)
            .await
            .map_err(|error| error.context(status.as_u16(), request_id))
    }

    async fn request(&self, endpoint: &str, oauth: bool) -> Result<reqwest::RequestBuilder> {
        let key = if oauth {
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
        let url = self
            .config
            .base_url
            .join(endpoint)
            .map_err(|_| Error::new("configuration", "invalid provider endpoint"))?;
        let mut request = self.client.post(url).bearer_auth(key);
        if !oauth && let Some(value) = &self.config.organization {
            request = request.header("OpenAI-Organization", value);
        }
        if !oauth && let Some(value) = &self.config.project {
            request = request.header("OpenAI-Project", value);
        }
        Ok(request)
    }

    async fn send(&self, request: reqwest::RequestBuilder) -> Result<Response> {
        let request = request.build().map_err(Error::transport)?;
        if request
            .headers()
            .get(header::CONTENT_LENGTH)
            .and_then(|h| h.to_str().ok())
            .and_then(|s| s.parse::<u64>().ok())
            .is_some_and(|n| n > self.config.max_request_bytes as u64)
        {
            return Err(Error::new(
                "request_too_large",
                "upload exceeds configured byte limit",
            ));
        }
        let response = self
            .client
            .execute(request)
            .await
            .map_err(Error::transport)?;
        if !response.status().is_success() {
            return Err(Error::http(&response, false));
        }
        Ok(response)
    }

    async fn decode_response(
        &self,
        operation: Operation,
        response: Response,
        oauth: bool,
    ) -> Result<Value> {
        if oauth {
            return sse::collect(response, self.config.max_response_bytes, self).await;
        }
        let bytes = read_bounded(response, self.config.max_response_bytes).await?;
        let value: Value = self
            .cpu(move || serde_json::from_slice(&bytes))
            .await?
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

/// Serialize without allocating beyond the configured boundary.
fn bounded_json(value: &impl serde::Serialize, limit: usize, request: bool) -> Result<Vec<u8>> {
    struct Writer {
        bytes: Vec<u8>,
        limit: usize,
    }
    impl std::io::Write for Writer {
        fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
            if bytes.len() > self.limit.saturating_sub(self.bytes.len()) {
                return Err(std::io::Error::other("byte limit"));
            }
            self.bytes.extend_from_slice(bytes);
            Ok(bytes.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
    let mut writer = Writer {
        bytes: Vec::new(),
        limit,
    };
    serde_json::to_writer(&mut writer, value).map_err(|_| {
        Error::new(
            if request {
                "request_too_large"
            } else {
                "response_too_large"
            },
            if request {
                "request exceeds configured byte limit"
            } else {
                "assembled result exceeds configured byte limit"
            },
        )
    })?;
    Ok(writer.bytes)
}

fn client_builder() -> reqwest::ClientBuilder {
    Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .retry(reqwest::retry::never())
        .connect_timeout(std::time::Duration::from_secs(10))
}

pub(crate) fn deadline_error() -> Error {
    let mut error = Error::new("timeout", "tool deadline exceeded");
    error.retryable = true;
    error
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

#[cfg(test)]
mod tests;
