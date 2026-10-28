//! Offline regressions for the process/credential driver used by live provider tests.

#[path = "support/live.rs"]
mod live;

use live::{Credential, Server, Transport};
use serde_json::{Value, json};
use std::time::Duration;

#[test]
fn credential_gates_distinguish_absence_from_present_invalid_state() {
    assert!(live::api_key(|_| None).is_none());
    assert!(live::api_key(|_| Some(String::new())).is_none());
    assert!(matches!(
        live::api_key(|_| Some("test".into())),
        Some(Credential::ApiKey(_))
    ));
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("auth.json");
    let get = |key: &str| {
        (key == "LLM_MCP_OAUTH_CREDENTIALS_PATH").then(|| path.to_string_lossy().into_owned())
    };
    assert!(live::oauth(get).unwrap().is_none());
    std::fs::write(&path, "invalid credential record").unwrap();
    assert!(matches!(
        live::oauth(get).unwrap(),
        Some(Credential::OAuth(_))
    ));
    std::fs::remove_file(&path).unwrap();
    std::fs::write(
        path.with_extension("pending.json"),
        "invalid pending record",
    )
    .unwrap();
    assert!(live::oauth(get).unwrap().is_some());
    // A non-directory path component produces an inspection error, not a skip.
    assert!(
        live::oauth(|key| (key == "LLM_MCP_OAUTH_CREDENTIALS_PATH").then(|| path
            .with_extension("pending.json")
            .join("auth.json")
            .to_string_lossy()
            .into_owned()))
        .is_err()
    );
    let default = temp.path().join(".config/llm-mcp");
    std::fs::create_dir_all(&default).unwrap();
    std::fs::write(default.join("auth.json"), "invalid").unwrap();
    assert!(
        live::oauth(|key| (key == "HOME").then(|| temp.path().to_string_lossy().into_owned()))
            .unwrap()
            .is_some()
    );
}

#[test]
fn child_configuration_separates_authentication_and_pins_provider_endpoint() {
    for credential in [
        Credential::ApiKey("test".into()),
        Credential::OAuth("/test/auth.json".into()),
    ] {
        let command = live::command(
            env!("CARGO_BIN_EXE_llm-mcp"),
            &credential,
            live::OPENAI,
            Duration::from_secs(20),
        );
        let vars: std::collections::HashMap<_, _> = command
            .as_std()
            .get_envs()
            .map(|(k, v)| {
                (
                    k.to_string_lossy().into_owned(),
                    v.map(|v| v.to_string_lossy().into_owned()),
                )
            })
            .collect();
        assert_eq!(vars["LLM_MCP_AUTH"].as_deref(), Some(credential.mode()));
        assert_eq!(
            vars["OPENAI_BASE_URL"].as_deref(),
            Some("https://api.openai.com/v1/")
        );
        assert_eq!(vars["LLM_MCP_TIMEOUT_MS"].as_deref(), Some("20000"));
        match credential {
            Credential::ApiKey(_) => {
                assert!(vars["OPENAI_API_KEY"].is_some());
                assert!(vars["LLM_MCP_OAUTH_CREDENTIALS_PATH"].is_none());
            }
            Credential::OAuth(_) => {
                assert!(vars["OPENAI_API_KEY"].is_none());
                assert!(vars["LLM_MCP_OAUTH_CREDENTIALS_PATH"].is_some());
            }
        }
    }
}

#[tokio::test]
async fn independent_driver_handles_both_transports_and_releases_processes() {
    use axum::{Json, Router, routing::post};
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let endpoint = format!("http://{}/v1/", listener.local_addr().unwrap());
    let task = tokio::spawn(async move {
        axum::serve(listener,Router::new().route("/v1/responses",post(||async { Json(json!({"status":"completed","output":[{"type":"message","content":[{"type":"output_text","text":"OK"}]}]})) }))).await.unwrap();
    });
    for transport in [Transport::Stdio, Transport::Http] {
        assert!(!transport.name().is_empty());
        let mut server = Server::start(
            env!("CARGO_BIN_EXE_llm-mcp"),
            &Credential::ApiKey("test".into()),
            transport,
            &endpoint,
            Duration::from_secs(5),
        )
        .await
        .unwrap();
        assert!(server.pid().is_some());
        let listed = server.rpc("tools/list", None).await.unwrap();
        assert_eq!(listed["tools"].as_array().unwrap().len(), 4);
        let result = server
            .call("generate", json!({"model":"test", "input":"hello"}))
            .await
            .unwrap();
        assert_eq!(live::structured(&result).unwrap()["status"], "completed");
        // A failed call does not prevent explicit cleanup.
        let result = server
            .call("generate", json!({"stream":true}))
            .await
            .unwrap();
        assert!(live::structured(&result).is_err());
        server.shutdown().await.unwrap();
    }
    task.abort();
}

#[tokio::test]
async fn present_invalid_oauth_credentials_fail_server_startup_in_both_transports() {
    let temp = tempfile::tempdir().unwrap();
    // A directory is intentionally present but is not a usable credential record.
    let path = temp.path().join("auth.json");
    std::fs::create_dir(&path).unwrap();
    let credential = Credential::OAuth(path);
    for transport in [Transport::Stdio, Transport::Http] {
        let result = Server::start(
            env!("CARGO_BIN_EXE_llm-mcp"),
            &credential,
            transport,
            live::OPENAI,
            Duration::from_secs(1),
        )
        .await;
        assert!(result.is_err());
    }
}

#[tokio::test]
async fn rejected_api_credentials_are_failures_not_skips() {
    use axum::{Json, Router, http::StatusCode, routing::post};
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let endpoint = format!("http://{}/v1/", listener.local_addr().unwrap());
    let task = tokio::spawn(async move {
        axum::serve(
            listener,
            Router::new().route(
                "/v1/responses",
                post(|| async {
                    (
                        StatusCode::UNAUTHORIZED,
                        [("x-request-id", "req-rejected")],
                        Json(json!({"error":{"message":"private-secret"}})),
                    )
                }),
            ),
        )
        .await
        .unwrap();
    });
    for transport in [Transport::Stdio, Transport::Http] {
        let mut server = Server::start(
            env!("CARGO_BIN_EXE_llm-mcp"),
            &Credential::ApiKey("invalid-test".into()),
            transport,
            &endpoint,
            Duration::from_secs(3),
        )
        .await
        .unwrap();
        let result = server
            .call("generate", json!({"input":"test"}))
            .await
            .unwrap();
        let error = live::structured(&result).unwrap_err().to_string();
        assert!(error.contains("authentication"));
        assert!(error.contains("401"));
        assert!(error.contains("req-rejected"));
        assert!(!error.contains("private-secret"));
        server.shutdown().await.unwrap();
    }
    task.abort();
}

#[test]
fn diagnostics_do_not_echo_provider_messages_or_untrusted_response_fields() {
    let result = json!({"isError":true,"structuredContent":{"error":{"code":"authentication","message":"private-secret","status":401,"request_id":"req-safe"}}});
    let error = live::structured(&result).unwrap_err().to_string();
    assert!(error.contains("authentication"));
    assert!(error.contains("401"));
    assert!(error.contains("req-safe"));
    assert!(!error.contains("private-secret"));
    let result = json!({"isError":true,"structuredContent":{"error":{"code":"bad\nsecret","request_id":"private\nsecret"}}});
    assert!(
        !live::structured(&result)
            .unwrap_err()
            .to_string()
            .contains("secret")
    );
    for result in [
        json!({}),
        json!({"structuredContent":{},"content":[]}),
        json!({"structuredContent":{},"content":[{"text":"{\"different\":true}"}]}),
    ] {
        assert!(live::structured(&result).is_err());
    }
    let good = json!({"structuredContent":{},"content":[{"text":"{}"}]});
    assert_eq!(
        live::structured(&good).unwrap(),
        &Value::Object(Default::default())
    );
}

#[test]
fn real_media_fixtures_have_valid_headers_and_nonempty_speech() {
    use base64::{Engine as _, engine::general_purpose::STANDARD};
    let image = STANDARD
        .decode(include_str!("fixtures/shapes.png.base64").trim())
        .unwrap();
    assert_eq!(&image[..8], b"\x89PNG\r\n\x1a\n");
    assert_eq!(u32::from_be_bytes(image[16..20].try_into().unwrap()), 480);
    assert_eq!(u32::from_be_bytes(image[20..24].try_into().unwrap()), 260);
    let audio = STANDARD
        .decode(include_str!("fixtures/shapes.wav.base64").trim())
        .unwrap();
    assert_eq!(&audio[..4], b"RIFF");
    assert_eq!(&audio[8..12], b"WAVE");
    assert_eq!(
        u32::from_le_bytes(audio[4..8].try_into().unwrap()) as usize,
        audio.len() - 8
    );
    assert!(audio.len() > 40_000);
    assert!(
        audio[44..]
            .as_chunks::<2>()
            .0
            .iter()
            .any(|sample| i16::from_le_bytes(*sample).unsigned_abs() > 1000)
    );
}
