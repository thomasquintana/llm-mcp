//! Environment-only application configuration.

use crate::error::{Error, Result};
use std::{net::SocketAddr, path::PathBuf, time::Duration};
use url::Url;

/// Explicit authentication method for generation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AuthMode {
    /// Platform API key.
    ApiKey,
    /// ChatGPT plan OAuth.
    OAuth,
}

/// Settings read once at startup. Debug formatting never includes credentials.
#[derive(Clone)]
pub struct Config {
    /// `stdio` or `http`.
    pub transport: String,
    /// HTTP listener.
    pub bind: SocketAddr,
    /// Base URL including `/v1/`.
    pub base_url: Url,
    /// Platform key, used for classification and API-key generation.
    pub api_key: Option<String>,
    /// Optional OpenAI organization.
    pub organization: Option<String>,
    /// Optional OpenAI project.
    pub project: Option<String>,
    /// Generation authentication selection.
    pub generate_auth: AuthMode,
    /// OAuth credential file, on writable persistent storage.
    pub credentials_path: PathBuf,
    /// Total tool-call deadline, including queue time.
    pub timeout: Duration,
    /// Maximum in-flight provider calls per instance.
    pub concurrency: usize,
    /// Maximum serialized tool argument bytes.
    pub max_request_bytes: usize,
    /// Maximum provider response bytes.
    pub max_response_bytes: usize,
}

impl std::fmt::Debug for Config {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Config")
            .field("transport", &self.transport)
            .field("bind", &self.bind)
            .field("generate_auth", &self.generate_auth)
            .finish_non_exhaustive()
    }
}

impl Config {
    /// Read the process environment.
    pub fn from_env() -> Result<Self> {
        Self::from_lookup(|key| std::env::var(key).ok())
    }

    /// Parse settings with an injected environment lookup, without global mutation.
    pub fn from_lookup(get: impl Fn(&str) -> Option<String>) -> Result<Self> {
        let number = |key: &str, default: usize| -> Result<usize> {
            get(key).map_or(Ok(default), |s| {
                s.parse::<usize>().ok().filter(|n| *n > 0).ok_or_else(|| {
                    Error::new("configuration", format!("{key} must be a positive integer"))
                })
            })
        };
        let transport = get("LLM_MCP_TRANSPORT").unwrap_or_else(|| "stdio".into());
        if !matches!(transport.as_str(), "stdio" | "http") {
            return Err(Error::new(
                "configuration",
                "LLM_MCP_TRANSPORT must be stdio or http",
            ));
        }
        let bind = get("LLM_MCP_BIND")
            .unwrap_or_else(|| "127.0.0.1:8080".into())
            .parse()
            .map_err(|_| Error::new("configuration", "LLM_MCP_BIND must be an IP:port"))?;
        let mut base_url = Url::parse(
            &get("OPENAI_BASE_URL").unwrap_or_else(|| "https://api.openai.com/v1/".into()),
        )
        .map_err(|_| Error::new("configuration", "invalid OPENAI_BASE_URL"))?;
        if !matches!(base_url.scheme(), "https" | "http")
            || base_url.host_str().is_none()
            || !base_url.username().is_empty()
            || base_url.password().is_some()
            || base_url.query().is_some()
            || base_url.fragment().is_some()
        {
            return Err(Error::new(
                "configuration",
                "OPENAI_BASE_URL must be an HTTP(S) URL without credentials, query or fragment",
            ));
        }
        if !base_url.path().ends_with('/') {
            base_url.set_path(&format!("{}/", base_url.path()));
        }
        let generate_auth = match get("LLM_MCP_GENERATE_AUTH").as_deref().unwrap_or("api_key") {
            "api_key" => AuthMode::ApiKey,
            "oauth" => AuthMode::OAuth,
            _ => {
                return Err(Error::new(
                    "configuration",
                    "LLM_MCP_GENERATE_AUTH must be api_key or oauth",
                ));
            }
        };
        let home = get("HOME")
            .or_else(|| get("USERPROFILE"))
            .unwrap_or_else(|| ".".into());
        Ok(Self {
            transport,
            bind,
            base_url,
            generate_auth,
            api_key: get("OPENAI_API_KEY").filter(|s| !s.is_empty()),
            organization: get("OPENAI_ORGANIZATION"),
            project: get("OPENAI_PROJECT"),
            credentials_path: get("LLM_MCP_OAUTH_CREDENTIALS_PATH")
                .map(PathBuf::from)
                .unwrap_or_else(|| PathBuf::from(home).join(".config/llm-mcp/auth.json")),
            timeout: Duration::from_millis(number("LLM_MCP_TIMEOUT_MS", 120_000)? as u64),
            concurrency: number("LLM_MCP_MAX_CONCURRENCY", 16)?,
            max_request_bytes: number("LLM_MCP_MAX_REQUEST_BYTES", 8 * 1024 * 1024)?,
            max_response_bytes: number("LLM_MCP_MAX_RESPONSE_BYTES", 16 * 1024 * 1024)?,
        })
    }
}
