//! OpenAI Sign in with ChatGPT lifecycle and rotating credential ownership.

mod store;

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
    sync::Arc,
    time::{SystemTime, UNIX_EPOCH},
};
use store::{Credentials, Store};
use tokio::{
    net::TcpListener,
    sync::{Mutex, oneshot},
};
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
        return Err(auth_error("cannot obtain OpenAI signing keys"));
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
    let claims = decode::<Claims>(token, &key, &validation)
        .map_err(|_| {
            auth_error("ID token signature, issuer, audience or expiry validation failed")
        })?
        .claims;
    if claims.iss != ISSUER
        || claims.sub.is_empty()
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
        return Err(auth_error(
            "OpenAI token exchange failed; sign in again if authorization was revoked",
        ));
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

/// Exclusive credential owner. A mutex coordinates token refresh across calls.
pub struct OAuthSession {
    store: Store,
    credentials: Mutex<Option<Credentials>>,
    client: Client,
    endpoints: Endpoints,
}

impl OAuthSession {
    /// Open protected OAuth state and acquire its process-lifetime lease.
    pub fn open(path: &Path, client: Client) -> Result<Self> {
        let store = Store::open(path)?;
        let credentials = Mutex::new(store.load()?);
        Ok(Self {
            store,
            credentials,
            client,
            endpoints: Endpoints::default(),
        })
    }

    /// Obtain an access token, refreshing and durably rotating credentials if needed.
    pub async fn access_token(self: &Arc<Self>) -> Result<String> {
        // A cancelled model call must not interrupt a rotating refresh after OpenAI
        // has consumed the old refresh token. The bounded task persists independently.
        let session = self.clone();
        tokio::spawn(async move { session.refresh_or_read().await })
            .await
            .map_err(|_| auth_error("OAuth refresh task failed"))?
    }

    async fn refresh_or_read(&self) -> Result<String> {
        let mut guard = self.credentials.lock().await;
        let current = guard
            .as_ref()
            .ok_or_else(|| auth_error("run llm-mcp login or auth import first"))?;
        if current.expires_at > now().saturating_add(60) {
            return Ok(current.access_token.clone());
        }
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
        .await?;
        let mut updated = current.clone();
        if let Some(id_token) = token.id_token {
            let claims = validate_id_token(
                &self.client,
                &self.endpoints,
                &id_token,
                &current.client_id,
                None,
            )
            .await?;
            if claims.sub != current.subject {
                return Err(auth_error("refreshed account identity changed"));
            }
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
        // Publish to memory only after durable persistence succeeds.
        self.store.save(&updated)?;
        let result = updated.access_token.clone();
        *guard = Some(updated);
        Ok(result)
    }
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
    let client = Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .timeout(std::time::Duration::from_secs(30))
        .build()
        .map_err(Error::transport)?;
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
    eprintln!("Continue with ChatGPT:\n{url}");
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
        (None, Some(id)) if id.starts_with("oaiapp_") => id,
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
    store.save(&credentials)
}

fn open_browser(url: &str) {
    #[cfg(target_os = "macos")]
    let _ = std::process::Command::new("open").arg(url).spawn();
    #[cfg(target_os = "windows")]
    let _ = std::process::Command::new("rundll32")
        .args(["url.dll,FileProtocolHandler", url])
        .spawn();
    #[cfg(all(unix, not(target_os = "macos")))]
    let _ = std::process::Command::new("xdg-open").arg(url).spawn();
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
}

/// Inspect protected state without revealing credentials.
pub fn status(path: &Path) -> Result<AuthStatus> {
    let store = Store::open(path)?;
    let credentials = store.load()?;
    Ok(AuthStatus {
        signed_in: credentials.is_some(),
        host_id: store.host_id.clone(),
        expires_at: credentials.map(|c| c.expires_at),
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
