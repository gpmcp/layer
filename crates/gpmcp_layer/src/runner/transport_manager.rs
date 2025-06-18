use crate::{RunnerConfig, Transport};
use anyhow::{Context, Result};
use rmcp::transport::{SseClientTransport, TokioChildProcess};
use tokio::process::Command;
use tracing::{info, warn};

/// TransportManager handles the creation and management of different transport types
pub struct TransportManager {
    transport: TransportVariant,
}

pub enum TransportVariant {
    Stdio(TokioChildProcess),
    Sse(SseClientTransport<reqwest::Client>),
}

impl TransportManager {
    /// Creates a new TransportManager
    pub async fn new(runner_config: &RunnerConfig) -> Result<Self> {
        let transport = match &runner_config.transport {
            Transport::Stdio => {
                info!("Creating stdio transport");
                Self::create_stdio_transport(runner_config.clone()).await?
            }
            Transport::Sse { url } => {
                info!("Creating SSE transport for URL: {}", url);
                Self::create_sse_transport(url).await?
            }
        };

        Ok(Self { transport })
    }

    /// Creates a stdio transport using TokioChildProcess
    async fn create_stdio_transport(runner_config: RunnerConfig) -> Result<TransportVariant> {
        // Create the command
        let mut cmd = Command::new(&runner_config.command);
        cmd.args(&runner_config.args);

        // Set working directory if specified
        if let Some(workdir) = &runner_config.working_directory {
            cmd.current_dir(workdir);
        }

        // Add environment variables
        for (key, value) in &runner_config.env {
            cmd.env(key, value);
        }

        // Try to get PID information before creating transport
        info!(
            "Creating STDIO transport for command: {} with args: {:?}",
            runner_config.command, runner_config.args
        );

        let transport =
            TokioChildProcess::new(cmd).context("Failed to create TokioChildProcess")?;

        // Note: TokioChildProcess doesn't expose PID directly
        warn!(
            "STDIO transport created for command: {} - PID not accessible through TokioChildProcess interface",
            runner_config.command
        );

        Ok(TransportVariant::Stdio(transport))
    }

    /// Creates an SSE transport
    async fn create_sse_transport(url: impl ToString) -> Result<TransportVariant> {
        // Wait a bit for the server to start up (if process manager started it)
        // TODO: Poll List Tools req to check if server is ready.
        tokio::time::sleep(tokio::time::Duration::from_millis(1000)).await;

        // Create SSE transport
        let transport = SseClientTransport::start(url.to_string())
            .await
            .context("Failed to create SSE transport")?;

        Ok(TransportVariant::Sse(transport))
    }

    /// Consumes the manager and returns the transport for service creation
    pub fn into_transport(self) -> TransportVariant {
        self.transport
    }
}
