//! Native OpenRouter HTTP contracts, independent of live credentials.

use axum::{
    Json, Router,
    extract::State,
    http::{HeaderMap, StatusCode, Uri},
    routing::post,
};
use llm_mcp::{
    config::Config,
    provider::{Operation, Provider},
};
use serde_json::{Value, json};
use tokio::{net::TcpListener, sync::mpsc};
use tokio_util::sync::CancellationToken;

#[derive(Clone)]
struct Mock {
    captured: mpsc::Sender<(String, HeaderMap, Value)>,
    response: Value,
    status: StatusCode,
}

async fn handle(
    State(state): State<Mock>,
    uri: Uri,
    headers: HeaderMap,
    Json(body): Json<Value>,
) -> (StatusCode, Json<Value>) {
    state
        .captured
        .send((uri.path().into(), headers, body))
        .await
        .unwrap();
    (state.status, Json(state.response))
}

struct Fixture {
    provider: Provider,
    captured: mpsc::Receiver<(String, HeaderMap, Value)>,
    task: tokio::task::JoinHandle<()>,
}

impl Drop for Fixture {
    fn drop(&mut self) {
        self.task.abort();
    }
}

async fn fixture(response: Value, status: StatusCode, settings: &[(&str, &str)]) -> Fixture {
    let (captured, rx) = mpsc::channel(20);
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}/api/v1/", listener.local_addr().unwrap());
    let app = Router::new()
        .route("/api/v1/responses", post(handle))
        .route("/api/v1/embeddings", post(handle))
        .route("/api/alpha/decisions", post(handle))
        .with_state(Mock {
            captured,
            response,
            status,
        });
    let task = tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    let config = Config::from_lookup(|key| {
        if let Some((_, value)) = settings.iter().find(|(name, _)| *name == key) {
            return Some((*value).into());
        }
        match key {
            "LLM_MCP_PROVIDER" => Some("openrouter".into()),
            "OPENROUTER_BASE_URL" => Some(url.clone()),
            "OPENROUTER_API_KEY" => Some("router-key".into()),
            "OPENAI_API_KEY" => Some("wrong-key".into()),
            "OPENAI_ORGANIZATION" | "OPENAI_PROJECT" => Some("wrong-header".into()),
            _ => None,
        }
    })
    .unwrap();
    Fixture {
        provider: Provider::new(config).unwrap(),
        captured: rx,
        task,
    }
}

#[tokio::test]
async fn native_endpoint_routing_preserves_json_and_isolates_headers() {
    let response = json!({"future": {"preserve": true}});
    let mut f = fixture(response.clone(), StatusCode::OK, &[]).await;
    for (operation, path, request) in [
        (
            Operation::Generate,
            "/api/v1/responses",
            json!({"model":"author/model","input":"hello","future_option":true}),
        ),
        (
            Operation::Classify,
            "/api/alpha/decisions",
            json!({"model":"typesafe/jev-latest","state":"hello","questions":{"relevant":{"type":"noul","instructions":"Relevant?"}}}),
        ),
    ] {
        assert_eq!(
            f.provider
                .call(operation, request.clone(), CancellationToken::new())
                .await
                .unwrap(),
            response
        );
        let (actual_path, headers, body) = f.captured.recv().await.unwrap();
        assert_eq!(actual_path, path);
        assert_eq!(headers["authorization"], "Bearer router-key");
        assert!(!headers.contains_key("openai-organization"));
        assert!(!headers.contains_key("openai-project"));
        assert_eq!(body, request);
    }
}

#[tokio::test]
async fn provider_errors_remain_credential_and_body_free() {
    let mut f = fixture(
        json!({"error":{"message":"private prompt router-key"}}),
        StatusCode::TOO_MANY_REQUESTS,
        &[],
    )
    .await;
    let error = f
        .provider
        .call(Operation::Generate, json!({}), CancellationToken::new())
        .await
        .unwrap_err();
    assert_eq!(error.code, "rate_limited");
    assert_eq!(error.status, Some(429));
    assert!(error.retryable);
    assert!(!error.message.contains("OpenAI"));
    assert!(!error.message.contains("private"));
    assert!(!error.message.contains("router-key"));
    assert!(f.captured.recv().await.is_some());
}
