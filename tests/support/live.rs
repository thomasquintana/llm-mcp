//! Independent MCP process driver and credential gates shared by live tests/regressions.

use llm_mcp::config::Config;
use serde_json::{Value, json};
use std::{collections::HashMap, path::PathBuf, process::Stdio, time::Duration};
use tokio::{
    io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader},
    net::TcpListener,
    process::{Child, ChildStdin, ChildStdout, Command},
};

pub const OPENAI: &str = "https://api.openai.com/v1/";
const MAX_RESPONSE: usize = 2 * 1024 * 1024;

pub type Result<T> = std::result::Result<T, Failure>;

#[derive(Debug)]
pub struct Failure(pub String);

impl Failure {
    pub fn new(message: &str) -> Self {
        Self(message.to_owned())
    }
}

impl std::fmt::Display for Failure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

pub enum Credential {
    ApiKey(String),
    OAuth(PathBuf),
}

impl Credential {
    pub fn mode(&self) -> &'static str {
        match self {
            Self::ApiKey(_) => "api_key",
            Self::OAuth(_) => "oauth",
        }
    }
}

pub fn api_key(get: impl Fn(&str) -> Option<String>) -> Option<Credential> {
    get("OPENAI_API_KEY")
        .filter(|s| !s.is_empty())
        .map(Credential::ApiKey)
}

pub fn oauth(get: impl Fn(&str) -> Option<String>) -> Result<Option<Credential>> {
    let config = Config::from_lookup(|key| match key {
        "HOME" | "USERPROFILE" | "LLM_MCP_OAUTH_CREDENTIALS_PATH" => get(key),
        _ => None,
    })
    .map_err(|_| Failure::new("cannot resolve OAuth credential path"))?;
    for path in [
        &config.credentials_path,
        &config.credentials_path.with_extension("pending.json"),
    ] {
        match std::fs::symlink_metadata(path) {
            Ok(_) => return Ok(Some(Credential::OAuth(config.credentials_path.clone()))),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(_) => {
                return Err(Failure::new(
                    "cannot inspect configured OAuth credential record",
                ));
            }
        }
    }
    Ok(None)
}

#[derive(Clone, Copy)]
pub enum Transport {
    Stdio,
    Http,
}

impl Transport {
    pub fn name(self) -> &'static str {
        match self {
            Self::Stdio => "stdio",
            Self::Http => "http",
        }
    }
}

fn environment(
    credential: &Credential,
    endpoint: &str,
    timeout: Duration,
) -> HashMap<String, String> {
    let mut env = HashMap::from([
        ("LLM_MCP_AUTH".into(), credential.mode().into()),
        ("OPENAI_BASE_URL".into(), endpoint.into()),
        ("LLM_MCP_TIMEOUT_MS".into(), timeout.as_millis().to_string()),
        ("LLM_MCP_MAX_CONCURRENCY".into(), "1".into()),
        ("RUST_LOG".into(), "off".into()),
    ]);
    match credential {
        Credential::ApiKey(key) => {
            env.insert("OPENAI_API_KEY".into(), key.clone());
        }
        Credential::OAuth(path) => {
            env.insert(
                "LLM_MCP_OAUTH_CREDENTIALS_PATH".into(),
                path.to_string_lossy().into_owned(),
            );
        }
    }
    env
}

pub fn command(
    binary: &str,
    credential: &Credential,
    endpoint: &str,
    timeout: Duration,
) -> Command {
    let mut command = Command::new(binary);
    // Preserve OS/TLS/proxy settings, but never inherit application settings or
    // the other authentication mode. In particular, do not inherit a mock URL.
    for (key, _) in std::env::vars_os() {
        let name = key.to_string_lossy();
        if name.starts_with("LLM_MCP_") || name.starts_with("OPENAI_") || name == "RUST_LOG" {
            command.env_remove(key);
        }
    }
    command
        .env_remove("OPENAI_API_KEY")
        .env_remove("LLM_MCP_OAUTH_CREDENTIALS_PATH");
    command
        .arg("serve")
        .envs(environment(credential, endpoint, timeout))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .kill_on_drop(true);
    if matches!(credential, Credential::ApiKey(_)) {
        for key in ["OPENAI_ORGANIZATION", "OPENAI_PROJECT"] {
            if let Some(value) = std::env::var_os(key) {
                command.env(key, value);
            }
        }
    }
    command
}

enum Wire {
    Stdio {
        input: Option<ChildStdin>,
        output: BufReader<ChildStdout>,
    },
    Http {
        client: reqwest::Client,
        url: String,
        session: Option<String>,
    },
}

pub struct Server {
    child: Child,
    wire: Wire,
    next_id: u64,
    timeout: Duration,
}

impl Server {
    pub async fn start(
        binary: &str,
        credential: &Credential,
        transport: Transport,
        endpoint: &str,
        timeout: Duration,
    ) -> Result<Self> {
        let mut command = command(binary, credential, endpoint, timeout);
        let url = if matches!(transport, Transport::Http) {
            let listener = TcpListener::bind("127.0.0.1:0")
                .await
                .map_err(|_| Failure::new("cannot reserve local MCP port"))?;
            let address = listener
                .local_addr()
                .map_err(|_| Failure::new("cannot inspect local MCP port"))?;
            command
                .env("LLM_MCP_TRANSPORT", "http")
                .env("LLM_MCP_BIND", address.to_string());
            Some(format!("http://{address}"))
        } else {
            command.env("LLM_MCP_TRANSPORT", "stdio");
            None
        };
        let mut child = command
            .spawn()
            .map_err(|_| Failure::new("cannot start MCP server binary"))?;
        let wire = if let Some(base) = url {
            let client = reqwest::Client::builder()
                .no_proxy()
                .redirect(reqwest::redirect::Policy::none())
                .retry(reqwest::retry::never())
                .timeout(timeout + Duration::from_secs(10))
                .build()
                .map_err(|_| Failure::new("cannot construct local MCP HTTP client"))?;
            let ready = tokio::time::timeout(Duration::from_secs(15), async {
                loop {
                    if child.try_wait().map_err(|_| Failure::new("cannot inspect MCP process"))?.is_some() {
                        return Err(Failure::new("MCP server exited during startup; check credentials and exclusive OAuth ownership"));
                    }
                    if client.get(format!("{base}/health/ready")).timeout(Duration::from_secs(1)).send().await.is_ok_and(|r| r.status().is_success()) { break; }
                    tokio::time::sleep(Duration::from_millis(50)).await;
                }
                Ok(())
            }).await.map_err(|_| Failure::new("MCP server readiness timed out"))?;
            ready?;
            Wire::Http {
                client,
                url: format!("{base}/mcp"),
                session: None,
            }
        } else {
            Wire::Stdio {
                input: child.stdin.take(),
                output: BufReader::new(
                    child
                        .stdout
                        .take()
                        .ok_or_else(|| Failure::new("MCP stdout unavailable"))?,
                ),
            }
        };
        let mut server = Self {
            child,
            wire,
            next_id: 1,
            timeout,
        };
        let initialized = server.rpc("initialize", Some(json!({"protocolVersion":"2025-11-25","capabilities":{},"clientInfo":{"name":"live-provider-e2e","version":"1"}}))).await?;
        if initialized["protocolVersion"] != "2025-11-25"
            || initialized["serverInfo"]["name"] != "llm-mcp"
        {
            return Err(Failure::new("MCP server identity/protocol mismatch"));
        }
        server.notify_initialized().await?;
        Ok(server)
    }

    async fn notify_initialized(&mut self) -> Result<()> {
        let request = json!({"jsonrpc":"2.0","method":"notifications/initialized"});
        match &mut self.wire {
            Wire::Stdio { input, .. } => write_line(input, &request).await,
            Wire::Http {
                client,
                url,
                session,
            } => {
                let response = http_post(client, url, session.as_deref(), &request)
                    .send()
                    .await
                    .map_err(|_| Failure::new("MCP initialized notification failed"))?;
                if !response.status().is_success() {
                    return Err(Failure::new("MCP initialized notification rejected"));
                }
                Ok(())
            }
        }
    }

    pub async fn rpc(&mut self, method: &str, params: Option<Value>) -> Result<Value> {
        let id = self.next_id;
        self.next_id += 1;
        let mut request = json!({"jsonrpc":"2.0", "id":id, "method":method});
        if let Some(params) = params {
            request["params"] = params;
        }
        let work = async {
            match &mut self.wire {
                Wire::Stdio { input, output } => {
                    write_line(input, &request).await?;
                    loop {
                        let mut bytes = Vec::new();
                        let n = (&mut *output)
                            .take(MAX_RESPONSE as u64 + 1)
                            .read_until(b'\n', &mut bytes)
                            .await
                            .map_err(|_| Failure::new("cannot read MCP response"))?;
                        if n == 0 {
                            return Err(Failure::new(
                                "MCP server closed stdout; check credentials and exclusive OAuth ownership",
                            ));
                        }
                        if n > MAX_RESPONSE {
                            return Err(Failure::new("MCP response exceeds test byte limit"));
                        }
                        let value: Value = serde_json::from_slice(&bytes)
                            .map_err(|_| Failure::new("MCP returned invalid JSON"))?;
                        if value.get("id").is_some() {
                            return rpc_result(value, id);
                        }
                    }
                }
                Wire::Http {
                    client,
                    url,
                    session,
                } => {
                    let response = http_post(client, url, session.as_deref(), &request)
                        .send()
                        .await
                        .map_err(|_| Failure::new("local MCP HTTP request failed"))?;
                    if !response.status().is_success() {
                        return Err(Failure::new("local MCP HTTP request rejected"));
                    }
                    if session.is_none() {
                        *session = response
                            .headers()
                            .get("mcp-session-id")
                            .and_then(|h| h.to_str().ok())
                            .map(str::to_owned);
                    }
                    let value = http_json(response).await?;
                    rpc_result(value, id)
                }
            }
        };
        tokio::time::timeout(self.timeout + Duration::from_secs(10), work)
            .await
            .map_err(|_| Failure::new("MCP call exceeded live-test deadline"))?
    }

    pub async fn call(&mut self, tool: &str, arguments: Value) -> Result<Value> {
        self.rpc(
            "tools/call",
            Some(json!({"name":tool,"arguments":arguments})),
        )
        .await
    }

    pub async fn shutdown(mut self) -> Result<()> {
        match &mut self.wire {
            Wire::Stdio { input, .. } => {
                input.take();
            }
            Wire::Http {
                client,
                url,
                session,
            } => {
                if let Some(session) = session.take() {
                    let _ = client
                        .delete(&*url)
                        .header("mcp-session-id", session)
                        .send()
                        .await;
                }
                #[cfg(unix)]
                if let Some(pid) = self.child.id() {
                    let status = Command::new("/bin/kill")
                        .args(["-TERM", &pid.to_string()])
                        .stdout(Stdio::null())
                        .stderr(Stdio::null())
                        .status()
                        .await
                        .map_err(|_| Failure::new("cannot signal MCP shutdown"))?;
                    if !status.success() {
                        return Err(Failure::new("MCP shutdown signal rejected"));
                    }
                }
                #[cfg(not(unix))]
                self.child
                    .start_kill()
                    .map_err(|_| Failure::new("cannot terminate MCP HTTP process"))?;
            }
        }
        let status = tokio::time::timeout(Duration::from_secs(15), self.child.wait())
            .await
            .map_err(|_| Failure::new("MCP shutdown timed out"))?
            .map_err(|_| Failure::new("cannot wait for MCP shutdown"))?;
        if !status.success() && cfg!(unix) {
            return Err(Failure::new("MCP server exited unsuccessfully"));
        }
        Ok(())
    }

    pub fn pid(&self) -> Option<u32> {
        self.child.id()
    }
}

fn http_post(
    client: &reqwest::Client,
    url: &str,
    session: Option<&str>,
    request: &Value,
) -> reqwest::RequestBuilder {
    let mut post = client
        .post(url)
        .header("accept", "application/json, text/event-stream")
        .header("mcp-protocol-version", "2025-11-25")
        .json(request);
    if let Some(session) = session {
        post = post.header("mcp-session-id", session);
    }
    post
}

async fn write_line(input: &mut Option<ChildStdin>, value: &Value) -> Result<()> {
    let mut bytes =
        serde_json::to_vec(value).map_err(|_| Failure::new("cannot encode MCP request"))?;
    bytes.push(b'\n');
    input
        .as_mut()
        .ok_or_else(|| Failure::new("MCP stdin unavailable"))?
        .write_all(&bytes)
        .await
        .map_err(|_| Failure::new("cannot write MCP request"))
}

async fn http_json(mut response: reqwest::Response) -> Result<Value> {
    let mut bytes = Vec::new();
    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|_| Failure::new("cannot read MCP HTTP response"))?
    {
        if chunk.len() > MAX_RESPONSE.saturating_sub(bytes.len()) {
            return Err(Failure::new("MCP response exceeds test byte limit"));
        }
        bytes.extend_from_slice(&chunk);
    }
    if let Ok(value) = serde_json::from_slice(&bytes) {
        return Ok(value);
    }
    let text =
        std::str::from_utf8(&bytes).map_err(|_| Failure::new("MCP returned invalid UTF-8"))?;
    for line in text.lines().filter_map(|line| line.strip_prefix("data:")) {
        if let Ok(value) = serde_json::from_str(line.trim()) {
            return Ok(value);
        }
    }
    Err(Failure::new(
        "MCP HTTP response contains no JSON-RPC result",
    ))
}

fn rpc_result(value: Value, id: u64) -> Result<Value> {
    if value["jsonrpc"] != "2.0" || value["id"] != id {
        return Err(Failure::new("MCP response ID/protocol mismatch"));
    }
    if value.get("error").is_some() {
        return Err(Failure::new("MCP JSON-RPC request failed"));
    }
    value
        .get("result")
        .filter(|r| r.is_object())
        .cloned()
        .ok_or_else(|| Failure::new("MCP response lacks an object result"))
}

pub fn structured(result: &Value) -> Result<&Value> {
    if result["isError"] == true {
        let error = &result["structuredContent"]["error"];
        return Err(Failure(format!(
            "tool failed: code={} status={} request_id={}",
            safe_id(&error["code"]),
            error["status"]
                .as_u64()
                .map_or_else(|| "none".into(), |s| s.to_string()),
            safe_id(&error["request_id"])
        )));
    }
    let value = result
        .get("structuredContent")
        .filter(|v| v.is_object())
        .ok_or_else(|| Failure::new("tool result lacks structuredContent"))?;
    let text = result["content"]
        .as_array()
        .and_then(|parts| parts.first())
        .and_then(|p| p.get("text"))
        .and_then(Value::as_str)
        .ok_or_else(|| Failure::new("tool result lacks text JSON"))?;
    if serde_json::from_str::<Value>(text).ok().as_ref() != Some(value) {
        return Err(Failure::new("tool text and structured result differ"));
    }
    Ok(value)
}

fn safe_id(value: &Value) -> String {
    match value.as_str() {
        Some(s)
            if !s.is_empty()
                && s.len() <= 128
                && s.bytes()
                    .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-' | b'.')) =>
        {
            s.to_owned()
        }
        Some(_) => "invalid".into(),
        None => "none".into(),
    }
}
