use crate::RunnerConfig;
use crate::error::GpmcpError;
use crate::runner::transport_manager::TransportManager;
use anyhow::Result;
use rmcp::model::{ClientInfo, InitializeRequestParam};
use rmcp::{ServiceExt, service::RunningService};
use tokio::task::JoinError;
use tracing::{error, info};

/// ServiceCoordinator manages the MCP service connection and provides
/// a unified interface for all MCP operations
pub struct ServiceCoordinator {
    service: RunningService<rmcp::RoleClient, InitializeRequestParam>,
}

impl ServiceCoordinator {
    /// Creates a new ServiceCoordinator
    pub async fn new(
        transport_manager: TransportManager,
        runner_config: &RunnerConfig,
    ) -> Result<Self, GpmcpError> {
        info!("Creating service coordinator");

        // Create client info
        let client_info = Self::create_client_info(runner_config);

        // Get the transport and create the service
        let transport = transport_manager.into_transport();

        // Create the service using the appropriate transport
        let service = match transport {
            super::transport_manager::TransportVariant::Stdio(stdio_transport) => {
                client_info.serve(stdio_transport).await?
            }
            super::transport_manager::TransportVariant::Sse(sse_transport) => {
                client_info.serve(sse_transport).await?
            }
        };

        info!("Service coordinator created successfully");

        Ok(Self { service })
    }

    /// Creates client info from server definition
    fn create_client_info(runner_config: &RunnerConfig) -> ClientInfo {
        ClientInfo {
            protocol_version: rmcp::model::ProtocolVersion::default(),
            capabilities: rmcp::model::ClientCapabilities::default(),
            client_info: rmcp::model::Implementation {
                name: runner_config.name.to_string(),
                version: runner_config.version.to_string(),
            },
        }
    }

    /// List available tools from the MCP server
    pub async fn list_tools(&self) -> Result<rmcp::model::ListToolsResult, rmcp::ServiceError> {
        self.service.list_tools(Default::default()).await
    }

    /// List available prompts from the MCP server
    pub async fn list_prompts(&self) -> Result<rmcp::model::ListPromptsResult, rmcp::ServiceError> {
        self.service.list_prompts(Default::default()).await
    }

    /// List available resources from the MCP server
    pub async fn list_resources(
        &self,
    ) -> Result<rmcp::model::ListResourcesResult, rmcp::ServiceError> {
        self.service.list_resources(Default::default()).await
    }

    /// Call a tool on the MCP server
    pub async fn call_tool(
        &self,
        request: rmcp::model::CallToolRequestParam,
    ) -> Result<rmcp::model::CallToolResult, rmcp::ServiceError> {
        self.service.call_tool(request).await
    }

    /// Get a prompt from the MCP server
    pub async fn get_prompt(
        &self,
        request: rmcp::model::GetPromptRequestParam,
    ) -> Result<rmcp::model::GetPromptResult, rmcp::ServiceError> {
        self.service.get_prompt(request).await
    }

    /// Read a resource from the MCP server
    pub async fn read_resource(
        &self,
        request: rmcp::model::ReadResourceRequestParam,
    ) -> Result<rmcp::model::ReadResourceResult, rmcp::ServiceError> {
        self.service.read_resource(request).await
    }

    /// Get server information
    pub fn peer_info(&self) -> Option<&rmcp::model::ServerInfo> {
        self.service.peer_info()
    }

    /// Cancel the service
    pub async fn cancel(self) -> Result<(), JoinError> {
        info!("Cancelling service coordinator");
        match self.service.cancel().await {
            Ok(_) => {
                info!("Service coordinator cancelled successfully");
                Ok(())
            }
            Err(e) => {
                error!(error=%e, "Failed to cancel service coordinator");
                Err(e)
            }
        }
    }
}
