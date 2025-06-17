use anyhow::Result;
use gpmcp_domain::blueprint::ServerDefinition;
use std::sync::Arc;
use tokio_util::sync::CancellationToken;
use tracing::info;
use crate::runner::process_manager::ProcessManager;
use crate::runner::service_coordinator::ServiceCoordinator;
use crate::runner::transport_manager::{TransportManager, TransportType};

/// GpmcpRunner is the main entry point for managing MCP server connections.
/// It coordinates between process management, transport handling, and service communication.
pub struct GpmcpRunner {
    service_coordinator: ServiceCoordinator,
    process_manager: Option<ProcessManager>,
    cancellation_token: Arc<CancellationToken>,
}

// TODO: split into
// - Service, where you can start and stop the runner -- we might not need this since this.
// - ProcessManager functions so that we can have platform independent impls. 

impl GpmcpRunner {
    /// Creates a new GpmcpRunner from a ServerDefinition.
    pub async fn new(server_definition: ServerDefinition) -> Result<Self> {
        let cancellation_token = Arc::new(CancellationToken::new());

        info!(
            "Creating GpmcpRunner for server: {}",
            server_definition.package.name
        );

        // Determine transport type and create appropriate managers
        let transport_type = TransportType::from_runner(&server_definition.runner);

        // Create process manager if needed (for commands that need subprocess)
        let process_manager = if transport_type.needs_subprocess() {
            Some(ProcessManager::new(server_definition.clone(), cancellation_token.clone()).await?)
        } else {
            None
        };

        // Create transport manager
        let transport_manager = TransportManager::new(
            transport_type,
            server_definition.clone(),
            process_manager.as_ref(),
        )
        .await?;

        // Create service coordinator
        let service_coordinator =
            ServiceCoordinator::new(transport_manager, server_definition).await?;

        info!("✅ GpmcpRunner created successfully");

        Ok(Self {
            service_coordinator,
            process_manager,
            cancellation_token,
        })
    }

    /// List available tools from the MCP server
    pub async fn list_tools(&self) -> Result<rmcp::model::ListToolsResult> {
        self.service_coordinator.list_tools().await
    }

    /// List available prompts from the MCP server
    pub async fn list_prompts(&self) -> Result<rmcp::model::ListPromptsResult> {
        self.service_coordinator.list_prompts().await
    }

    /// List available resources from the MCP server
    pub async fn list_resources(&self) -> Result<rmcp::model::ListResourcesResult> {
        self.service_coordinator.list_resources().await
    }

    /// Call a tool on the MCP server
    pub async fn call_tool(
        &self,
        request: rmcp::model::CallToolRequestParam,
    ) -> Result<rmcp::model::CallToolResult> {
        self.service_coordinator.call_tool(request).await
    }

    /// Get a prompt from the MCP server
    pub async fn get_prompt(
        &self,
        request: rmcp::model::GetPromptRequestParam,
    ) -> Result<rmcp::model::GetPromptResult> {
        self.service_coordinator.get_prompt(request).await
    }

    /// Read a resource from the MCP server
    pub async fn read_resource(
        &self,
        request: rmcp::model::ReadResourceRequestParam,
    ) -> Result<rmcp::model::ReadResourceResult> {
        self.service_coordinator.read_resource(request).await
    }

    /// Get server information
    pub fn peer_info(&self) -> Option<&rmcp::model::ServerInfo> {
        self.service_coordinator.peer_info()
    }

    /// Restart the process (useful for retry logic)
    pub async fn restart_process(&mut self) -> Result<()> {
        if let Some(ref mut process_manager) = self.process_manager {
            process_manager.restart().await?;
            info!("Process restarted successfully");
        }
        Ok(())
    }

    /// Check if the underlying process is healthy
    pub async fn is_process_healthy(&self) -> bool {
        if let Some(ref process_manager) = self.process_manager {
            process_manager.is_healthy().await
        } else {
            true // No process to check
        }
    }

    /// Cancel the runner and clean up resources
    pub async fn cancel(self) -> Result<()> {
        info!("Cancelling GpmcpRunner");

        // Destructure self to move out the components
        let GpmcpRunner {
            service_coordinator,
            process_manager,
            cancellation_token,
        } = self;
        cancellation_token.cancel();

        // Cancel service first
        service_coordinator.cancel().await?;

        // Then cleanup process if exists
        if let Some(process_manager) = process_manager {
            process_manager.cleanup().await?;
        }

        info!("GpmcpRunner cancelled successfully");
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpmcp_domain::blueprint::{CommandRunner, Runner};
    use std::collections::HashMap;

    #[tokio::test]
    async fn test_runner_creation_stdio() {
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

        let result = GpmcpRunner::new(server_definition).await;

        match result {
            Ok(runner) => {
                let _ = runner.cancel().await;
            }
            Err(e) => {
                println!("Expected error for echo command: {}", e);
            }
        }
    }

    #[tokio::test]
    async fn test_runner_creation_sse() {
        let command_runner = CommandRunner {
            command: "sleep".to_string(),
            args: vec!["1".to_string()],
            workdir: String::new(),
        };

        let server_definition = ServerDefinition {
            runner: Runner::Sse {
                command_runner,
                url: "http://localhost:8000/sse".to_string(),
            },
            env: HashMap::new(),
            ..Default::default()
        };

        let result = GpmcpRunner::new(server_definition).await;

        match result {
            Ok(runner) => {
                let _ = runner.cancel().await;
            }
            Err(e) => {
                println!("Expected error due to no SSE server: {}", e);
            }
        }
    }

    #[tokio::test]
    async fn test_process_health_check() {
        let command_runner = CommandRunner {
            command: "sleep".to_string(),
            args: vec!["10".to_string()],
            workdir: String::new(),
        };

        let server_definition = ServerDefinition {
            runner: Runner::Sse {
                command_runner,
                url: "http://localhost:8000/sse".to_string(),
            },
            env: HashMap::new(),
            ..Default::default()
        };

        let result = GpmcpRunner::new(server_definition).await;

        match result {
            Ok(runner) => {
                // Process should be healthy initially (even if SSE fails)
                let is_healthy = runner.is_process_healthy().await;
                println!("Process healthy: {}", is_healthy);

                let _ = runner.cancel().await;
            }
            Err(e) => {
                println!("Expected error: {}", e);
            }
        }
    }
}
