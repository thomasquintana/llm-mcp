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
    let mut session = OAuthSession::open(&path, Client::new()).unwrap();
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
    use rsa::{RsaPrivateKey, pkcs1::EncodeRsaPrivateKey, traits::PublicKeyParts};
    let key = RsaPrivateKey::new(&mut rand08::rngs::OsRng, 2048).unwrap();
    let jwks = json!({"keys":[{"kty":"RSA","kid":"test","alg":"RS256","use":"sig","n":URL_SAFE_NO_PAD.encode(key.n().to_bytes_be()),"e":URL_SAFE_NO_PAD.encode(key.e().to_bytes_be())}]});
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
    let encoding = jsonwebtoken::EncodingKey::from_rsa_der(key.to_pkcs1_der().unwrap().as_bytes());
    let mut header = jsonwebtoken::Header::new(Algorithm::RS256);
    header.kid = Some("test".into());
    let claims =
        json!({"iss":ISSUER,"aud":"oaiapp_test","exp":now()+3600,"sub":"user","nonce":"expected"});
    let token = jsonwebtoken::encode(&header, &claims, &encoding).unwrap();
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
        let token = jsonwebtoken::encode(&header, &invalid, &encoding).unwrap();
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
async fn oauth_generation_uses_internal_stream_and_classification_uses_key() {
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
        ([("content-type","text/event-stream")],"data: {\"type\":\"response.completed\",\"response\":{\"status\":\"completed\",\"output\":[],\"future\":true}}\n\n")
    })).route("/v1/decisions",post(|headers:HeaderMap|async move {assert_eq!(headers["authorization"],"Bearer platform-key");Json(json!({"answers":[]}))}));
    let task = tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    let config = Config::from_lookup(|k| match k {
        "OPENAI_BASE_URL" => Some(url.clone()),
        "OPENAI_API_KEY" => Some("platform-key".into()),
        "LLM_MCP_GENERATE_AUTH" => Some("oauth".into()),
        "LLM_MCP_OAUTH_CREDENTIALS_PATH" => Some(path.to_str().unwrap().into()),
        _ => None,
    })
    .unwrap();
    let provider = Provider::new(config).unwrap();
    assert_eq!(
        provider
            .call(
                Operation::Generate,
                json!({"input":[],"background":false}),
                CancellationToken::new()
            )
            .await
            .unwrap()["future"],
        true
    );
    assert!(
        provider
            .call(Operation::Classify, json!({}), CancellationToken::new())
            .await
            .is_ok()
    );
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
