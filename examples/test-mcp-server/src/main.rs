use crate::counter::Counter;
use rmcp::ServiceExt;
use rmcp::transport::sse_server::SseServerConfig;
use rmcp::transport::{SseServer, stdio};

mod counter;

fn main() -> anyhow::Result<()> {
    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()?;

    rt.block_on(start())
}

/// Determines which transport to use based on the TRANSPORT environment variable.
/// Starts the appropriate server or exits if not set.
async fn start() -> anyhow::Result<()> {
    match std::env::var("TRANSPORT")
        .map(|v| v.to_ascii_lowercase())
        .as_deref()
    {
        Ok("sse") => Ok(start_sse().await?),
        Ok("stdio") => Ok(start_stdio().await?),
        _ => {
            panic!("TRANSPORT environment variable must be set to either 'sse' or 'stdio'");
        }
    }
}

async fn start_sse() -> anyhow::Result<()> {
    let config = SseServerConfig {
        bind: std::env::var("PORT")
            .map(|v| format!("0.0.0.0:{v}"))?
            .parse()?,
        sse_path: "/sse".to_string(),
        post_path: "/message".to_string(),
        ct: tokio_util::sync::CancellationToken::new(),
        sse_keep_alive: None,
    };

    let (sse_server, router) = SseServer::new(config);

    let listener = tokio::net::TcpListener::bind(sse_server.config.bind).await?;

    let ct = sse_server.config.ct.child_token();

    let server = axum::serve(listener, router).with_graceful_shutdown(async move {
        ct.cancelled().await;
        tracing::info!("sse server cancelled");
    });

    tokio::spawn(async move {
        if let Err(e) = server.await {
            tracing::error!(error = %e, "sse server shutdown with error");
        }
    });

    let ct = sse_server.with_service(Counter::new);

    tokio::signal::ctrl_c().await?;
    ct.cancel();
    Ok(())
}

/// Starts the server using stdio transport.
/// Logs errors if serving fails and waits for the service to finish.
async fn start_stdio() -> anyhow::Result<()> {
    let service = Counter::new().serve(stdio()).await.inspect_err(|e| {
        tracing::error!("serving error: {:?}", e);
    })?;

    service.waiting().await?;
    Ok(())
}
