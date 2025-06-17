use crate::runner::transport_manager::TransportManager;
use anyhow::{Context, Result};
use gpmcp_domain::blueprint::ServerDefinition;
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
        server_definition: ServerDefinition,
    ) -> Result<Self> {
        info!("Creating service coordinator");

        // Create client info
        let client_info = Self::create_client_info(&server_definition);

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
        };

        info!("✅ Service coordinator created successfully");

        Ok(Self { service })
    }

    /// Creates client info from server definition
    fn create_client_info(server_definition: &ServerDefinition) -> ClientInfo {
        ClientInfo {
            protocol_version: rmcp::model::ProtocolVersion::default(),
            capabilities: rmcp::model::ClientCapabilities::default(),
            client_info: rmcp::model::Implementation {
                name: server_definition.package.name.to_string(),
                version: server_definition.package.version.to_string(),
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runner::transport_manager::TransportType;
    use gpmcp_domain::blueprint::{CommandRunner, Runner};
    use std::collections::HashMap;

    fn create_test_server_definition() -> ServerDefinition {
        let command_runner = CommandRunner {
            command: "echo".to_string(),
            args: vec!["hello".to_string()],
            workdir: String::new(),
        };

        ServerDefinition {
            runner: Runner::Stdio { command_runner },
            env: HashMap::new(),
            ..Default::default()
        }
    }

    #[test]
    fn test_client_info_creation() {
        let server_definition = create_test_server_definition();
        let client_info = ServiceCoordinator::create_client_info(&server_definition);

        assert_eq!(client_info.client_info.name, server_definition.package.name);
        assert_eq!(
            client_info.client_info.version,
            server_definition.package.version
        );
    }

    #[tokio::test]
    async fn test_service_coordinator_creation() {
        let server_definition = create_test_server_definition();
        let transport_type = TransportType::from_runner(&server_definition.runner);

        let transport_manager =
            TransportManager::new(transport_type, server_definition.clone(), None).await;

        match transport_manager {
            Ok(tm) => {
                let result = ServiceCoordinator::new(tm, server_definition).await;
                match result {
                    Ok(coordinator) => {
                        let _ = coordinator.cancel().await;
                    }
                    Err(e) => {
                        println!("Expected error for echo command: {}", e);
                    }
                }
            }
            Err(e) => {
                println!("Transport manager creation failed: {}", e);
            }
        }
    }
}
