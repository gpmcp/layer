use crate::error::GpmcpError;
use crate::{RunnerConfig, Transport};
use anyhow::Result;
use backon::{BackoffBuilder, ExponentialBuilder, Retryable};
use rmcp::ServiceExt;
use rmcp::model::ClientInfo;
use rmcp::transport::{SseClientTransport, TokioChildProcess};
use std::time::Duration;
use tokio::process::Command;
use tracing::info;

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
    pub async fn new(runner_config: &RunnerConfig) -> Result<Self, GpmcpError> {
        let transport = match &runner_config.transport {
            Transport::Stdio => {
                info!("Creating stdio transport");
                Self::create_stdio_transport(runner_config.clone()).await?
            }
            Transport::Sse { url } => {
                info!(url=%url, "Creating SSE transport for URL");
                Self::create_sse_transport(url).await?
            }
        };

        Ok(Self { transport })
    }

    /// Creates a stdio transport using TokioChildProcess
    async fn create_stdio_transport(
        runner_config: RunnerConfig,
    ) -> Result<TransportVariant, GpmcpError> {
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

        let transport = TokioChildProcess::new(cmd)?;

        info!("Stdio transport created successfully");
        Ok(TransportVariant::Stdio(transport))
    }

    /// Creates an SSE transport with server readiness polling
    async fn create_sse_transport(url: impl ToString) -> Result<TransportVariant, GpmcpError> {
        let url_string = url.to_string();

        // Poll the server to check if it's ready using list_tools request
        // Use shorter timeout for faster failure in test environments
        Self::poll_server_readiness(&url_string, 10, 1000).await?;

        // Create SSE transport
        // TODO: Add varient in GpmcpError and use that.
        let transport = SseClientTransport::start(url_string).await?;

        info!("SSE transport created successfully"); 
        Ok(TransportVariant::Sse(transport))
    }

    /// Poll the server readiness by attempting to connect and call list_tools
    async fn poll_server_readiness(
        url: &str,
        max_attempts: u32,
        interval_ms: u64,
    ) -> Result<(), GpmcpError> {
        info!(
            "Polling server readiness at {} (max {} attempts, {}ms interval)",
            url, max_attempts, interval_ms
        );

        let poll = ExponentialBuilder::new()
            .with_jitter()
            .with_factor(1.0)
            .with_max_times(max_attempts as usize)
            .with_min_delay(Duration::from_millis(interval_ms))
            .with_max_delay(Duration::from_secs(1))
            .build();

        (|| Self::test_server_connectivity(url)).retry(poll).await
    }

    /// Test server connectivity by creating a temporary connection and calling list_tools
    async fn test_server_connectivity(url: &str) -> Result<(), GpmcpError> {
        // Create a temporary SSE transport for testing
        let test_transport = SseClientTransport::start(url.to_string()).await?;

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
        let service = client_info.serve(test_transport).await?;

        // Call list_tools to verify server is responding
        let _result = service.list_tools(Default::default()).await?;

        // Cancel the test service
        service.cancel().await?;
        
        info!("Server is ready and responding to list_tools request");
        Ok(())
    }

    /// Consumes the manager and returns the transport for service creation
    pub fn into_transport(self) -> TransportVariant {
        self.transport
    }
}
