use crate::GpmcpError;
use crate::RunnerConfig;
use crate::catch::Catch;
use crate::runner::process_manager::ProcessManager;
use crate::runner::service_coordinator::ServiceCoordinator;
use crate::runner::transport_manager::TransportManager;
use std::sync::Arc;
use tokio::sync::RwLock;
use tokio_util::sync::CancellationToken;
use tracing::{error, info, warn};

pub struct Initialized;

pub struct Uninitialized;

#[derive(Clone)]
pub struct GpmcpRunnerInner<Status> {
    cancellation_token: Arc<CancellationToken>,
    runner_config: RunnerConfig,
    service_coordinator: Arc<RwLock<Option<ServiceCoordinator>>>,
    process_manager: Arc<RwLock<Option<ProcessManager>>>,
    _status: std::marker::PhantomData<Status>,
}

impl GpmcpRunnerInner<Uninitialized> {
    pub fn new(runner_config: RunnerConfig) -> Self {
        Self {
            cancellation_token: Arc::new(CancellationToken::new()),
            runner_config,
            service_coordinator: Arc::new(RwLock::new(None)),
            process_manager: Arc::new(RwLock::new(None)),
            _status: Default::default(),
        }
    }
    pub async fn connect(&self) -> Result<GpmcpRunnerInner<Initialized>, GpmcpError> {

        // Determine transport type and create appropriate managers

        // Create process manager if needed (for commands that need subprocess)
        let process_manager = ProcessManager::new(&self.runner_config).await;

        // For SSE transport, start the server process first
        if matches!(self.runner_config.transport, crate::Transport::Sse { .. }) {
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

        Ok(GpmcpRunnerInner {
            cancellation_token: self.cancellation_token.clone(),
            runner_config: self.runner_config.clone(),
            service_coordinator: self.service_coordinator.clone(),
            process_manager: self.process_manager.clone(),
            _status: Default::default(),
        })
    }
}

impl GpmcpRunnerInner<Initialized> {
    /// List available tools from the MCP server
    pub async fn list_tools(&self) -> Result<rmcp::model::ListToolsResult, GpmcpError> {
        if let Some(ref coordinator) = *self.service_coordinator.read().await {
            info!("Listing available tools from MCP server");
            coordinator.list_tools().await.catch()
        } else {
            error!("Service coordinator not available for list_tools operation");
            Err(GpmcpError::service_not_found())
        }
    }

    /// Call a tool on the MCP server
    pub async fn call_tool(
        &self,
        request: rmcp::model::CallToolRequestParam,
    ) -> Result<rmcp::model::CallToolResult, GpmcpError> {
        if let Some(ref coordinator) = *self.service_coordinator.read().await {
            info!("Calling tool: {}", request.name);
            coordinator.call_tool(request).await.catch()
        } else {
            error!("Service coordinator not available for call_tool operation");
            Err(GpmcpError::service_not_found())
        }
    }

    /// List available prompts from the MCP server
    pub async fn list_prompts(&self) -> Result<rmcp::model::ListPromptsResult, GpmcpError> {
        if let Some(ref coordinator) = *self.service_coordinator.read().await {
            info!("Listing available prompts from MCP server");
            coordinator.list_prompts().await.catch()
        } else {
            error!("Service coordinator not available for list_prompts operation");
            Err(GpmcpError::service_not_found())
        }
    }

    /// List available resources from the MCP server
    pub async fn list_resources(&self) -> Result<rmcp::model::ListResourcesResult, GpmcpError> {
        if let Some(ref coordinator) = *self.service_coordinator.read().await {
            info!("Listing available resources from MCP server");
            coordinator.list_resources().await.catch()
        } else {
            error!("Service coordinator not available for list_resources operation");
            Err(GpmcpError::service_not_found())
        }
    }

    /// Get a prompt from the MCP server
    pub async fn get_prompt(
        &self,
        request: rmcp::model::GetPromptRequestParam,
    ) -> Result<rmcp::model::GetPromptResult, GpmcpError> {
        if let Some(ref coordinator) = *self.service_coordinator.read().await {
            info!("Getting prompt: {}", request.name);
            coordinator.get_prompt(request).await.catch()
        } else {
            error!("Service coordinator not available for get_prompt operation");
            Err(GpmcpError::service_not_found())
        }
    }

    /// Read a resource from the MCP server
    pub async fn read_resource(
        &self,
        request: rmcp::model::ReadResourceRequestParam,
    ) -> Result<rmcp::model::ReadResourceResult, GpmcpError> {
        if let Some(ref coordinator) = *self.service_coordinator.read().await {
            info!("Reading resource: {}", request.uri);
            coordinator.read_resource(request).await.catch()
        } else {
            error!("Service coordinator not available for read_resource operation");
            Err(GpmcpError::service_not_found())
        }
    }

    /// Get server information
    pub async fn peer_info(&self) -> Option<rmcp::model::ServerInfo> {
        if let Some(ref coordinator) = *self.service_coordinator.read().await {
            info!("Retrieving peer server information");
            coordinator.peer_info().cloned()
        } else {
            warn!("Service coordinator not available for peer_info operation");
            None
        }
    }

    pub async fn cancel(&self) -> Result<(), GpmcpError> {
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
            coordinator.cancel().await?;
        }

        // Then cleanup process if exists
        if let Some(manager) = process_manager.write().await.take() {
            manager.cleanup().await;
        }
        info!("GpmcpRunner cancelled successfully");
        Ok(())
    }
}
