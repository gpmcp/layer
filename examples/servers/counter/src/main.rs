use crate::counter::Counter;
use rmcp::ServiceExt;
use rmcp::transport::stdio;
use std::process::exit;

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
        Ok("sse") => todo!(),
        Ok("stdio") => Ok(start_stdio().await?),
        _ => {
            println!("TRANSPORT env var not set");
            exit(1);
        }
    }
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
