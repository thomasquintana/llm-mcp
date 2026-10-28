//! Audio multipart contracts decoded by an independent server-side parser.

use axum::{
    Router,
    body::Body,
    extract::Multipart,
    http::{HeaderMap, Response, StatusCode},
    routing::post,
};
use base64::{Engine as _, engine::general_purpose::STANDARD};
use llm_mcp::{
    config::Config,
    mcp::McpServer,
    provider::{Operation, Provider},
};
use serde_json::{Value, json};
use std::time::Duration;
use tokio::{net::TcpListener, sync::mpsc};
use tokio_util::sync::CancellationToken;

#[derive(Debug)]
struct Field {
    name: String,
    filename: Option<String>,
    mime: Option<String>,
    bytes: Vec<u8>,
}
struct Fixture {
    provider: Provider,
    captured: mpsc::UnboundedReceiver<Vec<Field>>,
    task: tokio::task::JoinHandle<()>,
}
impl Drop for Fixture {
    fn drop(&mut self) {
        self.task.abort();
    }
}

async fn fixture(
    response: Vec<u8>,
    status: StatusCode,
    delay: Duration,
    settings: &[(&str, &str)],
) -> Fixture {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}/v1/", listener.local_addr().unwrap());
    let (tx, rx) = mpsc::unbounded_channel();
    let app = Router::new().route(
        "/v1/audio/transcriptions",
        post(move |headers: HeaderMap, mut multipart: Multipart| {
            let tx = tx.clone();
            let response = response.clone();
            async move {
                assert_eq!(headers["authorization"], "Bearer test");
                assert_eq!(headers["openai-organization"], "org-test");
                let mut fields = Vec::new();
                while let Some(field) = multipart.next_field().await.unwrap() {
                    let name = field.name().unwrap().to_owned();
                    let filename = field.file_name().map(str::to_owned);
                    let mime = field.content_type().map(str::to_owned);
                    let bytes = field.bytes().await.unwrap().to_vec();
                    fields.push(Field {
                        name,
                        filename,
                        mime,
                        bytes,
                    });
                }
                tx.send(fields).unwrap();
                tokio::time::sleep(delay).await;
                Response::builder()
                    .status(status)
                    .header("x-request-id", "req-transcribe")
                    .body(Body::from(response))
                    .unwrap()
            }
        }),
    );
    let task = tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    let config = Config::from_lookup(|key| {
        if let Some((_, value)) = settings.iter().find(|(name, _)| *name == key) {
            return Some((*value).into());
        }
        match key {
            "OPENAI_API_KEY" => Some("test".into()),
            "OPENAI_BASE_URL" => Some(url.clone()),
            "OPENAI_ORGANIZATION" => Some("org-test".into()),
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
fn request() -> Value {
    json!({"model":"gpt-4o-transcribe", "file":{"filename":"recording.wav", "mime_type":"audio/wav", "data_base64":STANDARD.encode(b"RIFF\0\x01\x80\xffWAVE")}})
}
fn values<'a>(fields: &'a [Field], name: &str) -> Vec<&'a str> {
    fields
        .iter()
        .filter(|field| field.name == name)
        .map(|field| std::str::from_utf8(&field.bytes).unwrap())
        .collect()
}

#[tokio::test]
async fn uploads_exact_audio_and_native_scalar_array_nested_options() {
    let response = json!({"text":"Hello", "usage":{"total_tokens":9}, "future":{"enabled":true}});
    let mut f = fixture(
        response.to_string().into_bytes(),
        StatusCode::OK,
        Duration::ZERO,
        &[],
    )
    .await;
    let mut body = request();
    body.as_object_mut().unwrap().extend(json!({
        "language":"en", "prompt":"Equipment names", "temperature":0,
        "include":["logprobs"], "keywords":["pump","valve"], "languages":["en","es"],
        "timestamp_granularities":["word","segment"],
        "chunking_strategy":{"type":"server_vad","threshold":0.5,"silence_duration_ms":500},
        "known_speaker_names":["agent"], "known_speaker_references":["data:audio/wav;base64,eA=="],
        "future_option":{"enabled":true,"labels":["a","b"]}, "optional":null, "stream":false,"background":false
    }).as_object().unwrap().clone());
    let result = McpServer::new(f.provider.clone())
        .invoke("transcribe", body, CancellationToken::new())
        .await
        .unwrap();
    assert_eq!(result.is_error, Some(false));
    assert_eq!(result.structured_content, Some(response));
    let fields = f.captured.recv().await.unwrap();
    let file = fields.iter().find(|field| field.name == "file").unwrap();
    assert_eq!(file.bytes, b"RIFF\0\x01\x80\xffWAVE");
    assert_eq!(file.filename.as_deref(), Some("recording.wav"));
    assert_eq!(file.mime.as_deref(), Some("audio/wav"));
    for (name, expected) in [
        ("model", vec!["gpt-4o-transcribe"]),
        ("response_format", vec!["json"]),
        ("language", vec!["en"]),
        ("prompt", vec!["Equipment names"]),
        ("temperature", vec!["0"]),
        ("include[]", vec!["logprobs"]),
        ("keywords[]", vec!["pump", "valve"]),
        ("languages[]", vec!["en", "es"]),
        ("timestamp_granularities[]", vec!["word", "segment"]),
        ("chunking_strategy[type]", vec!["server_vad"]),
        ("chunking_strategy[threshold]", vec!["0.5"]),
        ("chunking_strategy[silence_duration_ms]", vec!["500"]),
        ("known_speaker_names[]", vec!["agent"]),
        (
            "known_speaker_references[]",
            vec!["data:audio/wav;base64,eA=="],
        ),
        ("future_option[enabled]", vec!["true"]),
        ("future_option[labels][]", vec!["a", "b"]),
        ("stream", vec![]),
        ("background", vec![]),
        ("optional", vec![]),
    ] {
        assert_eq!(values(&fields, name), expected, "{name}");
    }
}

#[tokio::test]
async fn preserves_json_diarization_timestamps_and_exact_text_subtitles() {
    for format in [
        "json",
        "verbose_json",
        "diarized_json",
        "text",
        "srt",
        "vtt",
    ] {
        let response = json!({"text":"Hello", "segments":[{"speaker":"agent","start":0.1,"end":1.2,"text":"Hello"}], "words":[{"word":"Hello","start":0.1}], "usage":{"seconds":2},"future":true});
        let text = "WEBVTT\r\n\r\n00:00.100 --> 00:01.200\r\nHéllo\r\n";
        let is_json = format.ends_with("json");
        let bytes = if is_json {
            response.to_string().into_bytes()
        } else {
            text.as_bytes().to_vec()
        };
        let f = fixture(bytes, StatusCode::OK, Duration::ZERO, &[]).await;
        let mut body = request();
        body["response_format"] = json!(format);
        let result = f
            .provider
            .call(Operation::Transcribe, body, CancellationToken::new())
            .await
            .unwrap();
        assert_eq!(
            result,
            if is_json {
                response
            } else {
                json!({"text":text,"response_format":format})
            }
        );
    }
}

#[tokio::test]
async fn rejects_invalid_media_and_options_before_upload() {
    let mut f = fixture(vec![], StatusCode::OK, Duration::ZERO, &[]).await;
    let mut requests = Vec::new();
    for file in [
        json!("/tmp/audio.wav"),
        json!({"url":"https://example.com/audio.wav"}),
        json!({"filename":"audio.wav","mime_type":"audio/wav","data_base64":"eA==","path":"private"}),
    ] {
        let mut body = request();
        body["file"] = file;
        requests.push(body);
    }
    for (field, value) in [
        ("data_base64", ""),
        ("data_base64", "invalid-secret"),
        ("data_base64", "data:audio/wav;base64,eA=="),
        ("filename", "../audio.wav"),
        ("filename", "audio"),
        ("filename", "audio.exe"),
        ("mime_type", "image/png"),
    ] {
        let mut body = request();
        body["file"][field] = json!(value);
        requests.push(body);
    }
    for (field, value) in [
        ("stream", json!(true)),
        ("background", json!(true)),
        ("model", json!("")),
        ("response_format", json!("stream")),
        (
            "known_speaker_references",
            json!(["https://example.com/audio.wav"]),
        ),
        (
            "known_speaker_references",
            json!(["data:image/png;base64,eA=="]),
        ),
        ("chunking_strategy", json!({"bad[field]":true})),
    ] {
        let mut body = request();
        body[field] = value;
        requests.push(body);
    }
    for body in requests {
        let error = f
            .provider
            .call(Operation::Transcribe, body, CancellationToken::new())
            .await
            .unwrap_err();
        assert!(
            matches!(error.code.as_str(), "invalid_media" | "invalid_request"),
            "{error}"
        );
        assert!(!error.message.contains("secret"));
    }
    assert!(f.captured.try_recv().is_err());
}

#[tokio::test]
async fn upload_overhead_and_result_sizes_are_bounded() {
    let mut f = fixture(
        b"{}".to_vec(),
        StatusCode::OK,
        Duration::ZERO,
        &[("LLM_MCP_MAX_REQUEST_BYTES", "300")],
    )
    .await;
    let error = f
        .provider
        .call(Operation::Transcribe, request(), CancellationToken::new())
        .await
        .unwrap_err();
    assert_eq!(error.code, "request_too_large");
    assert!(f.captured.try_recv().is_err());
    let f = fixture(
        vec![b'x'; 100],
        StatusCode::OK,
        Duration::ZERO,
        &[("LLM_MCP_MAX_RESPONSE_BYTES", "20")],
    )
    .await;
    let error = f
        .provider
        .call(Operation::Transcribe, request(), CancellationToken::new())
        .await
        .unwrap_err();
    assert_eq!(error.code, "response_too_large");
    assert_eq!(error.request_id.as_deref(), Some("req-transcribe"));
    let f = fixture(
        b"a".to_vec(),
        StatusCode::OK,
        Duration::ZERO,
        &[("LLM_MCP_MAX_RESPONSE_BYTES", "20")],
    )
    .await;
    let mut body = request();
    body["response_format"] = json!("text");
    assert_eq!(
        f.provider
            .call(Operation::Transcribe, body, CancellationToken::new())
            .await
            .unwrap_err()
            .code,
        "response_too_large"
    );
}

#[tokio::test]
async fn provider_failures_are_sanitized_and_not_retried() {
    for (status, bytes, format, code) in [
        (
            StatusCode::TOO_MANY_REQUESTS,
            b"private-secret".to_vec(),
            "json",
            "rate_limited",
        ),
        (
            StatusCode::OK,
            b"{invalid".to_vec(),
            "json",
            "provider_protocol",
        ),
        (StatusCode::OK, b"[]".to_vec(), "json", "provider_protocol"),
        (StatusCode::OK, vec![255], "text", "provider_protocol"),
    ] {
        let mut f = fixture(bytes, status, Duration::ZERO, &[]).await;
        let mut body = request();
        body["response_format"] = json!(format);
        let error = f
            .provider
            .call(Operation::Transcribe, body, CancellationToken::new())
            .await
            .unwrap_err();
        assert_eq!(error.code, code);
        assert_eq!(error.request_id.as_deref(), Some("req-transcribe"));
        assert_eq!(error.status, Some(status.as_u16()));
        assert!(!error.message.contains("secret"));
        assert!(f.captured.recv().await.is_some());
        assert!(f.captured.try_recv().is_err());
    }
}

#[tokio::test]
async fn cancellation_and_deadline_abort_transcription_and_release_capacity() {
    let mut f = fixture(
        b"{\"text\":\"late\"}".to_vec(),
        StatusCode::OK,
        Duration::from_millis(100),
        &[("LLM_MCP_MAX_CONCURRENCY", "1")],
    )
    .await;
    let cancel = CancellationToken::new();
    let task = {
        let provider = f.provider.clone();
        let ct = cancel.clone();
        tokio::spawn(async move { provider.call(Operation::Transcribe, request(), ct).await })
    };
    f.captured.recv().await.unwrap();
    cancel.cancel();
    assert_eq!(task.await.unwrap().unwrap_err().code, "cancelled");
    let result = tokio::time::timeout(
        Duration::from_secs(1),
        f.provider
            .call(Operation::Transcribe, request(), CancellationToken::new()),
    )
    .await
    .unwrap()
    .unwrap();
    assert_eq!(result["text"], "late");
    let f = fixture(
        b"{}".to_vec(),
        StatusCode::OK,
        Duration::from_millis(100),
        &[("LLM_MCP_TIMEOUT_MS", "20")],
    )
    .await;
    assert_eq!(
        f.provider
            .call(Operation::Transcribe, request(), CancellationToken::new())
            .await
            .unwrap_err()
            .code,
        "timeout"
    );
}
