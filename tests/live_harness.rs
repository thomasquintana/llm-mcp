//! Offline regressions for the process/credential driver used by live provider tests.

#[path = "support/live.rs"]
mod live;
#[path = "support/provider_cases.rs"]
mod provider_cases;

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

#[test]
fn live_case_selection_respects_standard_test_filters_without_implicit_opt_in() {
    use provider_cases::{API_SUITE, OAUTH_SUITE, Selection};
    let all = Selection::parse([]).unwrap();
    assert!(all.includes(API_SUITE) && all.includes(OAUTH_SUITE));
    let oauth =
        Selection::parse(["--nocapture", "oauth", "--test-threads", "1"].map(str::to_owned))
            .unwrap();
    assert!(!oauth.includes(API_SUITE) && oauth.includes(OAUTH_SUITE));
    let skip = Selection::parse(["--skip", OAUTH_SUITE].map(str::to_owned)).unwrap();
    assert!(skip.includes(API_SUITE) && !skip.includes(OAUTH_SUITE));
    let ignored = Selection::parse(["--ignored".into()]).unwrap();
    assert!(!ignored.includes(API_SUITE) && !ignored.includes(OAUTH_SUITE));
    let listed = Selection::parse(["--list", "--exact", API_SUITE].map(str::to_owned)).unwrap();
    assert!(listed.list && listed.includes(API_SUITE) && !listed.includes(OAUTH_SUITE));
    assert!(Selection::parse(["--skip".into()]).is_err());
    assert!(
        !Selection::parse(["--unknown-secret".into()])
            .err()
            .unwrap()
            .to_string()
            .contains("secret")
    );
}

#[test]
fn live_model_settings_and_timeouts_are_explicit_and_mode_specific() {
    assert_eq!(
        provider_cases::timeout(|_| None).unwrap(),
        Duration::from_secs(120)
    );
    assert_eq!(
        provider_cases::timeout(|_| Some("1234".into())).unwrap(),
        Duration::from_millis(1234)
    );
    for value in ["0", "not-a-number"] {
        assert!(provider_cases::timeout(|_| Some(value.into())).is_err());
    }
    let api = provider_cases::cases(&Credential::ApiKey("test".into()), |_| None).unwrap();
    assert_eq!(api.len(), 7);
    assert_eq!(
        api.iter().map(|case| case.name).collect::<Vec<_>>(),
        [
            "generate_text",
            "generate_image",
            "classify_text",
            "classify_image",
            "embed_text",
            "embed_images",
            "transcribe_audio"
        ]
    );
    assert_eq!(api[4].tool, "embed");
    assert_eq!(
        api[5].arguments["inputs"][0]["content"]
            .as_array()
            .unwrap()
            .len(),
        3
    );
    assert_eq!(api[6].tool, "transcribe");
    let oauth = provider_cases::cases(&Credential::OAuth("unused".into()), |key| {
        (key == "LLM_MCP_E2E_OAUTH_MODEL").then(|| "account-model".into())
    })
    .unwrap();
    assert_eq!(oauth.len(), 5);
    assert_eq!(oauth[0].model, "account-model");
    assert_eq!(oauth[1].model, "account-model");
    assert!(
        provider_cases::cases(&Credential::ApiKey("test".into()), |_| Some(
            "bad\nsecret".into()
        ))
        .is_err()
    );
    assert!(
        provider_cases::check_discovery(
            &json!({"tools":[{"name":"generate"}]}),
            &Credential::OAuth("unused".into())
        )
        .is_ok()
    );
    assert!(
        provider_cases::check_discovery(
            &json!({"tools":[{"name":"generate"}]}),
            &Credential::ApiKey("test".into())
        )
        .is_err()
    );
}

fn tool_success(value: Value) -> Value {
    json!({"isError":false,"structuredContent":value,"content":[{"type":"text","text":value.to_string()}]})
}

#[test]
fn live_semantic_checks_reject_empty_wrong_or_malformed_provider_results() {
    let api = provider_cases::cases(&Credential::ApiKey("test".into()), |_| None).unwrap();
    let generated = |text: &str| json!({"output":[{"type":"message","content":[{"type":"output_text","text":text}]}]});
    assert!(api[0].evaluate(&tool_success(generated("Orchid."))).is_ok());
    assert!(
        api[0]
            .evaluate(&tool_success(generated("Nothing.")))
            .is_err()
    );
    assert!(
        api[1]
            .evaluate(&tool_success(generated(
                "A RED square, a blue circle. Labels: RED 42 and BLUE."
            )))
            .is_ok()
    );
    assert!(
        api[1]
            .evaluate(&tool_success(generated("A green triangle.")))
            .is_err()
    );
    for (index, expected) in [(2, "left"), (3, "42")] {
        let classified = json!({"answers":[{"name":"red_square","type":"predicate","probability":0.99},{"name":"selection","type":"choice","choice":expected,"confidence":0.99}]});
        assert!(
            api[index]
                .evaluate(&tool_success(classified.clone()))
                .is_ok()
        );
        let mut refused = classified;
        refused["answers"][0] = json!({"name":"red_square","type":"refusal"});
        assert!(api[index].evaluate(&tool_success(refused)).is_err());
    }
    let vectors =
        json!({"data":[{"index":0,"embedding":vec![0.1;64]},{"index":1,"embedding":vec![0.2;64]}]});
    assert!(
        api[4]
            .evaluate(&tool_success(
                json!({"response":vectors,"prepared_inputs":[]})
            ))
            .is_ok()
    );
    let mut wrong = vectors.clone();
    wrong["data"][1]["index"] = json!(0);
    assert!(
        api[4]
            .evaluate(&tool_success(
                json!({"response":wrong,"prepared_inputs":[]})
            ))
            .is_err()
    );
    let description = "A red square labeled RED 42 and a blue circle labeled BLUE.";
    let prepared = json!([{"index":0,"description":description,"text":format!("Context:\nCatalog item: shapes-fixture-42.\n\nImage description:\n{description}"),"description_model":"vision","description_usage":{"input_tokens":10}},{"index":1,"text":"An independent text-only item."}]);
    assert!(
        api[5]
            .evaluate(&tool_success(
                json!({"response":vectors,"prepared_inputs":prepared})
            ))
            .is_ok()
    );
    let mut lost_context = prepared;
    lost_context[0]["text"] = json!(description);
    assert!(
        api[5]
            .evaluate(&tool_success(
                json!({"response":vectors,"prepared_inputs":lost_context})
            ))
            .is_err()
    );
    assert!(
        api[6]
            .evaluate(&tool_success(
                json!({"text":"The red square is next to the blue circle!"})
            ))
            .is_ok()
    );
    assert!(api[6].evaluate(&tool_success(json!({"text":""}))).is_err());
    for case in &api {
        assert!(case.evaluate(&tool_success(json!({}))).is_err());
    }
    let oauth = provider_cases::cases(&Credential::OAuth("unused".into()), |_| None).unwrap();
    for case in &oauth[2..] {
        assert!(
            case.evaluate(
                &json!({"isError":true,"structuredContent":{"error":{"code":"tool_disabled"}}})
            )
            .is_ok()
        );
        assert!(case.evaluate(&tool_success(json!({}))).is_err());
    }
}
