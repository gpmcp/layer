use crate::RunnerConfig;
use crate::GpmcpError;
use crate::runner::process_manager::ProcessManager;
use crate::runner::service_coordinator::ServiceCoordinator;
use crate::runner::transport_manager::TransportManager;
use anyhow::Context;
use std::sync::Arc;
use tokio::sync::RwLock;
use tokio_util::sync::CancellationToken;
use tracing::info;

pub struct GpmcpRunnerInner {
    cancellation_token: Arc<CancellationToken>,
    runner_config: RunnerConfig,
    service_coordinator: Arc<RwLock<Option<ServiceCoordinator>>>,
    process_manager: Arc<RwLock<Option<ProcessManager>>>,
}
impl GpmcpRunnerInner {
    pub fn new(runner_config: RunnerConfig) -> Self {
        Self {
            cancellation_token: Arc::new(CancellationToken::new()),
            runner_config,
            service_coordinator: Arc::new(RwLock::new(None)),
            process_manager: Arc::new(RwLock::new(None)),
        }
    }
    pub async fn connect(&self) -> anyhow::Result<()> {
        info!(
            "Creating GpmcpRunner for server: {}",
            self.runner_config.name
        );

        // Determine transport type and create appropriate managers

        // Create process manager if needed (for commands that need subprocess)
        let process_manager =
            ProcessManager::new(&self.runner_config).await?;

        // For SSE transport, start the server process first
        if matches!(self.runner_config.transport, crate::Transport::Sse { .. }) {
            info!("Starting server process for SSE transport");
            let _handle = process_manager.start_server().await?;
        }

        // Create transport manager
        let transport_manager = TransportManager::new(&self.runner_config).await?;

        // Create service coordinator
        let service_coordinator =
            ServiceCoordinator::new(transport_manager, &self.runner_config).await?;
        self.service_coordinator
            .write()
            .await
            .replace(service_coordinator);
        self.process_manager.write().await.replace(process_manager);

        Ok(())
    }
}

impl GpmcpRunnerInner {
    /// List available tools from the MCP server
    pub async fn list_tools(&self) -> anyhow::Result<rmcp::model::ListToolsResult, GpmcpError> {
        if let Some(ref coordinator) = *self.service_coordinator.read().await {
            coordinator.list_tools().await.map_err(GpmcpError::Other)
        } else {
            Err(GpmcpError::ServiceNotFound)
        }
    }

    /// Call a tool on the MCP server
    pub async fn call_tool(
        &self,
        request: rmcp::model::CallToolRequestParam,
    ) -> anyhow::Result<rmcp::model::CallToolResult, GpmcpError> {
        if let Some(ref coordinator) = *self.service_coordinator.read().await {
            coordinator
                .call_tool(request)
                .await
                .map_err(GpmcpError::Other)
        } else {
            Err(GpmcpError::ServiceNotFound)
        }
    }

    /// List available prompts from the MCP server
    pub async fn list_prompts(&self) -> anyhow::Result<rmcp::model::ListPromptsResult, GpmcpError> {
        if let Some(ref coordinator) = *self.service_coordinator.read().await {
            coordinator.list_prompts().await.map_err(GpmcpError::Other)
        } else {
            Err(GpmcpError::ServiceNotFound)
        }
    }

    /// List available resources from the MCP server
    pub async fn list_resources(
        &self,
    ) -> anyhow::Result<rmcp::model::ListResourcesResult, GpmcpError> {
        if let Some(ref coordinator) = *self.service_coordinator.read().await {
            coordinator
                .list_resources()
                .await
                .map_err(GpmcpError::Other)
        } else {
            Err(GpmcpError::ServiceNotFound)
        }
    }

    /// Get a prompt from the MCP server
    pub async fn get_prompt(
        &self,
        request: rmcp::model::GetPromptRequestParam,
    ) -> anyhow::Result<rmcp::model::GetPromptResult, GpmcpError> {
        if let Some(ref coordinator) = *self.service_coordinator.read().await {
            coordinator
                .get_prompt(request)
                .await
                .map_err(GpmcpError::Other)
        } else {
            Err(GpmcpError::ServiceNotFound)
        }
    }

    /// Read a resource from the MCP server
    pub async fn read_resource(
        &self,
        request: rmcp::model::ReadResourceRequestParam,
    ) -> anyhow::Result<rmcp::model::ReadResourceResult, GpmcpError> {
        if let Some(ref coordinator) = *self.service_coordinator.read().await {
            coordinator
                .read_resource(request)
                .await
                .map_err(GpmcpError::Other)
        } else {
            Err(GpmcpError::ServiceNotFound)
        }
    }

    /// Get server information
    pub async fn peer_info(&self) -> Option<rmcp::model::ServerInfo> {
        if let Some(ref coordinator) = *self.service_coordinator.read().await {
            coordinator.peer_info().cloned()
        } else {
            None
        }
    }
    /// Restart the process (useful for retry logic)
    pub async fn restart_process(&mut self) -> anyhow::Result<()> {
        self.process_manager
            .write()
            .await
            .as_mut()
            .ok_or(GpmcpError::ProcessError("Process manager not found".to_string()))?
            .restart()
            .await
            .context("Failed to restart process")?;
        info!("Process restarted successfully");
        Ok(())
    }

    pub async fn cancel(self) -> anyhow::Result<()> {
        info!("Cancelling GpmcpRunner");

        // Destructure self to move out the components
        let GpmcpRunnerInner {
            cancellation_token,
            service_coordinator,
            process_manager,
            ..
        } = self;
        cancellation_token.cancel();

        // Cancel service first
        if let Some(coordinator) = service_coordinator.write().await.take() {
            coordinator.cancel().await.ok();
        }

        // Then cleanup process if exists
        if let Some(manager) = process_manager.write().await.take() {
            manager.cleanup().await.ok();
        }

        info!("GpmcpRunner cancelled successfully");
        Ok(())
    }
}
