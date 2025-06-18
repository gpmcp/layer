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

    /// Creates an SSE transport with server readiness polling
    async fn create_sse_transport(url: impl ToString) -> Result<TransportVariant> {
        let url_string = url.to_string();
        
        // Poll the server to check if it's ready using list_tools request
        // Use shorter timeout for faster failure in test environments
        Self::poll_server_readiness(&url_string, 10, 100).await?;

        // Create SSE transport
        let transport = SseClientTransport::start(url_string)
            .await
            .context("Failed to create SSE transport")?;

        Ok(TransportVariant::Sse(transport))
    }

    /// Poll the server readiness by attempting to connect and call list_tools
    async fn poll_server_readiness(url: &str, max_attempts: u32, interval_ms: u64) -> Result<()> {
        info!("Polling server readiness at {} (max {} attempts, {}ms interval)", url, max_attempts, interval_ms);
        
        for attempt in 1..=max_attempts {
            // Try to create a temporary transport and test connectivity
            match Self::test_server_connectivity(url).await {
                Ok(()) => {
                    info!("Server is ready after {} attempts", attempt);
                    return Ok(());
                }
                Err(e) => {
                    if attempt == max_attempts {
                        return Err(anyhow::anyhow!(
                            "Server not ready after {} attempts. Last error: {}",
                            max_attempts,
                            e
                        ));
                    }
                    // Wait before next attempt
                    tokio::time::sleep(tokio::time::Duration::from_millis(interval_ms)).await;
                }
            }
        }
        
        Err(anyhow::anyhow!("Server polling failed unexpectedly"))
    }
    
    /// Test server connectivity by creating a temporary connection and calling list_tools
    async fn test_server_connectivity(url: &str) -> Result<()> {
        use rmcp::model::ClientInfo;
        use rmcp::ServiceExt;
        
        // Create a temporary SSE transport for testing
        let test_transport = SseClientTransport::start(url.to_string())
            .await
            .context("Failed to create test SSE transport")?;
        
        // Create minimal client info for testing
        let client_info = ClientInfo {
            protocol_version: rmcp::model::ProtocolVersion::default(),
            capabilities: rmcp::model::ClientCapabilities::default(),
            client_info: rmcp::model::Implementation {
                name: "gpmcp-readiness-test".to_string(),
                version: "0.1.0".to_string(),
            },
        };
        
        // Try to establish service and call list_tools
        let service = client_info
            .serve(test_transport)
            .await
            .context("Failed to create test service")?;
            
        // Call list_tools to verify server is responding
        let _result = service
            .list_tools(Default::default())
            .await
            .context("Server not responding to list_tools")?;
            
        // Cancel the test service
        service
            .cancel()
            .await
            .context("Failed to cancel test service")?;
            
        Ok(())
    }

    /// Consumes the manager and returns the transport for service creation
    pub fn into_transport(self) -> TransportVariant {
        self.transport
    }
}
