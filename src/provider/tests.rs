use super::*;
use std::sync::atomic::{AtomicUsize, Ordering};

#[tokio::test]
async fn http2_nacks_are_not_retried_when_sdk_features_unify() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}/v1/", listener.local_addr().unwrap());
    let count = Arc::new(AtomicUsize::new(0));
    let counter = count.clone();
    let server = tokio::spawn(async move {
        loop {
            let (socket, _) = listener.accept().await.unwrap();
            let counter = counter.clone();
            tokio::spawn(async move {
                let mut connection = h2::server::handshake(socket).await.unwrap();
                while let Some(Ok((_request, mut response))) = connection.accept().await {
                    counter.fetch_add(1, Ordering::SeqCst);
                    response.send_reset(h2::Reason::REFUSED_STREAM);
                }
            });
        }
    });
    let config = Config::from_lookup(|key| match key {
        "OPENAI_BASE_URL" => Some(url.clone()),
        "OPENAI_API_KEY" => Some("test".into()),
        "LLM_MCP_TIMEOUT_MS" => Some("1000".into()),
        _ => None,
    })
    .unwrap();
    let mut provider = Provider::new(config).unwrap();
    provider.client = client_builder().http2_prior_knowledge().build().unwrap();
    assert!(
        provider
            .call(
                Operation::Generate,
                serde_json::json!({}),
                CancellationToken::new()
            )
            .await
            .is_err()
    );
    assert_eq!(count.load(Ordering::SeqCst), 1);
    server.abort();
}
