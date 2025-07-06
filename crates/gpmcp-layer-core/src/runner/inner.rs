use crate::config::{RunnerConfig, Transport};
use crate::error::GpmcpError;
use crate::layer::{Initialized, Uninitialized};
use crate::process_manager_trait::DynRunnerProcessManager;
use crate::runner::service_coordinator::ServiceCoordinator;
use crate::runner::transport_manager::TransportManager;
use std::sync::Arc;
use tokio::sync::RwLock;
use tokio_util::sync::CancellationToken;
use tracing::info;

#[derive(Clone)]
pub struct GpmcpRunnerInner<Status> {
    cancellation_token: Arc<CancellationToken>,
    runner_config: RunnerConfig,
    service_coordinator: Arc<RwLock<Option<ServiceCoordinator>>>,
    process_manager: Arc<dyn DynRunnerProcessManager>,
    _status: std::marker::PhantomData<Status>,
}

impl GpmcpRunnerInner<Uninitialized> {
    pub fn new(
        runner_config: RunnerConfig,
        process_manager: Arc<dyn DynRunnerProcessManager>,
    ) -> Self {
        Self {
            cancellation_token: Arc::new(CancellationToken::new()),
            runner_config,
            service_coordinator: Arc::new(RwLock::new(None)),
            process_manager,
            _status: Default::default(),
        }
    }

    pub async fn connect(&self) -> Result<GpmcpRunnerInner<Initialized>, GpmcpError> {
        info!(
            "Creating GpmcpRunner for server: {}",
            self.runner_config.name
        );

        // Determine transport type and create appropriate managers

        // For SSE transport, start the server process first
        if matches!(self.runner_config.transport, Transport::Sse { .. }) {
            info!("Starting server process for SSE transport");
            let _handle =
                self.process_manager.start_server().await.map_err(|e| {
                    GpmcpError::process_error(format!("Failed to start server: {e}"))
                })?;
        }

        // Create transport manager
        let transport_manager = TransportManager::new(&self.runner_config)
            .await
            .map_err(|e| GpmcpError::transport_error(format!("Failed to create transport: {e}")))?;

        // Create service coordinator
        let service_coordinator = ServiceCoordinator::new(transport_manager, &self.runner_config)
            .await
            .map_err(|e| {
                GpmcpError::service_initialization_failed(format!(
                    "Failed to create service coordinator: {e}"
                ))
            })?;

        self.service_coordinator
            .write()
            .await
            .replace(service_coordinator);

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
            coordinator
                .list_tools()
                .await
                .map_err(|e| GpmcpError::mcp_operation_failed(format!("list_tools failed: {e}")))
        } else {
            Err(GpmcpError::ServiceNotFound)
        }
    }

    /// Call a tool on the MCP server
    pub async fn call_tool(
        &self,
        request: rmcp::model::CallToolRequestParam,
    ) -> Result<rmcp::model::CallToolResult, GpmcpError> {
        if let Some(ref coordinator) = *self.service_coordinator.read().await {
            coordinator
                .call_tool(request)
                .await
                .map_err(|e| GpmcpError::mcp_operation_failed(format!("call_tool failed: {e}")))
        } else {
            Err(GpmcpError::ServiceNotFound)
        }
    }

    /// List available prompts from the MCP server
    pub async fn list_prompts(&self) -> Result<rmcp::model::ListPromptsResult, GpmcpError> {
        if let Some(ref coordinator) = *self.service_coordinator.read().await {
            coordinator
                .list_prompts()
                .await
                .map_err(|e| GpmcpError::mcp_operation_failed(format!("list_prompts failed: {e}")))
        } else {
            Err(GpmcpError::ServiceNotFound)
        }
    }

    /// List available resources from the MCP server
    pub async fn list_resources(&self) -> Result<rmcp::model::ListResourcesResult, GpmcpError> {
        if let Some(ref coordinator) = *self.service_coordinator.read().await {
            coordinator.list_resources().await.map_err(|e| {
                GpmcpError::mcp_operation_failed(format!("list_resources failed: {e}"))
            })
        } else {
            Err(GpmcpError::ServiceNotFound)
        }
    }

    /// Get a prompt from the MCP server
    pub async fn get_prompt(
        &self,
        request: rmcp::model::GetPromptRequestParam,
    ) -> Result<rmcp::model::GetPromptResult, GpmcpError> {
        if let Some(ref coordinator) = *self.service_coordinator.read().await {
            coordinator
                .get_prompt(request)
                .await
                .map_err(|e| GpmcpError::mcp_operation_failed(format!("get_prompt failed: {e}")))
        } else {
            Err(GpmcpError::ServiceNotFound)
        }
    }

    /// Read a resource from the MCP server
    pub async fn read_resource(
        &self,
        request: rmcp::model::ReadResourceRequestParam,
    ) -> Result<rmcp::model::ReadResourceResult, GpmcpError> {
        if let Some(ref coordinator) = *self.service_coordinator.read().await {
            coordinator
                .read_resource(request)
                .await
                .map_err(|e| GpmcpError::mcp_operation_failed(format!("read_resource failed: {e}")))
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
            coordinator.cancel().await.map_err(|e| {
                GpmcpError::service_initialization_failed(format!("Failed to cancel service: {e}"))
            })?;
        }

        process_manager
            .cleanup()
            .await
            .map_err(|e| GpmcpError::process_error(format!("Failed to cleanup process: {e}")))?;

        info!("GpmcpRunner cancelled successfully");
        Ok(())
    }
}
