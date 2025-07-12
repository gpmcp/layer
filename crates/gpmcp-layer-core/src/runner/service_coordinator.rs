use crate::config::RunnerConfig;
use crate::runner::transport_manager::TransportManager;
use anyhow::{Context, Result};
use rmcp::model::{ClientInfo, InitializeRequestParam};
use rmcp::{ServiceExt, service::RunningService};
use tracing::info;

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
    ) -> Result<Self> {
        info!("Creating service coordinator");

        // Create client info
        let client_info = Self::create_client_info(runner_config);

        // Get the transport and create the service
        let transport = transport_manager.into_transport();

        // Create the service using the appropriate transport
        let service = match transport {
            super::transport_manager::TransportVariant::Stdio(stdio_transport) => client_info
                .serve(stdio_transport)
                .await
                .context("Failed to create stdio service")?,
            super::transport_manager::TransportVariant::Sse(sse_transport) => client_info
                .serve(sse_transport)
                .await
                .context("Failed to create SSE service")?,
            super::transport_manager::TransportVariant::Http(http_transport) => client_info
                .serve(http_transport)
                .await
                .context("Failed to create HTTP service")?,
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
    pub async fn list_tools(&self) -> Result<rmcp::model::ListToolsResult> {
        self.service
            .list_tools(Default::default())
            .await
            .context("Failed to list tools")
    }

    /// List available prompts from the MCP server
    pub async fn list_prompts(&self) -> Result<rmcp::model::ListPromptsResult> {
        self.service
            .list_prompts(Default::default())
            .await
            .context("Failed to list prompts")
    }

    /// List available resources from the MCP server
    pub async fn list_resources(&self) -> Result<rmcp::model::ListResourcesResult> {
        self.service
            .list_resources(Default::default())
            .await
            .context("Failed to list resources")
    }

    /// Call a tool on the MCP server
    pub async fn call_tool(
        &self,
        request: rmcp::model::CallToolRequestParam,
    ) -> Result<rmcp::model::CallToolResult> {
        self.service
            .call_tool(request)
            .await
            .context("Failed to call tool")
    }

    /// Get a prompt from the MCP server
    pub async fn get_prompt(
        &self,
        request: rmcp::model::GetPromptRequestParam,
    ) -> Result<rmcp::model::GetPromptResult> {
        self.service
            .get_prompt(request)
            .await
            .context("Failed to get prompt")
    }

    /// Read a resource from the MCP server
    pub async fn read_resource(
        &self,
        request: rmcp::model::ReadResourceRequestParam,
    ) -> Result<rmcp::model::ReadResourceResult> {
        self.service
            .read_resource(request)
            .await
            .context("Failed to read resource")
    }

    /// Get server information
    pub fn peer_info(&self) -> Option<&rmcp::model::ServerInfo> {
        self.service.peer_info()
    }

    /// Cancel the service
    pub async fn cancel(self) -> Result<()> {
        info!("Cancelling service coordinator");
        self.service
            .cancel()
            .await
            .context("Failed to cancel service")?;
        info!("Service coordinator cancelled successfully");
        Ok(())
    }
}
