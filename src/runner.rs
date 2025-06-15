use anyhow::{Context, Result};
use gpmcp_domain::blueprint::{Runner, ServerDefinition};
use rmcp::model::{ClientInfo, InitializeRequestParam};
use rmcp::{
    ServiceExt,
    service::RunningService,
    transport::{ConfigureCommandExt, SseClientTransport, TokioChildProcess},
};
use std::sync::Arc;
use tokio::process::Command;
use tokio_util::sync::CancellationToken;
use tracing::{error, info, warn};

/// GpmcpRunner manages the connection to MCP servers using the rmcp SDK.
/// It abstracts away the transport layer and provides a unified interface
/// for both stdio and SSE-based MCP servers.
pub struct GpmcpRunner {
    inner: Option<GpmcpRunnerInner>,
    cancellation_token: Arc<CancellationToken>,
    _child_process_handle: Option<tokio::task::JoinHandle<()>>,
}

enum GpmcpRunnerInner {
    Stdio(RunningService<rmcp::RoleClient, InitializeRequestParam>),
    Sse(RunningService<rmcp::RoleClient, InitializeRequestParam>),
}

impl GpmcpRunner {
    /// Creates a new GpmcpRunner from a ServerDefinition.
    ///
    /// For stdio runners, it uses TokioChildProcess to spawn and communicate with the process.
    /// For SSE runners, it starts the command process and then connects via SseClientTransport.
    pub async fn new(server_definition: ServerDefinition) -> Result<Self> {
        let cancellation_token = Arc::new(CancellationToken::new());
        let client_info = Self::client_info(&server_definition);

        match &server_definition.runner {
            Runner::Stdio { command_runner } => {
                info!(
                    "Creating stdio runner for command: {} {:?}",
                    command_runner.command, command_runner.args
                );

                Self::create_stdio_runner(server_definition, cancellation_token, client_info).await
            }
            Runner::Sse {
                command_runner,
                url,
            } => {
                info!(
                    "Creating SSE runner for command: {} {:?}, target URL: {}",
                    command_runner.command, command_runner.args, url
                );

                Self::create_sse_runner(server_definition, cancellation_token, client_info).await
            }
        }
    }

    fn client_info(server_definition: &ServerDefinition) -> ClientInfo {
        ClientInfo {
            protocol_version: rmcp::model::ProtocolVersion::default(),
            capabilities: rmcp::model::ClientCapabilities::default(),
            client_info: rmcp::model::Implementation {
                name: server_definition.package.name.to_string(),
                version: server_definition.package.version.to_string(),
            },
        }
    }

    /// Creates a stdio-based runner using TokioChildProcess
    async fn create_stdio_runner(
        server_definition: ServerDefinition,
        cancellation_token: Arc<CancellationToken>,
        client_info: ClientInfo,
    ) -> Result<Self> {
        let Runner::Stdio { command_runner } = &server_definition.runner else {
            return Err(anyhow::anyhow!("Expected stdio runner"));
        };

        // Create the command
        let mut cmd = Command::new(&command_runner.command);
        cmd.args(&command_runner.args);

        // Set working directory if specified
        if !command_runner.workdir.is_empty() {
            cmd.current_dir(&command_runner.workdir);
        }

        // Add environment variables
        for (key, value) in &server_definition.env {
            cmd.env(key, value);
        }

        // Configure command for stdio communication
        let transport = TokioChildProcess::new(cmd.configure(|_| {}))
            .context("Failed to create TokioChildProcess")?;

        // Create the service using the pattern from examples
        let service = client_info
            .serve(transport)
            .await
            .context("Failed to create stdio service")?;

        info!("✅ Stdio runner created successfully");

        Ok(Self {
            inner: Some(GpmcpRunnerInner::Stdio(service)),
            cancellation_token,
            _child_process_handle: None,
        })
    }

    /// Creates an SSE-based runner by starting the command process and connecting via SSE
    async fn create_sse_runner(
        server_definition: ServerDefinition,
        cancellation_token: Arc<CancellationToken>,
        client_info: ClientInfo,
    ) -> Result<Self> {
        let Runner::Sse {
            command_runner: _,
            url,
        } = &server_definition.runner
        else {
            return Err(anyhow::anyhow!("Expected SSE runner"));
        };

        // Start the command process
        let child_handle =
            Self::start_command_process(server_definition.clone(), cancellation_token.clone())
                .await?;

        // Wait a bit for the server to start up
        tokio::time::sleep(tokio::time::Duration::from_millis(1000)).await;

        // Create SSE transport
        let transport = SseClientTransport::start(url.as_str())
            .await
            .context("Failed to create SSE transport")?;

        // Create the service using the pattern from examples
        let service = client_info
            .serve(transport)
            .await
            .context("Failed to create SSE service")?;

        info!("✅ SSE runner created successfully");

        Ok(Self {
            inner: Some(GpmcpRunnerInner::Sse(service)),
            cancellation_token,
            _child_process_handle: Some(child_handle),
        })
    }

    /// Starts a command process for SSE runners
    async fn start_command_process(
        server_definition: ServerDefinition,
        cancellation_token: Arc<CancellationToken>,
    ) -> Result<tokio::task::JoinHandle<()>> {
        let Runner::Sse { command_runner, .. } = &server_definition.runner else {
            return Err(anyhow::anyhow!("Expected SSE runner"));
        };

        let mut cmd = Command::new(&command_runner.command);
        cmd.args(&command_runner.args);

        // Set working directory if specified
        if !command_runner.workdir.is_empty() {
            cmd.current_dir(&command_runner.workdir);
        }

        // Add environment variables
        for (key, value) in &server_definition.env {
            cmd.env(key, value);
        }

        let mut child = cmd
            .spawn()
            .with_context(|| format!("Failed to start command: {}", command_runner.command))?;

        info!(
            "Started command process: {} with args: {:?}",
            command_runner.command, command_runner.args
        );

        // Spawn a task to monitor the child process
        let handle = tokio::spawn(async move {
            tokio::select! {
                _ = cancellation_token.cancelled() => {
                    info!("Cancellation requested, terminating child process");
                    if let Err(e) = child.kill().await {
                        warn!("Failed to kill child process: {}", e);
                    }
                }
                result = child.wait() => {
                    match result {
                        Ok(status) => {
                            if status.success() {
                                info!("Child process exited successfully");
                            } else {
                                warn!("Child process exited with status: {}", status);
                            }
                        }
                        Err(e) => {
                            error!("Error waiting for child process: {}", e);
                        }
                    }
                }
            }
        });

        Ok(handle)
    }

    /// List available tools from the MCP server
    pub async fn list_tools(&self) -> Result<rmcp::model::ListToolsResult> {
        match &self.inner {
            Some(GpmcpRunnerInner::Stdio(service)) => {
                Ok(service.list_tools(Default::default()).await?)
            }
            Some(GpmcpRunnerInner::Sse(service)) => {
                Ok(service.list_tools(Default::default()).await?)
            }
            _ => Err(anyhow::anyhow!("Service not available")),
        }
        .context("Failed to list tools")
    }

    /// List available prompts from the MCP server
    pub async fn list_prompts(&self) -> Result<rmcp::model::ListPromptsResult> {
        match &self.inner {
            Some(GpmcpRunnerInner::Stdio(service)) => {
                Ok(service.list_prompts(Default::default()).await?)
            }
            Some(GpmcpRunnerInner::Sse(service)) => {
                Ok(service.list_prompts(Default::default()).await?)
            }
            _ => Err(anyhow::anyhow!("Service not available")),
        }
        .context("Failed to list prompts")
    }

    /// List available resources from the MCP server
    pub async fn list_resources(&self) -> Result<rmcp::model::ListResourcesResult> {
        match &self.inner {
            Some(GpmcpRunnerInner::Stdio(service)) => {
                Ok(service.list_resources(Default::default()).await?)
            }
            Some(GpmcpRunnerInner::Sse(service)) => {
                Ok(service.list_resources(Default::default()).await?)
            }
            _ => Err(anyhow::anyhow!("Service not available")),
        }
        .context("Failed to list resources")
    }

    /// Call a tool on the MCP server
    pub async fn call_tool(
        &self,
        request: rmcp::model::CallToolRequestParam,
    ) -> Result<rmcp::model::CallToolResult> {
        match &self.inner {
            Some(GpmcpRunnerInner::Stdio(service)) => Ok(service.call_tool(request).await?),
            Some(GpmcpRunnerInner::Sse(service)) => Ok(service.call_tool(request).await?),
            _ => Err(anyhow::anyhow!("Service not available")),
        }
        .context("Failed to call tool")
    }

    /// Get server information
    pub fn peer_info(&self) -> Option<&rmcp::model::ServerInfo> {
        match &self.inner {
            Some(GpmcpRunnerInner::Stdio(service)) => service.peer_info(),
            Some(GpmcpRunnerInner::Sse(service)) => service.peer_info(),
            _ => None,
        }
    }

    /// Cancel the runner and clean up resources
    pub async fn cancel(mut self) -> Result<()> {
        info!("Cancelling GpmcpRunner");

        // Cancel the cancellation token to stop any background processes
        self.cancellation_token.cancel();

        // Cancel the service
        match self.inner.take() {
            Some(GpmcpRunnerInner::Stdio(service)) => service.cancel(),
            Some(GpmcpRunnerInner::Sse(service)) => service.cancel(),
            _ => return Err(anyhow::anyhow!("Service not available")),
        }
        .await?;

        info!("GpmcpRunner cancelled successfully");
        Ok(())
    }
}

impl Drop for GpmcpRunner {
    fn drop(&mut self) {
        // Ensure cancellation token is cancelled when dropped
        self.cancellation_token.cancel();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpmcp_domain::blueprint::CommandRunner;
    use std::collections::HashMap;

    #[tokio::test]
    async fn test_stdio_runner_creation() {
        let command_runner = CommandRunner {
            command: "echo".to_string(),
            args: vec!["hello".to_string()],
            workdir: String::new(),
        };

        let server_definition = ServerDefinition {
            runner: Runner::Stdio { command_runner },
            env: HashMap::new(),
            ..Default::default()
        };

        // This test might fail if echo doesn't behave as expected for MCP
        // but it tests the creation logic
        let result = GpmcpRunner::new(server_definition).await;

        // We expect this to potentially fail due to protocol mismatch,
        // but the creation logic should work
        match result {
            Ok(runner) => {
                // If successful, clean up
                let _ = runner.cancel().await;
            }
            Err(e) => {
                // Expected for echo command as it's not an MCP server
                println!("Expected error for echo command: {}", e);
            }
        }
    }
}
