//! Provider contracts tested over independent local HTTP exchanges.

use axum::{
    Json, Router,
    extract::State,
    http::{HeaderMap, StatusCode},
    routing::post,
};
use llm_mcp::{
    config::Config,
    mcp::McpServer,
    provider::{Operation, Provider},
};
use serde_json::{Value, json};
use std::{collections::HashMap, sync::Arc, time::Duration};
use tokio::{
    net::TcpListener,
    sync::{Mutex, mpsc},
};
use tokio_util::sync::CancellationToken;

#[derive(Clone)]
struct Mock {
    captured: mpsc::Sender<(HeaderMap, Value)>,
    response: Value,
    status: StatusCode,
    delay: Duration,
}

async fn handle(
    State(state): State<Mock>,
    headers: HeaderMap,
    Json(body): Json<Value>,
) -> (StatusCode, HeaderMap, Json<Value>) {
    state.captured.send((headers, body)).await.unwrap();
    tokio::time::sleep(state.delay).await;
    let mut headers = HeaderMap::new();
    headers.insert("x-request-id", "req-test".parse().unwrap());
    (state.status, headers, Json(state.response))
}

struct Fixture {
    provider: Provider,
    captured: Arc<Mutex<mpsc::Receiver<(HeaderMap, Value)>>>,
    task: tokio::task::JoinHandle<()>,
}

impl Drop for Fixture {
    fn drop(&mut self) {
        self.task.abort();
    }
}

async fn fixture(
    response: Value,
    status: StatusCode,
    delay: Duration,
    settings: &[(&str, &str)],
) -> Fixture {
    let (tx, rx) = mpsc::channel(20);
    let state = Mock {
        captured: tx,
        response,
        status,
        delay,
    };
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}/v1/", listener.local_addr().unwrap());
    let app = Router::new()
        .route("/v1/responses", post(handle))
        .route("/v1/decisions", post(handle))
        .with_state(state);
    let task = tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    let mut env = HashMap::from([
        ("OPENAI_API_KEY", "test-key".to_owned()),
        ("OPENAI_BASE_URL", url),
    ]);
    env.extend(settings.iter().map(|(k, v)| (*k, (*v).to_owned())));
    let config = Config::from_lookup(|key| env.get(key).cloned()).unwrap();
    Fixture {
        provider: Provider::new(config).unwrap(),
        captured: Arc::new(Mutex::new(rx)),
        task,
    }
}

#[tokio::test]
async fn preserves_full_request_and_response_and_headers() {
    let response = json!({"id":"resp_1","status":"completed","output":[{"type":"function_call","call_id":"c1","name":"lookup","arguments":"{}"}],"usage":{"input_tokens":5},"future":{"x":true}});
    let f = fixture(
        response.clone(),
        StatusCode::OK,
        Duration::ZERO,
        &[
            ("OPENAI_ORGANIZATION", "org-test"),
            ("OPENAI_PROJECT", "proj-test"),
        ],
    )
    .await;
    let request = json!({"model":"some-model","input":[{"role":"user","content":"hello"},{"type":"function_call","call_id":"prior","name":"lookup","arguments":"{}"},{"type":"function_call_output","call_id":"prior","output":"data"}],"tools":[{"type":"function","name":"lookup","parameters":{"type":"object"}}],"reasoning":{"effort":"high"},"text":{"format":{"type":"json_schema","name":"answer","schema":{"type":"object"}}},"future_option":{"enabled":true}});
    let result = f
        .provider
        .call(
            Operation::Generate,
            request.clone(),
            CancellationToken::new(),
        )
        .await
        .unwrap();
    assert_eq!(result, response);
    let (headers, body) = f.captured.lock().await.recv().await.unwrap();
    assert_eq!(body, request);
    assert_eq!(headers["authorization"], "Bearer test-key");
    assert_eq!(headers["openai-organization"], "org-test");
    assert_eq!(headers["openai-project"], "proj-test");
}

#[tokio::test]
async fn preserves_batched_classification_and_refusals() {
    let response = json!({"answers":[{"name":"a","type":"predicate","probability":0.8},{"name":"b","type":"refusal"}],"usage":{"input_tokens":10}});
    let f = fixture(response.clone(), StatusCode::OK, Duration::ZERO, &[]).await;
    let server = McpServer::new(f.provider.clone());
    let result = server.invoke("classify", json!({"model":"gpt-6-luna","input":"text","questions":[{"name":"a","type":"predicate","instructions":"Relevant?"},{"name":"b","type":"score","instructions":"Severity?","levels":[{"label":"low"},{"label":"high"}]}]}), CancellationToken::new()).await.unwrap();
    assert_eq!(result.structured_content, Some(response.clone()));
    assert_eq!(
        result.content[0].as_text().unwrap().text,
        response.to_string()
    );
    assert_eq!(result.is_error, Some(false));
    assert!(result.result_type.is_none());
}

#[tokio::test]
async fn rejects_background_and_streaming_before_network_dispatch() {
    let f = fixture(json!({}), StatusCode::OK, Duration::ZERO, &[]).await;
    for request in [
        json!({"background":true}),
        json!({"stream":true}),
        json!({"background":"false"}),
        json!([]),
    ] {
        let error = f
            .provider
            .call(Operation::Generate, request, CancellationToken::new())
            .await
            .unwrap_err();
        assert_eq!(error.code, "invalid_request");
    }
    assert!(f.captured.lock().await.try_recv().is_err());
}

#[tokio::test]
async fn reports_rate_limit_without_retry_or_body_leaks() {
    let f = fixture(
        json!({"error":{"message":"private secret"}}),
        StatusCode::TOO_MANY_REQUESTS,
        Duration::ZERO,
        &[],
    )
    .await;
    let error = f
        .provider
        .call(Operation::Generate, json!({}), CancellationToken::new())
        .await
        .unwrap_err();
    assert_eq!(error.code, "rate_limited");
    assert!(error.retryable);
    assert_eq!(error.request_id.as_deref(), Some("req-test"));
    assert!(!error.to_string().contains("private"));
    assert!(f.captured.lock().await.try_recv().is_ok());
    assert!(f.captured.lock().await.try_recv().is_err());
}

#[tokio::test]
async fn deadlines_cancellation_and_payload_bounds() {
    let f = fixture(
        json!({"padding":"abcdefghijklmnop"}),
        StatusCode::OK,
        Duration::from_secs(1),
        &[("LLM_MCP_TIMEOUT_MS", "20")],
    )
    .await;
    assert_eq!(
        f.provider
            .call(Operation::Generate, json!({}), CancellationToken::new())
            .await
            .unwrap_err()
            .code,
        "timeout"
    );
    let cancel = CancellationToken::new();
    cancel.cancel();
    assert_eq!(
        f.provider
            .call(Operation::Generate, json!({}), cancel)
            .await
            .unwrap_err()
            .code,
        "cancelled"
    );
    let f = fixture(
        json!({"padding":"abcdefghijklmnop"}),
        StatusCode::OK,
        Duration::ZERO,
        &[
            ("LLM_MCP_MAX_RESPONSE_BYTES", "4"),
            ("LLM_MCP_MAX_REQUEST_BYTES", "4"),
        ],
    )
    .await;
    assert_eq!(
        f.provider
            .call(
                Operation::Generate,
                json!({"large":true}),
                CancellationToken::new()
            )
            .await
            .unwrap_err()
            .code,
        "request_too_large"
    );
    assert_eq!(
        f.provider
            .call(Operation::Generate, json!({}), CancellationToken::new())
            .await
            .unwrap_err()
            .code,
        "response_too_large"
    );
}

#[tokio::test]
async fn rejects_nonobject_and_incomplete_provider_results() {
    for response in [json!([]), json!({"status":"incomplete"})] {
        let f = fixture(response, StatusCode::OK, Duration::ZERO, &[]).await;
        assert!(
            f.provider
                .call(Operation::Generate, json!({}), CancellationToken::new())
                .await
                .is_err()
        );
    }
}

#[test]
fn validates_configuration_without_environment_mutation() {
    for (key, value) in [
        ("LLM_MCP_MAX_CONCURRENCY", "0"),
        ("LLM_MCP_GENERATE_AUTH", "automatic"),
        ("OPENAI_BASE_URL", "https://key:secret@example.com/v1"),
        ("LLM_MCP_TRANSPORT", "sse"),
    ] {
        assert!(Config::from_lookup(|k| (k == key).then(|| value.into())).is_err());
    }
    let config =
        Config::from_lookup(|k| (k == "OPENAI_API_KEY").then(|| "super-secret".into())).unwrap();
    assert!(!format!("{config:?}").contains("super-secret"));
}

#[tokio::test]
async fn rejects_malformed_json_from_provider() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}/v1/", listener.local_addr().unwrap());
    let task = tokio::spawn(async move {
        axum::serve(
            listener,
            Router::new().route("/v1/responses", post(|| async { "{broken" })),
        )
        .await
        .unwrap();
    });
    let config = Config::from_lookup(|key| match key {
        "OPENAI_BASE_URL" => Some(url.clone()),
        "OPENAI_API_KEY" => Some("test".into()),
        _ => None,
    })
    .unwrap();
    let result = Provider::new(config)
        .unwrap()
        .call(Operation::Generate, json!({}), CancellationToken::new())
        .await;
    task.abort();
    assert_eq!(result.unwrap_err().code, "provider_protocol");
}
