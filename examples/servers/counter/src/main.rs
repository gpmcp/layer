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

async fn start_stdio() -> anyhow::Result<()> {
    let service = Counter::new().serve(stdio()).await.inspect_err(|e| {
        tracing::error!("serving error: {:?}", e);
    })?;

    service.waiting().await?;
    Ok(())
}
