//! Stdio and Streamable HTTP process lifecycle.

use crate::{
    config::Config,
    error::{Error, Result},
    mcp::McpServer,
    provider::Provider,
};
use axum::{Router, http::StatusCode, routing::get};
use rmcp::{
    ServiceExt,
    transport::{
        async_rw::AsyncRwTransport,
        streamable_http_server::{
            StreamableHttpServerConfig, StreamableHttpService, session::local::LocalSessionManager,
        },
    },
};
use std::sync::Arc;
use tokio::net::TcpListener;
use tokio_util::sync::CancellationToken;

/// Run the configured transport until EOF or an operating-system termination signal.
pub async fn serve(config: Config) -> Result<()> {
    let shutdown = CancellationToken::new();
    let signal = shutdown.clone();
    let signals = tokio::spawn(async move {
        termination_signal().await;
        signal.cancel();
    });
    let result = if config.transport == "http" {
        let listener = TcpListener::bind(config.bind)
            .await
            .map_err(|_| Error::new("transport", "cannot bind HTTP listener"))?;
        tracing::info!(address = %listener.local_addr().map_err(|_| Error::new("transport", "cannot obtain HTTP address"))?, "HTTP listener ready");
        serve_http(listener, config, shutdown).await
    } else {
        let limit = config.max_request_bytes.saturating_add(64 * 1024);
        let handler = McpServer::new(Provider::new(config)?);
        let transport = AsyncRwTransport::new_server(tokio::io::stdin(), tokio::io::stdout())
            .with_max_line_length(limit);
        tokio::select! {
            () = shutdown.cancelled() => Ok(()),
            result = async {
                let service = handler.serve(transport).await.map_err(|_| Error::new("transport", "MCP initialization failed"))?;
                let cancellation = service.cancellation_token();
                tokio::select! {
                    () = shutdown.cancelled() => { cancellation.cancel(); Ok(()) },
                    result = service.waiting() => result.map(|_| ()).map_err(|_| Error::new("transport", "MCP service failed")),
                }
            } => result,
        }
    };
    signals.abort();
    result
}

/// Serve HTTP on a pre-bound listener. Sessions hold protocol metadata, never conversations.
pub async fn serve_http(
    listener: TcpListener,
    config: Config,
    shutdown: CancellationToken,
) -> Result<()> {
    let grace = config.timeout + std::time::Duration::from_secs(5);
    let handler = McpServer::new(Provider::new(config.clone())?);
    let sessions = CancellationToken::new();
    let mut http =
        StreamableHttpServerConfig::default().with_allowed_hosts(config.allowed_hosts.clone());
    http.cancellation_token = sessions.clone();
    http.max_request_body_bytes = config.max_request_bytes.saturating_add(64 * 1024);
    http.sse_keep_alive = None;
    http.sse_retry = None;
    // Legacy sessions are required for 2025 cancellation notifications. These are
    // protocol sessions, not stored model conversations. API-key replicas use affinity.
    http.legacy_session_mode = true;
    let mcp = StreamableHttpService::new(
        move || Ok(handler.clone()),
        Arc::new(LocalSessionManager::default()),
        http,
    );
    let readiness = shutdown.clone();
    let app = Router::new()
        .nest_service("/mcp", mcp)
        .route("/health/live", get(|| async { StatusCode::OK }))
        .route(
            "/health/ready",
            get(move || {
                let readiness = readiness.clone();
                async move {
                    if readiness.is_cancelled() {
                        StatusCode::SERVICE_UNAVAILABLE
                    } else {
                        StatusCode::OK
                    }
                }
            }),
        );
    let stopping = shutdown.clone();
    let server = axum::serve(listener, app)
        .with_graceful_shutdown(stopping.cancelled_owned())
        .into_future();
    tokio::pin!(server);
    let result = tokio::select! {
        result = &mut server => result,
        () = shutdown.cancelled() => {
            match tokio::time::timeout(grace, &mut server).await {
                Ok(result) => result,
                Err(_) => { sessions.cancel(); tokio::time::timeout(std::time::Duration::from_secs(5), &mut server).await.map_err(|_| Error::new("shutdown", "HTTP shutdown deadline exceeded"))? }
            }
        }
    };
    sessions.cancel();
    result.map_err(|_| Error::new("transport", "HTTP server failed"))
}

async fn termination_signal() {
    #[cfg(unix)]
    {
        if let Ok(mut term) =
            tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
        {
            tokio::select! { _ = tokio::signal::ctrl_c() => {}, _ = term.recv() => {} }
            return;
        }
    }
    let _ = tokio::signal::ctrl_c().await;
}
