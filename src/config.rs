//! Environment-only application configuration.

use crate::error::{Error, Result};
use std::{net::SocketAddr, path::PathBuf, time::Duration};
use url::Url;

/// Server-wide service provider.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ServiceProvider {
    /// OpenAI Platform or ChatGPT plan access.
    OpenAI,
    /// OpenRouter API-key access.
    OpenRouter,
}

impl ServiceProvider {
    /// Environment variable containing the selected provider's API key.
    pub fn api_key_variable(self) -> &'static str {
        match self {
            Self::OpenAI => "OPENAI_API_KEY",
            Self::OpenRouter => "OPENROUTER_API_KEY",
        }
    }

    /// Reject authentication combinations the provider cannot serve.
    pub fn validate_auth(self, auth: AuthMode) -> Result<()> {
        if self == Self::OpenRouter && auth == AuthMode::OAuth {
            return Err(Error::new(
                "configuration",
                "OpenRouter requires LLM_MCP_AUTH=api_key; ChatGPT OAuth is OpenAI-only",
            ));
        }
        Ok(())
    }
}

/// Mutually exclusive server-wide authentication method.
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
    /// Selected service provider.
    pub provider: ServiceProvider,
    /// `stdio` or `http`.
    pub transport: String,
    /// HTTP listener.
    pub bind: SocketAddr,
    /// Host authorities accepted by the HTTP transport.
    pub allowed_hosts: Vec<String>,
    /// Selected provider base URL, including a trailing slash.
    pub base_url: Url,
    /// Selected provider key, used exclusively in API-key mode.
    pub api_key: Option<String>,
    /// Optional OpenAI organization.
    pub organization: Option<String>,
    /// Optional OpenAI project.
    pub project: Option<String>,
    /// Server-wide authentication selection.
    pub auth: AuthMode,
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
            .field("provider", &self.provider)
            .field("transport", &self.transport)
            .field("bind", &self.bind)
            .field("auth", &self.auth)
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
        let provider = match get("LLM_MCP_PROVIDER").as_deref().unwrap_or("openai") {
            "openai" => ServiceProvider::OpenAI,
            "openrouter" => ServiceProvider::OpenRouter,
            _ => {
                return Err(Error::new(
                    "configuration",
                    "LLM_MCP_PROVIDER must be openai or openrouter",
                ));
            }
        };
        let (base_variable, default_base) = match provider {
            ServiceProvider::OpenAI => ("OPENAI_BASE_URL", "https://api.openai.com/v1/"),
            ServiceProvider::OpenRouter => ("OPENROUTER_BASE_URL", "https://openrouter.ai/api/v1/"),
        };
        let mut base_url =
            Url::parse(&get(base_variable).unwrap_or_else(|| default_base.into()))
                .map_err(|_| Error::new("configuration", format!("invalid {base_variable}")))?;
        if !matches!(base_url.scheme(), "https" | "http")
            || base_url.host_str().is_none()
            || !base_url.username().is_empty()
            || base_url.password().is_some()
            || base_url.query().is_some()
            || base_url.fragment().is_some()
        {
            return Err(Error::new(
                "configuration",
                format!(
                    "{base_variable} must be an HTTP(S) URL without credentials, query or fragment"
                ),
            ));
        }
        if !base_url.path().ends_with('/') {
            base_url.set_path(&format!("{}/", base_url.path()));
        }
        let auth = match get("LLM_MCP_AUTH").as_deref().unwrap_or("api_key") {
            "api_key" => AuthMode::ApiKey,
            "oauth" => AuthMode::OAuth,
            _ => {
                return Err(Error::new(
                    "configuration",
                    "LLM_MCP_AUTH must be api_key or oauth",
                ));
            }
        };
        provider.validate_auth(auth)?;
        let home = get("HOME")
            .or_else(|| get("USERPROFILE"))
            .unwrap_or_else(|| ".".into());
        Ok(Self {
            provider,
            transport,
            bind,
            allowed_hosts: get("LLM_MCP_ALLOWED_HOSTS")
                .unwrap_or_else(|| "localhost,127.0.0.1,::1".into())
                .split(',')
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(str::to_owned)
                .collect(),
            base_url,
            auth,
            api_key: get(provider.api_key_variable()).filter(|s| !s.is_empty()),
            organization: (provider == ServiceProvider::OpenAI)
                .then(|| get("OPENAI_ORGANIZATION"))
                .flatten(),
            project: (provider == ServiceProvider::OpenAI)
                .then(|| get("OPENAI_PROJECT"))
                .flatten(),
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
