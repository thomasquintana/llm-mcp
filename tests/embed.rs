//! Independent HTTP contracts for contextual image embedding orchestration.

use axum::{
    Json, Router,
    extract::State,
    http::{HeaderMap, StatusCode, Uri},
    routing::post,
};
use llm_mcp::{
    config::Config,
    mcp::McpServer,
    provider::{Operation, Provider},
};
use serde_json::{Value, json};
use std::{collections::VecDeque, sync::Arc, time::Duration};
use tokio::{
    net::TcpListener,
    sync::{Mutex, mpsc},
};
use tokio_util::sync::CancellationToken;

type Reply = (StatusCode, Value, Duration);
struct Fixture {
    provider: Provider,
    captured: mpsc::UnboundedReceiver<(String, Value)>,
    task: tokio::task::JoinHandle<()>,
}
impl Drop for Fixture {
    fn drop(&mut self) {
        self.task.abort();
    }
}

async fn fixture(replies: Vec<Reply>, settings: &[(&str, &str)]) -> Fixture {
    let (tx, rx) = mpsc::unbounded_channel();
    let replies = Arc::new(Mutex::new(VecDeque::from(replies)));
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}/v1/", listener.local_addr().unwrap());
    let app = Router::new()
        .fallback(post(
            move |State(replies): State<Arc<Mutex<VecDeque<Reply>>>>,
                  headers: HeaderMap,
                  uri: Uri,
                  Json(body): Json<Value>| {
                let tx = tx.clone();
                async move {
                    assert_eq!(headers["authorization"], "Bearer test");
                    assert_eq!(headers["openai-organization"], "org-test");
                    assert_eq!(headers["openai-project"], "proj-test");
                    tx.send((uri.path().to_owned(), body)).unwrap();
                    let (status, value, delay) = replies
                        .lock()
                        .await
                        .pop_front()
                        .expect("unexpected provider call");
                    tokio::time::sleep(delay).await;
                    (status, [("x-request-id", "req-embed")], Json(value))
                }
            },
        ))
        .with_state(replies);
    let task = tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    let config = Config::from_lookup(|key| {
        if let Some((_, value)) = settings.iter().find(|(name, _)| *name == key) {
            return Some((*value).to_owned());
        }
        match key {
            "OPENAI_API_KEY" => Some("test".into()),
            "OPENAI_BASE_URL" => Some(url.clone()),
            "OPENAI_ORGANIZATION" => Some("org-test".into()),
            "OPENAI_PROJECT" => Some("proj-test".into()),
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
fn ok(value: Value) -> Reply {
    (StatusCode::OK, value, Duration::ZERO)
}
fn description(text: &str) -> Value {
    json!({"status":"completed", "model":"vision-returned", "output":[{"type":"message", "content":[{"type":"output_text", "text":text}]}], "usage":{"input_tokens":9,"output_tokens":3}})
}
fn image() -> Value {
    json!({"type":"input_image", "image_url":"data:image/png;base64,eA=="})
}
fn multimodal() -> Value {
    json!({"model":"embedding-model", "inputs":[{"content":[{"type":"input_text", "text":"Maintenance context"},image()]}], "description":{"model":"vision-model"}})
}

#[tokio::test]
async fn text_and_token_inputs_preserve_provider_options_and_full_response() {
    let response = json!({"object":"list", "model":"embedding-model", "data":[{"embedding":"AACAPw==", "index":0}], "usage":{"total_tokens":4}, "future":true});
    for input in [
        json!("text"),
        json!(["one", "two"]),
        json!([1, 2, 3]),
        json!([[1, 2], [3, 4]]),
    ] {
        let mut f = fixture(vec![ok(response.clone())], &[]).await;
        let request = json!({"model":"embedding-model", "input":input, "dimensions":1, "encoding_format":"base64", "future_option":true});
        let result = McpServer::new(f.provider.clone())
            .invoke("embed", request.clone(), CancellationToken::new())
            .await
            .unwrap();
        assert_eq!(result.is_error, Some(false));
        assert_eq!(
            result.structured_content,
            Some(json!({"response":response, "prepared_inputs":[]}))
        );
        assert_eq!(
            f.captured.recv().await.unwrap(),
            ("/v1/embeddings".into(), request)
        );
    }
}

#[tokio::test]
async fn prepares_all_images_then_embeds_context_and_descriptions_in_item_order() {
    let response = json!({"data":[{"index":0,"embedding":[0.1]},{"index":1,"embedding":[0.2]},{"index":2,"embedding":[0.3]}], "future":true});
    let mut f = fixture(
        vec![
            ok(description("Two views of pump P-17.")),
            ok(description("A red valve.")),
            ok(response.clone()),
        ],
        &[("LLM_MCP_MAX_CONCURRENCY", "1")],
    )
    .await;
    let request = json!({"model":"embedding-model", "dimensions":1, "stream":false, "background":false, "future_option":true,
        "inputs":[
            {"content":[{"type":"input_text", "text":"Text document"}]},
            {"content":[{"type":"input_text", "text":"Equipment"},image(),{"type":"input_text", "text":"Maintenance"},image()]},
            {"content":[image()]}
        ], "description":{"model":"vision-model", "instructions":"Retain visible equipment labels."}});
    let result = f
        .provider
        .call(Operation::Embed, request.clone(), CancellationToken::new())
        .await
        .unwrap();
    assert_eq!(result["response"], response);
    let prepared = &result["prepared_inputs"];
    assert_eq!(prepared[0], json!({"index":0, "text":"Text document"}));
    assert_eq!(prepared[1]["index"], 1);
    assert_eq!(prepared[1]["description"], "Two views of pump P-17.");
    assert_eq!(
        prepared[1]["text"],
        "Context:\nEquipment\n\nMaintenance\n\nImage description:\nTwo views of pump P-17."
    );
    assert_eq!(prepared[1]["description_model"], "vision-returned");
    assert_eq!(prepared[1]["description_usage"]["input_tokens"], 9);
    assert_eq!(prepared[2]["text"], "A red valve.");
    for index in [1, 2] {
        let (path, body) = f.captured.recv().await.unwrap();
        assert_eq!(path, "/v1/responses");
        assert_eq!(
            body["input"][0]["content"],
            request["inputs"][index]["content"]
        );
        assert_eq!(body["model"], "vision-model");
        assert_eq!(body["store"], false);
        assert!(
            body["instructions"]
                .as_str()
                .unwrap()
                .contains("Retain visible equipment labels.")
        );
    }
    let (path, body) = f.captured.recv().await.unwrap();
    assert_eq!(path, "/v1/embeddings");
    assert_eq!(
        body,
        json!({"model":"embedding-model", "dimensions":1, "future_option":true, "input":[prepared[0]["text"],prepared[1]["text"],prepared[2]["text"]]})
    );
    assert!(f.captured.try_recv().is_err());
}

#[tokio::test]
async fn rejects_entire_invalid_batch_before_any_description_request() {
    let mut f = fixture(vec![], &[]).await;
    let mut requests = vec![
        json!({}),
        json!({"model":"m","input":[],"inputs":[]}),
        json!({"model":"m","input":[{}]}),
        json!({"model":"m","inputs":[]}),
    ];
    let mut no_model = multimodal();
    no_model.as_object_mut().unwrap().remove("description");
    requests.push(no_model);
    let mut bad_guidance = multimodal();
    bad_guidance["description"]["instructions"] = json!(1);
    requests.push(bad_guidance);
    let mut invalid_last = multimodal();
    invalid_last["inputs"].as_array_mut().unwrap().push(
        json!({"content":[{"type":"input_image","image_url":"https://example.com/image.png"}]}),
    );
    requests.push(invalid_last);
    for request in requests {
        assert!(
            f.provider
                .call(Operation::Embed, request, CancellationToken::new())
                .await
                .is_err()
        );
    }
    assert!(f.captured.try_recv().is_err());
}

#[tokio::test]
async fn description_failures_skip_embedding_and_retain_input_and_provider_context() {
    for (reply, code) in [
        (ok(json!({"status":"incomplete"})), "provider_incomplete"),
        (
            ok(json!({"status":"completed","output":[]})),
            "description_failed",
        ),
        (
            ok(
                json!({"status":"completed","output":[{"type":"message","content":[{"type":"refusal","refusal":"private"}]}]}),
            ),
            "description_failed",
        ),
        (
            (
                StatusCode::TOO_MANY_REQUESTS,
                json!({"private":"secret"}),
                Duration::ZERO,
            ),
            "rate_limited",
        ),
    ] {
        let mut f = fixture(vec![ok(description("First image.")), reply], &[]).await;
        let mut request = multimodal();
        request["inputs"]
            .as_array_mut()
            .unwrap()
            .push(json!({"content":[image()]}));
        let error = f
            .provider
            .call(Operation::Embed, request, CancellationToken::new())
            .await
            .unwrap_err();
        assert_eq!(error.code, code);
        assert_eq!(error.input_index, Some(1));
        if code != "description_failed" {
            assert_eq!(error.request_id.as_deref(), Some("req-embed"));
        }
        assert!(!error.message.contains("secret"));
        assert_eq!(f.captured.recv().await.unwrap().0, "/v1/responses");
        assert_eq!(f.captured.recv().await.unwrap().0, "/v1/responses");
        assert!(f.captured.try_recv().is_err());
    }
}

#[tokio::test]
async fn cancelled_description_releases_capacity_and_never_starts_embedding() {
    let mut f = fixture(
        vec![
            (StatusCode::OK, description("Late"), Duration::from_secs(1)),
            ok(json!({"data":[]})),
        ],
        &[("LLM_MCP_MAX_CONCURRENCY", "1")],
    )
    .await;
    let cancel = CancellationToken::new();
    let task = {
        let provider = f.provider.clone();
        let ct = cancel.clone();
        tokio::spawn(async move { provider.call(Operation::Embed, multimodal(), ct).await })
    };
    assert_eq!(f.captured.recv().await.unwrap().0, "/v1/responses");
    cancel.cancel();
    assert_eq!(task.await.unwrap().unwrap_err().code, "cancelled");
    let result = tokio::time::timeout(
        Duration::from_secs(1),
        f.provider.call(
            Operation::Embed,
            json!({"model":"m","input":"text"}),
            CancellationToken::new(),
        ),
    )
    .await
    .unwrap()
    .unwrap();
    assert_eq!(result["response"], json!({"data":[]}));
    assert_eq!(f.captured.recv().await.unwrap().0, "/v1/embeddings");
    assert!(f.captured.try_recv().is_err());
}

#[tokio::test]
async fn pipeline_uses_one_deadline_and_bounds_assembled_results() {
    let mut f = fixture(
        vec![
            (
                StatusCode::OK,
                description("Description"),
                Duration::from_millis(60),
            ),
            (
                StatusCode::OK,
                json!({"data":[]}),
                Duration::from_millis(120),
            ),
        ],
        &[("LLM_MCP_TIMEOUT_MS", "150")],
    )
    .await;
    let error = f
        .provider
        .call(Operation::Embed, multimodal(), CancellationToken::new())
        .await
        .unwrap_err();
    assert_eq!(error.code, "timeout");
    assert_eq!(f.captured.recv().await.unwrap().0, "/v1/responses");
    assert_eq!(f.captured.recv().await.unwrap().0, "/v1/embeddings");
    let response = description("A".repeat(100).as_str());
    let mut f = fixture(
        vec![
            ok(response),
            ok(json!({"data":[],"padding":"x".repeat(280)})),
        ],
        &[("LLM_MCP_MAX_RESPONSE_BYTES", "400")],
    )
    .await;
    let error = f
        .provider
        .call(Operation::Embed, multimodal(), CancellationToken::new())
        .await
        .unwrap_err();
    assert_eq!(error.code, "response_too_large");
    assert_eq!(f.captured.recv().await.unwrap().0, "/v1/responses");
    assert_eq!(f.captured.recv().await.unwrap().0, "/v1/embeddings");
}

#[tokio::test]
async fn oversized_prepared_text_skips_final_embedding_request() {
    let mut f = fixture(
        vec![ok(description(&"x".repeat(1500)))],
        &[("LLM_MCP_MAX_REQUEST_BYTES", "1024")],
    )
    .await;
    let error = f
        .provider
        .call(Operation::Embed, multimodal(), CancellationToken::new())
        .await
        .unwrap_err();
    assert_eq!(error.code, "request_too_large");
    assert_eq!(f.captured.recv().await.unwrap().0, "/v1/responses");
    assert!(f.captured.try_recv().is_err());
}
