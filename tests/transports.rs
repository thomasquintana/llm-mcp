//! Independent JSON-RPC wire tests, without RMCP's client implementation.

use llm_mcp::{config::Config, mcp, runtime};
use serde_json::{Value, json};
use std::{process::Stdio, time::Duration};
use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader},
    net::TcpListener,
    process::Command,
};
use tokio_util::sync::CancellationToken;

fn initialize() -> Value {
    json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-11-25","capabilities":{},"clientInfo":{"name":"wire-tests","version":"1"}}})
}

async fn decode_http(response: reqwest::Response) -> Value {
    assert!(response.status().is_success(), "{}", response.status());
    let text = response.text().await.unwrap();
    if let Ok(value) = serde_json::from_str(&text) {
        return value;
    }
    text.lines()
        .filter_map(|line| line.strip_prefix("data:"))
        .find_map(|data| serde_json::from_str(data.trim()).ok())
        .unwrap_or_else(|| panic!("no JSON-RPC response in {text}"))
}

#[tokio::test]
async fn stdio_initializes_lists_calls_without_stdout_noise() {
    let mut child = Command::new(env!("CARGO_BIN_EXE_llm-mcp"))
        .arg("serve")
        .env("LLM_MCP_TRANSPORT", "stdio")
        .env("OPENAI_API_KEY", "test")
        .env("OPENAI_BASE_URL", "http://127.0.0.1:1/v1/")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .kill_on_drop(true)
        .spawn()
        .unwrap();
    let mut input = child.stdin.take().unwrap();
    let mut output = BufReader::new(child.stdout.take().unwrap()).lines();
    input
        .write_all(format!("{}\n", initialize()).as_bytes())
        .await
        .unwrap();
    let response: Value = serde_json::from_str(
        &tokio::time::timeout(Duration::from_secs(5), output.next_line())
            .await
            .unwrap()
            .unwrap()
            .unwrap(),
    )
    .unwrap();
    assert_eq!(response["result"]["protocolVersion"], "2025-11-25");
    assert_eq!(response["result"]["serverInfo"]["name"], "llm-mcp");
    for request in [
        json!({"jsonrpc":"2.0","method":"notifications/initialized"}),
        json!({"jsonrpc":"2.0","id":2,"method":"tools/list"}),
        json!({"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"generate","arguments":{"background":true}}}),
    ] {
        input
            .write_all(format!("{request}\n").as_bytes())
            .await
            .unwrap();
    }
    let mut found = 0;
    while found < 2 {
        let response: Value = serde_json::from_str(
            &tokio::time::timeout(Duration::from_secs(5), output.next_line())
                .await
                .unwrap()
                .unwrap()
                .unwrap(),
        )
        .unwrap();
        if response["id"] == 2 {
            assert_eq!(
                response["result"]["tools"],
                serde_json::to_value(mcp::tools()).unwrap()
            );
            found += 1;
        }
        if response["id"] == 3 {
            assert_eq!(response["result"]["isError"], true);
            assert_eq!(
                response["result"]["structuredContent"]["error"]["code"],
                "invalid_request"
            );
            found += 1;
        }
    }
    drop(input);
    assert!(
        tokio::time::timeout(Duration::from_secs(5), child.wait())
            .await
            .unwrap()
            .unwrap()
            .success()
    );
}

#[tokio::test]
async fn http_cancellation_releases_provider_capacity() {
    use axum::{Json, Router, routing::post};
    let (tx, mut rx) = tokio::sync::mpsc::channel(1);
    let provider_listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let provider_url = format!("http://{}/v1/", provider_listener.local_addr().unwrap());
    let mock = tokio::spawn(async move {
        axum::serve(
            provider_listener,
            Router::new().route(
                "/v1/responses",
                post(move |Json(body): Json<Value>| {
                    let tx = tx.clone();
                    async move {
                        if body["input"] == "slow" {
                            let _ = tx.send(()).await;
                            tokio::time::sleep(Duration::from_secs(3)).await;
                        }
                        Json(json!({"status":"completed","output":[]}))
                    }
                }),
            ),
        )
        .await
        .unwrap();
    });
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}/mcp", listener.local_addr().unwrap());
    let config = Config::from_lookup(|k| match k {
        "OPENAI_API_KEY" => Some("test".into()),
        "OPENAI_BASE_URL" => Some(provider_url.clone()),
        "LLM_MCP_MAX_CONCURRENCY" => Some("1".into()),
        _ => None,
    })
    .unwrap();
    let shutdown = CancellationToken::new();
    let stop = shutdown.clone();
    let task = tokio::spawn(async move {
        runtime::serve_http(listener, config, stop).await.unwrap();
    });
    let client = reqwest::Client::new();
    let response = client
        .post(&url)
        .header("accept", "application/json, text/event-stream")
        .json(&initialize())
        .send()
        .await
        .unwrap();
    let session = response.headers()["mcp-session-id"]
        .to_str()
        .unwrap()
        .to_owned();
    let _ = decode_http(response).await;
    let post = |body: Value| {
        client
            .post(&url)
            .header("accept", "application/json, text/event-stream")
            .header("mcp-protocol-version", "2025-11-25")
            .header("mcp-session-id", &session)
            .json(&body)
    };
    post(json!({"jsonrpc":"2.0","method":"notifications/initialized"}))
        .send()
        .await
        .unwrap();
    let slow=post(json!({"jsonrpc":"2.0","id":10,"method":"tools/call","params":{"name":"generate","arguments":{"input":"slow"}}})).send().await.unwrap();
    let reader = tokio::spawn(async move { slow.text().await });
    tokio::time::timeout(Duration::from_secs(5), rx.recv())
        .await
        .unwrap()
        .unwrap();
    post(json!({"jsonrpc":"2.0","method":"notifications/cancelled","params":{"requestId":10,"reason":"test"}})).send().await.unwrap();
    let fast = post(
        json!({"jsonrpc":"2.0","id":11,"method":"tools/call","params":{"name":"generate","arguments":{"input":"fast"}}}),
    );
    let response = tokio::time::timeout(Duration::from_secs(1), async {
        decode_http(fast.send().await.unwrap()).await
    })
    .await
    .expect("cancelled request must release its semaphore slot");
    assert_eq!(
        response["result"]["structuredContent"]["status"],
        "completed"
    );
    client
        .delete(&url)
        .header("mcp-session-id", &session)
        .send()
        .await
        .unwrap();
    reader.abort();
    shutdown.cancel();
    tokio::time::timeout(Duration::from_secs(5), task)
        .await
        .unwrap()
        .unwrap();
    mock.abort();
}

#[tokio::test]
async fn http_session_health_calls_and_shutdown() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let config = Config::from_lookup(|k| match k {
        "OPENAI_API_KEY" => Some("test".into()),
        "OPENAI_BASE_URL" => Some("http://127.0.0.1:1/v1/".into()),
        _ => None,
    })
    .unwrap();
    let shutdown = CancellationToken::new();
    let stop = shutdown.clone();
    let task = tokio::spawn(async move {
        runtime::serve_http(listener, config, stop).await.unwrap();
    });
    let client = reqwest::Client::new();
    let url = format!("{base}/mcp");
    assert_eq!(
        client
            .get(format!("{base}/health/ready"))
            .send()
            .await
            .unwrap()
            .status(),
        200
    );
    assert_eq!(
        client
            .get(format!("{base}/health/live"))
            .send()
            .await
            .unwrap()
            .status(),
        200
    );
    let response = client
        .post(&url)
        .header("accept", "application/json, text/event-stream")
        .json(&initialize())
        .send()
        .await
        .unwrap();
    let session = response
        .headers()
        .get("mcp-session-id")
        .unwrap()
        .to_str()
        .unwrap()
        .to_owned();
    assert_eq!(
        decode_http(response).await["result"]["protocolVersion"],
        "2025-11-25"
    );
    let post = |request: Value| {
        client
            .post(&url)
            .header("accept", "application/json, text/event-stream")
            .header("mcp-protocol-version", "2025-11-25")
            .header("mcp-session-id", &session)
            .json(&request)
    };
    assert!(
        post(json!({"jsonrpc":"2.0","method":"notifications/initialized"}))
            .send()
            .await
            .unwrap()
            .status()
            .is_success()
    );
    let response = decode_http(
        post(json!({"jsonrpc":"2.0","id":2,"method":"tools/list"}))
            .send()
            .await
            .unwrap(),
    )
    .await;
    assert_eq!(
        response["result"]["tools"],
        serde_json::to_value(mcp::tools()).unwrap()
    );
    let response=decode_http(post(json!({"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"classify","arguments":{"stream":true}}})).send().await.unwrap()).await;
    assert_eq!(response["result"]["isError"], true);
    let response = client
        .delete(&url)
        .header("mcp-session-id", &session)
        .header("mcp-protocol-version", "2025-11-25")
        .send()
        .await
        .unwrap();
    assert!(response.status().is_success());
    shutdown.cancel();
    tokio::time::timeout(Duration::from_secs(5), task)
        .await
        .unwrap()
        .unwrap();
}
