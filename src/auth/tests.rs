use super::*;
use axum::{Json, routing::post};
use serde_json::{Value, json};
use std::sync::atomic::{AtomicUsize, Ordering};

fn credentials(host: &str) -> Credentials {
    Credentials {
        issuer: ISSUER.into(),
        subject: "user-123".into(),
        email: None,
        client_id: "oaiapp_test".into(),
        ext_agent_host_id: host.into(),
        id_token: "retained-id".into(),
        access_token: "old-access".into(),
        refresh_token: "old-refresh".into(),
        scopes: SCOPES.split_whitespace().map(str::to_owned).collect(),
        expires_at: 0,
    }
}

async fn token_mock(
    status: axum::http::StatusCode,
    body: Value,
    delay: std::time::Duration,
) -> (Endpoints, Arc<AtomicUsize>, tokio::task::JoinHandle<()>) {
    let count = Arc::new(AtomicUsize::new(0));
    let counter = count.clone();
    let app = Router::new().route(
        "/token",
        post(move || {
            let counter = counter.clone();
            let body = body.clone();
            async move {
                counter.fetch_add(1, Ordering::SeqCst);
                tokio::time::sleep(delay).await;
                (status, [("x-request-id", "req-token")], Json(body))
            }
        }),
    );
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let token = format!("http://{}/token", listener.local_addr().unwrap());
    let task = tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    (
        Endpoints {
            token,
            ..Endpoints::default()
        },
        count,
        task,
    )
}

fn saved_session(path: &Path) -> OAuthSession {
    let store = Store::open(path).unwrap();
    store.save(&credentials(&store.host_id)).unwrap();
    drop(store);
    OAuthSession::open(path).unwrap()
}

#[test]
fn returning_login_fallback_omits_id_token_hint_but_preserves_pkce_and_state() {
    let url=Url::parse("https://auth.openai.com/api/accounts/authorize?client_id=oaiapp_test&state=bound&code_challenge=pkce&id_token_hint=secret-id-token").unwrap();
    let fallback = fallback_authorization_url(&url);
    assert!(!fallback.as_str().contains("secret-id-token"));
    let parameters: std::collections::HashMap<_, _> = fallback.query_pairs().collect();
    assert_eq!(parameters["state"], "bound");
    assert_eq!(parameters["code_challenge"], "pkce");
    assert_eq!(parameters["client_id"], "oaiapp_test");
}

#[tokio::test]
async fn refresh_failures_are_shared_and_abandoned_waiters_do_not_retry() {
    for status in [
        axum::http::StatusCode::TOO_MANY_REQUESTS,
        axum::http::StatusCode::UNAUTHORIZED,
    ] {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("auth.json");
        let mut session = saved_session(&path);
        let (endpoints, count, task) = token_mock(
            status,
            json!({"error":"invalid_grant"}),
            std::time::Duration::from_millis(30),
        )
        .await;
        session.endpoints = endpoints;
        let session = Arc::new(session);
        let mut calls = Vec::new();
        for _ in 0..20 {
            let s = session.clone();
            calls.push(tokio::spawn(
                async move { s.access_token().await.unwrap_err() },
            ));
        }
        for call in calls {
            let error = call.await.unwrap();
            assert_eq!(error.status, Some(status.as_u16()));
            assert_eq!(error.request_id.as_deref(), Some("req-token"));
            assert_eq!(error.retryable, status.as_u16() == 429);
        }
        assert!(session.access_token().await.is_err());
        assert_eq!(count.load(Ordering::SeqCst), 1);
        session.shutdown().await;
        task.abort();
    }
}

#[tokio::test]
async fn pending_rotation_recovers_after_persistence_failure_without_exchanging_old_token() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("auth.json");
    let mut session = saved_session(&path);
    let (endpoints,count,task)=token_mock(axum::http::StatusCode::OK,json!({"access_token":"new-access","refresh_token":"new-refresh","token_type":"Bearer","expires_in":3600}),std::time::Duration::ZERO).await;
    session.endpoints = endpoints.clone();
    std::fs::remove_file(&path).unwrap();
    std::fs::create_dir(&path).unwrap();
    let session = Arc::new(session);
    let error = session.access_token().await.unwrap_err();
    assert_eq!(error.code, "credential_store");
    assert!(error.retryable);
    assert_eq!(
        session
            .store
            .pending()
            .unwrap()
            .unwrap()
            .credentials
            .refresh_token,
        "new-refresh"
    );
    session.shutdown().await;
    drop(session);
    std::fs::remove_dir(&path).unwrap();
    let mut reopened = OAuthSession::open(&path).unwrap();
    reopened.endpoints = endpoints;
    let reopened = Arc::new(reopened);
    assert_eq!(reopened.access_token().await.unwrap(), "new-access");
    assert_eq!(count.load(Ordering::SeqCst), 1);
    assert!(reopened.store.pending().unwrap().is_none());
    assert_eq!(
        reopened.store.load().unwrap().unwrap().refresh_token,
        "new-refresh"
    );
    reopened.shutdown().await;
    task.abort();
}

#[tokio::test]
async fn shutdown_joins_rotation_after_the_original_caller_is_cancelled() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("auth.json");
    let mut session = saved_session(&path);
    let (endpoints,count,task)=token_mock(axum::http::StatusCode::OK,json!({"access_token":"new-access","refresh_token":"new-refresh","token_type":"Bearer","expires_in":3600}),std::time::Duration::from_millis(50)).await;
    session.endpoints = endpoints;
    let session = Arc::new(session);
    let caller = session.clone();
    let caller = tokio::spawn(async move { caller.access_token().await });
    while count.load(Ordering::SeqCst) == 0 {
        tokio::task::yield_now().await;
    }
    caller.abort();
    session.shutdown().await;
    assert_eq!(
        session.store.load().unwrap().unwrap().refresh_token,
        "new-refresh"
    );
    assert_eq!(session.access_token().await.unwrap_err().code, "shutdown");
    task.abort();
}

#[tokio::test]
async fn jwt_validation_outage_keeps_rotated_credentials_durable_for_restart() {
    use aws_lc_rs::{
        rsa::{KeyPair, KeySize},
        signature::KeyPair as _,
    };
    let key = KeyPair::generate(KeySize::Rsa2048).unwrap();
    let keys = json!({"keys":[{"kty":"RSA","kid":"test","alg":"RS256","use":"sig","n":URL_SAFE_NO_PAD.encode(key.public_key().modulus().big_endian_without_leading_zero()),"e":URL_SAFE_NO_PAD.encode(key.public_key().exponent().big_endian_without_leading_zero())}]});
    let message = format!(
        "{}.{}",
        URL_SAFE_NO_PAD.encode(br#"{"alg":"RS256","kid":"test"}"#),
        URL_SAFE_NO_PAD.encode(
            serde_json::to_vec(
                &json!({"iss":ISSUER,"aud":"oaiapp_test","sub":"user-123","exp":now()+3600})
            )
            .unwrap()
        )
    );
    let mut signature = vec![0; key.public_modulus_len()];
    key.sign(
        &aws_lc_rs::signature::RSA_PKCS1_SHA256,
        &aws_lc_rs::rand::SystemRandom::new(),
        message.as_bytes(),
        &mut signature,
    )
    .unwrap();
    let id_token = format!("{message}.{}", URL_SAFE_NO_PAD.encode(signature));
    let ready = Arc::new(AtomicBool::new(false));
    let available = ready.clone();
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let jwks = format!("http://{}/keys", listener.local_addr().unwrap());
    let server = tokio::spawn(async move {
        axum::serve(
            listener,
            Router::new().route(
                "/keys",
                get(move || {
                    let keys = keys.clone();
                    let available = available.clone();
                    async move {
                        if available.load(Ordering::SeqCst) {
                            (axum::http::StatusCode::OK, Json(keys))
                        } else {
                            (axum::http::StatusCode::SERVICE_UNAVAILABLE, Json(json!({})))
                        }
                    }
                }),
            ),
        )
        .await
        .unwrap();
    });
    let (mut endpoints,count,task)=token_mock(axum::http::StatusCode::OK,json!({"access_token":"rotated","refresh_token":"rotated-refresh","id_token":id_token,"expires_in":3600,"token_type":"Bearer"}),std::time::Duration::ZERO).await;
    endpoints.jwks = jwks;
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("auth.json");
    let mut session = saved_session(&path);
    session.endpoints = endpoints.clone();
    let session = Arc::new(session);
    let error = session.access_token().await.unwrap_err();
    assert_eq!(error.status, Some(503));
    assert!(error.retryable);
    assert_eq!(
        session
            .store
            .pending()
            .unwrap()
            .unwrap()
            .credentials
            .refresh_token,
        "rotated-refresh"
    );
    session.shutdown().await;
    drop(session);
    ready.store(true, Ordering::SeqCst);
    let mut session = OAuthSession::open(&path).unwrap();
    session.endpoints = endpoints;
    let session = Arc::new(session);
    assert_eq!(session.access_token().await.unwrap(), "rotated");
    assert_eq!(count.load(Ordering::SeqCst), 1);
    session.shutdown().await;
    task.abort();
    server.abort();
}

#[test]
fn store_exclusivity_import_permissions_and_host_survive_logout() {
    let temp = tempfile::tempdir().unwrap();
    let source_path = temp.path().join("source.json");
    let source = Store::open(&source_path).unwrap();
    source.save(&credentials(&source.host_id)).unwrap();
    assert!(Store::open(&source_path).is_err());
    let path = temp.path().join("destination.json");
    let destination = Store::open(&path).unwrap();
    assert_ne!(destination.host_id, source.host_id);
    destination.import(&source_path).unwrap();
    assert_eq!(
        destination.load().unwrap().unwrap().ext_agent_host_id,
        destination.host_id
    );
    let host = destination.host_id.clone();
    destination.clear().unwrap();
    assert!(destination.load().unwrap().is_none());
    drop(destination);
    assert_eq!(Store::open(&path).unwrap().host_id, host);
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            std::fs::metadata(&source_path)
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o600
        );
        std::fs::set_permissions(&source_path, std::fs::Permissions::from_mode(0o644)).unwrap();
        assert!(Store::read_path(&source_path).is_err());
    }
}

#[tokio::test]
async fn refresh_is_single_flight_durable_and_survives_caller_cancellation() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("auth.json");
    let store = Store::open(&path).unwrap();
    store.save(&credentials(&store.host_id)).unwrap();
    drop(store);
    let count = Arc::new(AtomicUsize::new(0));
    let counter = count.clone();
    let app = Router::new().route("/token", post(move |body: String| { let counter=counter.clone(); async move {
        assert!(body.contains("refresh_token=old-refresh"));
        counter.fetch_add(1, Ordering::SeqCst); tokio::time::sleep(std::time::Duration::from_millis(30)).await;
        Json(json!({"access_token":"new-access","refresh_token":"new-refresh","expires_in":3600,"token_type":"Bearer"}))
    }}));
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}/token", listener.local_addr().unwrap());
    let task = tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    let mut session = OAuthSession::open(&path).unwrap();
    session.endpoints.token = url;
    let session = Arc::new(session);
    let abandoned = session.clone();
    let call = tokio::spawn(async move { abandoned.access_token().await });
    tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    call.abort();
    let mut calls = Vec::new();
    for _ in 0..10 {
        let s = session.clone();
        calls.push(tokio::spawn(async move { s.access_token().await.unwrap() }));
    }
    for call in calls {
        assert_eq!(call.await.unwrap(), "new-access");
    }
    assert_eq!(count.load(Ordering::SeqCst), 1);
    drop(session);
    let reopened = Store::open(&path).unwrap();
    assert_eq!(
        reopened.load().unwrap().unwrap().refresh_token,
        "new-refresh"
    );
    task.abort();
}

#[tokio::test]
async fn callback_state_is_bound_and_rejected_attempt_does_not_consume_login() {
    let (tx, mut rx) = oneshot::channel();
    let state = ("expected".into(), Arc::new(Mutex::new(Some(tx))));
    let query = Callback {
        state: "wrong".into(),
        code: Some("code".into()),
        client_id: None,
        error: None,
    };
    assert_eq!(
        callback(State(state.clone()), Query(query)).await.0,
        axum::http::StatusCode::BAD_REQUEST
    );
    assert!(rx.try_recv().is_err());
    let query = Callback {
        state: "expected".into(),
        code: None,
        client_id: None,
        error: Some("access_denied".into()),
    };
    assert_eq!(
        callback(State(state), Query(query)).await.0,
        axum::http::StatusCode::OK
    );
    assert_eq!(rx.await.unwrap().error.as_deref(), Some("access_denied"));
}

#[tokio::test]
async fn validates_rsa_signature_issuer_audience_nonce_expiry_and_subject() {
    use aws_lc_rs::{
        rsa::{KeyPair, KeySize},
        signature::KeyPair as _,
    };
    let key = KeyPair::generate(KeySize::Rsa2048).unwrap();
    let jwks = json!({"keys":[{"kty":"RSA","kid":"test","alg":"RS256","use":"sig","n":URL_SAFE_NO_PAD.encode(key.public_key().modulus().big_endian_without_leading_zero()),"e":URL_SAFE_NO_PAD.encode(key.public_key().exponent().big_endian_without_leading_zero())}]});
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}/jwks", listener.local_addr().unwrap());
    let task = tokio::spawn(async move {
        axum::serve(
            listener,
            Router::new().route(
                "/jwks",
                get(move || {
                    let jwks = jwks.clone();
                    async { Json(jwks) }
                }),
            ),
        )
        .await
        .unwrap();
    });
    let endpoints = Endpoints {
        jwks: url,
        ..Endpoints::default()
    };
    let sign = |claims: &Value| {
        let header = URL_SAFE_NO_PAD.encode(br#"{"alg":"RS256","kid":"test"}"#);
        let payload = URL_SAFE_NO_PAD.encode(serde_json::to_vec(claims).unwrap());
        let message = format!("{header}.{payload}");
        let mut signature = vec![0; key.public_modulus_len()];
        key.sign(
            &aws_lc_rs::signature::RSA_PKCS1_SHA256,
            &aws_lc_rs::rand::SystemRandom::new(),
            message.as_bytes(),
            &mut signature,
        )
        .unwrap();
        format!("{message}.{}", URL_SAFE_NO_PAD.encode(signature))
    };
    let claims =
        json!({"iss":ISSUER,"aud":"oaiapp_test","exp":now()+3600,"sub":"user","nonce":"expected"});
    let token = sign(&claims);
    assert_eq!(
        validate_id_token(
            &Client::new(),
            &endpoints,
            &token,
            "oaiapp_test",
            Some("expected")
        )
        .await
        .unwrap()
        .sub,
        "user"
    );
    for (field, bad) in [
        ("iss", json!("https://other.example")),
        ("aud", json!("another-client")),
        ("exp", json!(1)),
        ("sub", json!("")),
        ("nonce", json!("wrong")),
    ] {
        let mut invalid = claims.clone();
        invalid[field] = bad;
        let token = sign(&invalid);
        assert!(
            validate_id_token(
                &Client::new(),
                &endpoints,
                &token,
                "oaiapp_test",
                Some("expected")
            )
            .await
            .is_err()
        );
    }
    let wrong = jsonwebtoken::encode(
        &jsonwebtoken::Header::new(Algorithm::HS256),
        &claims,
        &jsonwebtoken::EncodingKey::from_secret(b"wrong"),
    )
    .unwrap();
    assert!(
        validate_id_token(
            &Client::new(),
            &endpoints,
            &wrong,
            "oaiapp_test",
            Some("expected")
        )
        .await
        .is_err()
    );
    for field in ["iss", "aud", "exp", "sub", "nonce"] {
        let mut missing = claims.clone();
        missing.as_object_mut().unwrap().remove(field);
        assert!(
            validate_id_token(
                &Client::new(),
                &endpoints,
                &sign(&missing),
                "oaiapp_test",
                Some("expected")
            )
            .await
            .is_err()
        );
    }
    let (message, signature) = token.rsplit_once('.').unwrap();
    let corrupt = format!(
        "{message}.{}{}",
        if signature.starts_with('A') { 'B' } else { 'A' },
        &signature[1..]
    );
    assert!(
        validate_id_token(
            &Client::new(),
            &endpoints,
            &corrupt,
            "oaiapp_test",
            Some("expected")
        )
        .await
        .is_err()
    );
    let mut multiple = claims.clone();
    multiple["aud"] = json!(["oaiapp_test", "other"]);
    assert!(
        validate_id_token(
            &Client::new(),
            &endpoints,
            &sign(&multiple),
            "oaiapp_test",
            Some("expected")
        )
        .await
        .is_err()
    );
    multiple["azp"] = json!("oaiapp_test");
    assert!(
        validate_id_token(
            &Client::new(),
            &endpoints,
            &sign(&multiple),
            "oaiapp_test",
            Some("expected")
        )
        .await
        .is_ok()
    );
    task.abort();
}

#[test]
fn pkce_uses_sha256_base64url_and_unique_secrets() {
    let verifier = "dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk";
    assert_eq!(
        URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes())),
        "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM"
    );
    assert_ne!(random_secret(), random_secret());
}

#[tokio::test]
async fn oauth_generation_uses_internal_stream_and_disables_classification() {
    use crate::{
        config::Config,
        provider::{Operation, Provider},
    };
    use axum::http::HeaderMap;
    use tokio_util::sync::CancellationToken;
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("auth.json");
    let store = Store::open(&path).unwrap();
    let mut c = credentials(&store.host_id);
    c.expires_at = now() + 3600;
    store.save(&c).unwrap();
    drop(store);
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}/v1/", listener.local_addr().unwrap());
    let app=Router::new().route("/v1/responses",post(|headers:HeaderMap,Json(body):Json<Value>|async move {
        assert_eq!(headers["authorization"],"Bearer old-access");assert_eq!(body["stream"],true);assert_eq!(body["store"],false);assert!(body.get("background").is_none());
        assert_eq!(body["input"][0]["content"][1], json!({"type":"input_image","image_url":"data:image/png;base64,eA==","detail":"high"}));
        ([("content-type","text/event-stream")],"data: {\"type\":\"response.completed\",\"response\":{\"status\":\"completed\",\"output\":[],\"future\":true}}\n\n")
    }));
    let task = tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    let config = Config::from_lookup(|k| match k {
        "OPENAI_BASE_URL" => Some(url.clone()),
        "LLM_MCP_AUTH" => Some("oauth".into()),
        "LLM_MCP_OAUTH_CREDENTIALS_PATH" => Some(path.to_str().unwrap().into()),
        _ => None,
    })
    .unwrap();
    let provider = Provider::new(config).unwrap();
    assert_eq!(
        provider
            .call(
                Operation::Generate,
                json!({"input":[{"role":"user","content":[{"type":"input_text","text":"Describe"},{"type":"input_image","image_url":"data:image/png;base64,eA==","detail":"high"}]}],"background":false}),
                CancellationToken::new()
            )
            .await
            .unwrap()["future"],
        true
    );
    assert_eq!(
        provider
            .call(Operation::Classify, json!({}), CancellationToken::new())
            .await
            .unwrap_err()
            .code,
        "tool_disabled"
    );
    let mcp = crate::mcp::McpServer::new(provider.clone());
    let disabled = mcp
        .invoke("classify", json!({}), CancellationToken::new())
        .await
        .unwrap();
    assert_eq!(
        disabled.structured_content.unwrap()["error"]["code"],
        "tool_disabled"
    );
    assert!(rmcp::ServerHandler::get_tool(&mcp, "classify").is_none());
    let disabled = mcp
        .invoke("embed", json!({}), CancellationToken::new())
        .await
        .unwrap();
    assert_eq!(
        disabled.structured_content.unwrap()["error"]["code"],
        "tool_disabled"
    );
    assert!(rmcp::ServerHandler::get_tool(&mcp, "embed").is_none());
    assert_eq!(
        provider
            .call(
                Operation::Generate,
                json!({"input":[],"temperature":1}),
                CancellationToken::new()
            )
            .await
            .unwrap_err()
            .code,
        "unsupported_oauth_option"
    );
    task.abort();
}

#[tokio::test]
async fn oauth_stream_failures_preserve_provider_context() {
    use crate::{
        config::Config,
        provider::{Operation, Provider},
    };
    use tokio_util::sync::CancellationToken;
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("auth.json");
    let store = Store::open(&path).unwrap();
    let mut record = credentials(&store.host_id);
    record.expires_at = now() + 3600;
    store.save(&record).unwrap();
    drop(store);
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}/v1/", listener.local_addr().unwrap());
    let server = tokio::spawn(async move {
        axum::serve(listener,Router::new().route("/v1/responses",post(|Json(body):Json<Value>|async move {
            let body=match body["input"][0]["content"].as_str().unwrap() {
                "truncated" => "data: {\"type\":\"response.output_text.delta\",\"delta\":\"hello\"}\n\n".into(),
                "failed" => "data: {\"type\":\"response.failed\"}\n\n".into(),
                "malformed" => "data: malformed\n\n".into(),
                "oversized" => format!(": {}\n\n","x".repeat(1024)),
                _ => unreachable!(),
            };
            ([("content-type","text/event-stream"),("x-request-id","req-stream")],body)
        }))).await.unwrap();
    });
    let config = Config::from_lookup(|key| match key {
        "OPENAI_BASE_URL" => Some(url.clone()),
        "LLM_MCP_AUTH" => Some("oauth".into()),
        "LLM_MCP_OAUTH_CREDENTIALS_PATH" => Some(path.to_str().unwrap().into()),
        "LLM_MCP_MAX_RESPONSE_BYTES" => Some("512".into()),
        _ => None,
    })
    .unwrap();
    let provider = Provider::new(config).unwrap();
    for (input, code) in [
        ("truncated", "provider_incomplete"),
        ("failed", "provider_incomplete"),
        ("malformed", "provider_protocol"),
        ("oversized", "response_too_large"),
    ] {
        let error = provider
            .call(
                Operation::Generate,
                json!({"input":[{"role":"user","content":input}]}),
                CancellationToken::new(),
            )
            .await
            .unwrap_err();
        assert_eq!(error.code, code);
        assert_eq!(error.status, Some(200));
        assert_eq!(error.request_id.as_deref(), Some("req-stream"));
    }
    provider.shutdown().await;
    server.abort();
}

#[tokio::test]
async fn oauth_accepts_untyped_event_streams_without_weakening_validation() {
    use crate::{config::Config, mcp::McpServer, provider::Provider};
    use axum::{body::Body, http::Response};
    use tokio_util::sync::CancellationToken;

    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("auth.json");
    let store = Store::open(&path).unwrap();
    let mut record = credentials(&store.host_id);
    record.expires_at = now() + 3600;
    store.save(&record).unwrap();
    drop(store);

    let expected =
        json!({"id":"resp_live_header", "status":"completed", "output":[], "future_field":true});
    let completed = expected.clone();
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}/v1/", listener.local_addr().unwrap());
    let app = Router::new().route(
        "/v1/responses",
        post(move |Json(body): Json<Value>| {
            let completed = completed.clone();
            async move {
                let case = body["input"][0]["content"].as_str().unwrap();
                let content_type = match case {
                    "typed" => Some("text/event-stream"),
                    "mixed-case" => Some("Text/Event-Stream; charset=utf-8"),
                    "explicit-json" => Some("application/json"),
                    "misleading" => Some("text/event-stream-json"),
                    _ => None,
                };
                let bytes = match case {
                    "json" => completed.to_string(),
                    "truncated" => "data: {\"type\":\"response.created\"}\n\n".into(),
                    "oversized" => format!(": {}\n\n", "x".repeat(1024)),
                    _ => format!(
                        "data: {}\n\n",
                        json!({"type":"response.completed", "response":completed})
                    ),
                };
                let mut response = Response::builder().header("x-request-id", "req-live-header");
                if let Some(value) = content_type {
                    response = response.header("content-type", value);
                }
                response.body(Body::from(bytes)).unwrap()
            }
        }),
    );
    let task = tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    let config = Config::from_lookup(|key| match key {
        "OPENAI_BASE_URL" => Some(url.clone()),
        "LLM_MCP_AUTH" => Some("oauth".into()),
        "LLM_MCP_OAUTH_CREDENTIALS_PATH" => Some(path.to_str().unwrap().into()),
        "LLM_MCP_MAX_RESPONSE_BYTES" => Some("512".into()),
        _ => None,
    })
    .unwrap();
    let provider = Provider::new(config).unwrap();
    let mcp = McpServer::new(provider.clone());
    for (case, error) in [
        ("missing", None),
        ("typed", None),
        ("mixed-case", None),
        ("explicit-json", Some("provider_protocol")),
        ("misleading", Some("provider_protocol")),
        ("json", Some("provider_incomplete")),
        ("truncated", Some("provider_incomplete")),
        ("oversized", Some("response_too_large")),
    ] {
        let result = mcp
            .invoke(
                "generate",
                json!({"model":"mock", "input":[{"role":"user", "content":case}]}),
                CancellationToken::new(),
            )
            .await
            .unwrap();
        if let Some(code) = error {
            assert_eq!(result.is_error, Some(true), "{case}");
            let value = result.structured_content.unwrap();
            assert_eq!(value["error"]["code"], code, "{case}");
            assert_eq!(value["error"]["request_id"], "req-live-header", "{case}");
        } else {
            assert_eq!(
                result.is_error,
                Some(false),
                "{case}: {:?}",
                result.structured_content
            );
            assert_eq!(result.structured_content, Some(expected.clone()), "{case}");
        }
    }
    provider.shutdown().await;
    task.abort();
}
