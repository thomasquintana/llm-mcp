//! OpenAI Sign in with ChatGPT lifecycle and rotating credential ownership.

mod store;
#[cfg(windows)]
mod windows;

use crate::{
    error::{Error, Result},
    provider::read_bounded,
};
use axum::{
    Router,
    extract::{Query, State},
    routing::get,
};
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use jsonwebtoken::{Algorithm, DecodingKey, Validation, decode, decode_header, jwk::JwkSet};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    path::Path,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::{SystemTime, UNIX_EPOCH},
};
use store::{Credentials, PendingRotation, Store};
use tokio::{
    net::TcpListener,
    sync::{Mutex, oneshot, watch},
};
use tokio_util::task::TaskTracker;
use url::Url;

const ISSUER: &str = "https://auth.openai.com";
const RESOURCE: &str = "https://api.openai.com/v1";
const SCOPES: &str =
    "openid profile email offline_access resource.invoke chatgpt.tokens.use.direct";

fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock after epoch")
        .as_secs()
}
fn random_secret() -> String {
    URL_SAFE_NO_PAD.encode(rand::random::<[u8; 32]>())
}
fn auth_error(message: &str) -> Error {
    Error::new("authentication", message)
}

#[derive(Clone)]
struct Endpoints {
    token: String,
    jwks: String,
}
impl Default for Endpoints {
    fn default() -> Self {
        Self {
            token: format!("{ISSUER}/api/accounts/oauth/token"),
            jwks: format!("{ISSUER}/.well-known/jwks.json"),
        }
    }
}

#[derive(Deserialize)]
struct TokenResponse {
    access_token: String,
    refresh_token: Option<String>,
    id_token: Option<String>,
    expires_in: u64,
    scope: Option<String>,
    token_type: String,
}

#[derive(Debug, Clone, Deserialize)]
struct Claims {
    iss: String,
    sub: String,
    aud: serde_json::Value,
    azp: Option<String>,
    exp: u64,
    nonce: Option<String>,
    email: Option<String>,
}

async fn validate_id_token(
    client: &Client,
    endpoints: &Endpoints,
    token: &str,
    client_id: &str,
    nonce: Option<&str>,
) -> Result<Claims> {
    validate_id_token_at(client, endpoints, token, client_id, nonce, None).await
}

async fn validate_id_token_at(
    client: &Client,
    endpoints: &Endpoints,
    token: &str,
    client_id: &str,
    nonce: Option<&str>,
    received_at: Option<u64>,
) -> Result<Claims> {
    let header = decode_header(token).map_err(|_| auth_error("invalid ID token header"))?;
    if header.alg != Algorithm::RS256 {
        return Err(auth_error("unsupported ID token signature algorithm"));
    }
    let response = client
        .get(&endpoints.jwks)
        .timeout(std::time::Duration::from_secs(30))
        .send()
        .await
        .map_err(Error::transport)?;
    if !response.status().is_success() {
        return Err(Error::http(&response, true));
    }
    let keys: JwkSet = serde_json::from_slice(&read_bounded(response, 256 * 1024).await?)
        .map_err(|_| auth_error("invalid OpenAI signing keys"))?;
    let jwk = keys
        .find(
            header
                .kid
                .as_deref()
                .ok_or_else(|| auth_error("ID token has no signing key identifier"))?,
        )
        .ok_or_else(|| auth_error("unknown ID token signing key"))?;
    let key = DecodingKey::from_jwk(jwk).map_err(|_| auth_error("invalid ID token signing key"))?;
    let mut validation = Validation::new(Algorithm::RS256);
    validation.set_issuer(&[ISSUER]);
    validation.set_audience(&[client_id]);
    validation.set_required_spec_claims(&["exp", "iss", "aud", "sub"]);
    validation.leeway = 30;
    validation.validate_exp = received_at.is_none();
    let claims = decode::<Claims>(token, &key, &validation)
        .map_err(|_| {
            auth_error("ID token signature, issuer, audience or expiry validation failed")
        })?
        .claims;
    if claims.iss != ISSUER
        || claims.sub.is_empty()
        || received_at.is_some_and(|received| claims.exp <= received)
        || nonce.is_some_and(|n| claims.nonce.as_deref() != Some(n))
    {
        return Err(auth_error("ID token identity or nonce mismatch"));
    }
    if (claims.azp.is_some() || claims.aud.as_array().is_some_and(|a| a.len() > 1))
        && claims.azp.as_deref() != Some(client_id)
    {
        return Err(auth_error("ID token authorized party mismatch"));
    }
    Ok(claims)
}

async fn token_request(
    client: &Client,
    endpoints: &Endpoints,
    fields: &[(&str, &str)],
) -> Result<TokenResponse> {
    let response = client
        .post(&endpoints.token)
        .timeout(std::time::Duration::from_secs(30))
        .form(fields)
        .send()
        .await
        .map_err(Error::transport)?;
    if !response.status().is_success() {
        let status = response.status();
        let mut error = Error::new(
            if status.as_u16() == 429 {
                "rate_limited"
            } else if status.is_server_error() {
                "provider"
            } else {
                "authentication"
            },
            "OpenAI token exchange failed",
        );
        error.status = Some(status.as_u16());
        error.request_id = response
            .headers()
            .get("x-request-id")
            .and_then(|h| h.to_str().ok())
            .map(str::to_owned);
        error.retryable = status.as_u16() == 429 || status.is_server_error();
        return Err(error);
    }
    let token: TokenResponse = serde_json::from_slice(&read_bounded(response, 256 * 1024).await?)
        .map_err(|_| auth_error("invalid OAuth token response"))?;
    if !token.token_type.eq_ignore_ascii_case("bearer")
        || token.access_token.is_empty()
        || token.expires_in == 0
    {
        return Err(auth_error("invalid OAuth token type or lifetime"));
    }
    Ok(token)
}

type RefreshReceiver = watch::Receiver<Option<Result<String>>>;

struct SessionState {
    credentials: Option<Credentials>,
    pending: Option<PendingRotation>,
    in_flight: Option<RefreshReceiver>,
    failure: Option<(Error, Option<tokio::time::Instant>)>,
}

/// Exclusive credential owner with shared refresh outcomes and durable recovery.
pub struct OAuthSession {
    store: Store,
    state: Mutex<SessionState>,
    client: Client,
    endpoints: Endpoints,
    tasks: TaskTracker,
    stopping: AtomicBool,
}

impl OAuthSession {
    /// Open protected OAuth state and acquire its process-lifetime lease.
    pub fn open(path: &Path) -> Result<Self> {
        let store = Store::open(path)?;
        let state = Mutex::new(SessionState {
            credentials: store.load()?,
            pending: store.pending()?,
            in_flight: None,
            failure: None,
        });
        Ok(Self {
            store,
            state,
            client: auth_client()?,
            endpoints: Endpoints::default(),
            tasks: TaskTracker::new(),
            stopping: AtomicBool::new(false),
        })
    }

    /// Obtain an access token, refreshing and durably rotating credentials if needed.
    pub async fn access_token(self: &Arc<Self>) -> Result<String> {
        let mut state = self.state.lock().await;
        if self.stopping.load(Ordering::Acquire) {
            return Err(Error::new("shutdown", "OAuth owner is shutting down"));
        }
        let mut receiver = if let Some(receiver) = &state.in_flight {
            receiver.clone()
        } else {
            if let Some((error, until)) = &state.failure
                && until.is_none_or(|until| tokio::time::Instant::now() < until)
            {
                return Err(error.clone());
            }
            if state.pending.is_none() {
                let current = state
                    .credentials
                    .as_ref()
                    .ok_or_else(|| auth_error("run llm-mcp login or auth import first"))?;
                if current.expires_at > now().saturating_add(60) {
                    return Ok(current.access_token.clone());
                }
            }
            let (sender, receiver) = watch::channel(None);
            state.in_flight = Some(receiver.clone());
            let session = self.clone();
            self.tasks.spawn(async move {
                let result = session.refresh_or_recover().await;
                let mut state = session.state.lock().await;
                if let Err(error) = &result {
                    let retry_at = error
                        .retryable
                        .then(|| tokio::time::Instant::now() + std::time::Duration::from_secs(30));
                    state.failure = Some((error.clone(), retry_at));
                    if retry_at.is_none() {
                        state.credentials = None;
                    }
                } else {
                    state.failure = None;
                }
                let _ = sender.send(Some(result));
                state.in_flight = None;
            });
            receiver
        };
        drop(state);
        loop {
            if let Some(result) = receiver.borrow().clone() {
                return result;
            }
            receiver
                .changed()
                .await
                .map_err(|_| auth_error("OAuth refresh task failed"))?;
        }
    }

    async fn refresh_or_recover(&self) -> Result<String> {
        let pending = self.state.lock().await.pending.clone();
        if let Some(pending) = pending {
            let access_token = self.finish_pending(pending).await?;
            if self
                .state
                .lock()
                .await
                .credentials
                .as_ref()
                .is_some_and(|c| c.expires_at > now().saturating_add(60))
            {
                return Ok(access_token);
            }
        }
        let current = self
            .state
            .lock()
            .await
            .credentials
            .clone()
            .ok_or_else(|| auth_error("OAuth reauthorization required"))?;
        let token = token_request(
            &self.client,
            &self.endpoints,
            &[
                ("grant_type", "refresh_token"),
                ("client_id", &current.client_id),
                ("refresh_token", &current.refresh_token),
                ("resource", RESOURCE),
            ],
        )
        .await
        .map_err(|error| {
            if error.status.is_none() {
                auth_error("ambiguous OAuth token exchange; reauthorization required")
            } else {
                error
            }
        })?;
        let mut updated = current.clone();
        let verify_id_token = token.id_token.is_some();
        if let Some(id_token) = token.id_token {
            updated.id_token = id_token;
        }
        updated.access_token = token.access_token;
        if let Some(refresh_token) = token.refresh_token {
            updated.refresh_token = refresh_token;
        }
        if let Some(scope) = token.scope {
            updated.scopes = scope.split_whitespace().map(str::to_owned).collect();
        }
        updated.expires_at = now()
            .checked_add(token.expires_in)
            .ok_or_else(|| auth_error("invalid token expiry"))?;
        let pending = PendingRotation {
            credentials: updated,
            verify_id_token,
            received_at: now(),
        };
        self.state.lock().await.pending = Some(pending.clone());
        self.finish_pending(pending).await
    }

    async fn finish_pending(&self, pending: PendingRotation) -> Result<String> {
        let persistence = |mut error: Error| {
            error.retryable = true;
            error
        };
        // Keep a durable replacement token BEFORE any fallible network validation.
        self.store.stage(&pending).map_err(persistence)?;
        let updated = &pending.credentials;
        if updated.ext_agent_host_id != self.store.host_id {
            return Err(auth_error("pending OAuth host mismatch"));
        }
        updated.validate()?;
        if pending.verify_id_token {
            let claims = validate_id_token_at(
                &self.client,
                &self.endpoints,
                &updated.id_token,
                &updated.client_id,
                None,
                Some(pending.received_at),
            )
            .await?;
            if claims.sub != updated.subject {
                return Err(auth_error("refreshed account identity changed"));
            }
        }
        self.store.save(updated).map_err(persistence)?;
        self.store.clear_pending().map_err(persistence)?;
        let mut state = self.state.lock().await;
        state.credentials = Some(updated.clone());
        state.pending = None;
        Ok(updated.access_token.clone())
    }

    /// Stop starting refreshes, while allowing an already-started rotation to finish.
    pub fn stop(&self) {
        self.stopping.store(true, Ordering::Release);
    }

    /// Join refresh work before destroying the runtime or releasing credential ownership.
    pub async fn shutdown(&self) {
        self.stop();
        let state = self.state.lock().await;
        self.tasks.close();
        drop(state);
        self.tasks.wait().await;
    }
}

fn auth_client() -> Result<Client> {
    Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .retry(reqwest::retry::never())
        .timeout(std::time::Duration::from_secs(30))
        .build()
        .map_err(Error::transport)
}

#[derive(Clone, Deserialize)]
struct Callback {
    state: String,
    code: Option<String>,
    client_id: Option<String>,
    error: Option<String>,
}
type CallbackSender = Arc<Mutex<Option<oneshot::Sender<Callback>>>>;

async fn callback(
    State(state): State<(String, CallbackSender)>,
    Query(query): Query<Callback>,
) -> (axum::http::StatusCode, &'static str) {
    if query.state != state.0 {
        return (
            axum::http::StatusCode::BAD_REQUEST,
            "Invalid authorization state.",
        );
    }
    if let Some(tx) = state.1.lock().await.take() {
        let _ = tx.send(query);
    }
    (
        axum::http::StatusCode::OK,
        "Authorization received. Return to llm-mcp.",
    )
}

/// Complete browser-based OpenAI login. The loopback listener is bounded to five minutes.
pub async fn login(path: &Path) -> Result<()> {
    let client = auth_client()?;
    let store = Store::open(path)?;
    let previous = store.load()?;
    let state = random_secret();
    let nonce = random_secret();
    let verifier = random_secret();
    let challenge = URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()));
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .map_err(|_| auth_error("cannot start OAuth loopback listener"))?;
    let redirect = format!(
        "http://127.0.0.1:{}/auth/callback",
        listener
            .local_addr()
            .map_err(|_| auth_error("cannot obtain loopback port"))?
            .port()
    );
    let (tx, rx) = oneshot::channel();
    let app = Router::new()
        .route("/auth/callback", get(callback))
        .with_state((state.clone(), Arc::new(Mutex::new(Some(tx)))));
    let server = tokio::spawn(async move { axum::serve(listener, app).await });
    let mut url = Url::parse(&format!("{ISSUER}/api/accounts/authorize")).expect("static URL");
    {
        let mut query = url.query_pairs_mut();
        query.extend_pairs([
            ("response_type", "code"),
            ("redirect_uri", &redirect),
            ("scope", SCOPES),
            ("resource", RESOURCE),
            ("state", &state),
            ("nonce", &nonce),
            ("code_challenge_method", "S256"),
            ("code_challenge", &challenge),
            ("ext_agent_host_id", &store.host_id),
        ]);
        if let Some(credentials) = &previous {
            query
                .append_pair("client_id", &credentials.client_id)
                .append_pair("id_token_hint", &credentials.id_token);
        } else {
            query
                .append_pair("client_id", "dynamic_agent_client")
                .append_pair("agent_name_hint", "llm-mcp");
        }
    }
    let fallback = fallback_authorization_url(&url);
    eprintln!("Continue with ChatGPT:\n{fallback}");
    open_browser(url.as_str());
    let result = tokio::time::timeout(std::time::Duration::from_secs(300), rx).await;
    server.abort();
    let callback = result
        .map_err(|_| auth_error("OAuth login timed out"))?
        .map_err(|_| auth_error("OAuth callback closed"))?;
    if callback.state != state || callback.error.is_some() {
        return Err(auth_error("OpenAI authorization was denied or invalid"));
    }
    let client_id = match (&previous, callback.client_id) {
        (Some(old), Some(id)) if id != old.client_id => {
            return Err(auth_error("OAuth client registration changed"));
        }
        (Some(old), _) => old.client_id.clone(),
        (None, Some(id)) if !id.is_empty() && id != "dynamic_agent_client" => id,
        _ => {
            return Err(auth_error(
                "OAuth registration did not return an issued client ID",
            ));
        }
    };
    let code = callback
        .code
        .ok_or_else(|| auth_error("OAuth callback omitted authorization code"))?;
    let endpoints = Endpoints::default();
    let token = token_request(
        &client,
        &endpoints,
        &[
            ("grant_type", "authorization_code"),
            ("client_id", &client_id),
            ("code", &code),
            ("code_verifier", &verifier),
            ("redirect_uri", &redirect),
            ("resource", RESOURCE),
        ],
    )
    .await?;
    let id_token = token
        .id_token
        .ok_or_else(|| auth_error("OpenAI omitted ID token"))?;
    let claims =
        validate_id_token(&client, &endpoints, &id_token, &client_id, Some(&nonce)).await?;
    if previous
        .as_ref()
        .is_some_and(|old| old.subject != claims.sub)
    {
        return Err(auth_error(
            "signed-in identity differs from the registered account; logout first",
        ));
    }
    let credentials = Credentials {
        issuer: ISSUER.into(),
        subject: claims.sub,
        email: claims.email,
        client_id,
        ext_agent_host_id: store.host_id.clone(),
        id_token,
        access_token: token.access_token,
        refresh_token: token
            .refresh_token
            .ok_or_else(|| auth_error("OpenAI omitted refresh token"))?,
        scopes: token
            .scope
            .unwrap_or_default()
            .split_whitespace()
            .map(str::to_owned)
            .collect(),
        expires_at: now()
            .checked_add(token.expires_in)
            .ok_or_else(|| auth_error("invalid token expiry"))?,
    };
    store.save(&credentials)?;
    store.clear_pending()
}

fn fallback_authorization_url(url: &Url) -> Url {
    let mut fallback = url.clone();
    let pairs: Vec<(String, String)> = url
        .query_pairs()
        .filter(|(key, _)| key != "id_token_hint")
        .map(|(key, value)| (key.into_owned(), value.into_owned()))
        .collect();
    fallback.set_query(None);
    fallback.query_pairs_mut().extend_pairs(pairs);
    fallback
}

fn open_browser(url: &str) {
    #[cfg(target_os = "macos")]
    let _ = std::process::Command::new("open")
        .arg(url)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn();
    #[cfg(target_os = "windows")]
    let _ = std::process::Command::new("rundll32")
        .args(["url.dll,FileProtocolHandler", url])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn();
    #[cfg(all(unix, not(target_os = "macos")))]
    let _ = std::process::Command::new("xdg-open")
        .arg(url)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn();
}

/// Credential status containing no tokens.
#[derive(Debug, Serialize)]
pub struct AuthStatus {
    /// Whether a local OAuth registration is present.
    pub signed_in: bool,
    /// Stable identity of this installation.
    pub host_id: String,
    /// Access-token expiry, if signed in. Status does not refresh tokens.
    pub expires_at: Option<u64>,
    /// Whether a rotation must finish validation/persistence before inference.
    pub rotation_pending: bool,
}

/// Inspect protected state without revealing credentials.
pub fn status(path: &Path) -> Result<AuthStatus> {
    let store = Store::open(path)?;
    let credentials = store.load()?;
    let pending = store.pending()?;
    Ok(AuthStatus {
        signed_in: credentials.is_some() || pending.is_some(),
        host_id: store.host_id.clone(),
        expires_at: credentials.map(|c| c.expires_at),
        rotation_pending: pending.is_some(),
    })
}

/// Remove local credentials while retaining stable host identity.
pub fn logout(path: &Path) -> Result<()> {
    Store::open(path)?.clear()
}

/// Import a trusted transferred credential record, preserving destination host identity.
pub fn import(path: &Path, source: &Path) -> Result<()> {
    Store::open(path)?.import(source)
}

#[cfg(test)]
mod tests;
