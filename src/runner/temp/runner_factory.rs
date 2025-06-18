use anyhow::Result;
use gpmcp_domain::blueprint::ServerDefinition;
use crate::runner::{TraitBasedGpmcpRunner, TraitBasedMcpService};

/// Configuration for choosing which runner implementation to use
#[derive(Debug, Clone, Copy)]
pub enum RunnerImplementation {
    /// Use the original direct rmcp implementation
    Legacy,
    /// Use the new trait-based implementation
    TraitBased,
}

impl Default for RunnerImplementation {
    fn default() -> Self {
        // Route to new implementation by default
        RunnerImplementation::TraitBased
    }
}

/// Factory for creating GPMCP runners with different implementations
pub struct GpmcpRunnerFactory;

impl GpmcpRunnerFactory {
    /// Create a runner using the specified implementation
    pub async fn create_runner(
        server_definition: ServerDefinition,
        implementation: RunnerImplementation,
    ) -> Result<GpmcpRunnerWrapper> {
        match implementation {
            RunnerImplementation::Legacy => {
                // For now, we'll create the trait-based version
                // In the future, this could route to the original implementation
                tracing::info!("Creating legacy runner (routing to trait-based for now)");
                let runner = TraitBasedGpmcpRunner::new(server_definition).await?;
                Ok(GpmcpRunnerWrapper::TraitBased(runner))
            }
            RunnerImplementation::TraitBased => {
                tracing::info!("Creating trait-based runner");
                let runner = TraitBasedGpmcpRunner::new(server_definition).await?;
                Ok(GpmcpRunnerWrapper::TraitBased(runner))
            }
        }
    }
    
    /// Create a runner using the default implementation (trait-based)
    pub async fn create_default_runner(
        server_definition: ServerDefinition,
    ) -> Result<GpmcpRunnerWrapper> {
        Self::create_runner(server_definition, RunnerImplementation::default()).await
    }
}

/// Wrapper enum that can hold different runner implementations
pub enum GpmcpRunnerWrapper {
    TraitBased(TraitBasedGpmcpRunner),
    // Legacy(OriginalGpmcpRunner), // Could be added later
}

impl GpmcpRunnerWrapper {
    /// Initialize the runner
    pub async fn initialize(&mut self) -> Result<()> {
        match self {
            GpmcpRunnerWrapper::TraitBased(runner) => runner.initialize().await,
        }
    }
    
    /// Create an MCP service
    pub async fn create_service(&mut self) -> Result<McpServiceWrapper> {
        match self {
            GpmcpRunnerWrapper::TraitBased(runner) => {
                let service = runner.create_service().await?;
                Ok(McpServiceWrapper::TraitBased(service))
            }
        }
    }
    
    /// Check if the process is healthy
    pub async fn is_process_healthy(&self) -> Result<bool> {
        match self {
            GpmcpRunnerWrapper::TraitBased(runner) => runner.is_process_healthy().await,
        }
    }
    
    /// Cancel the runner
    pub async fn cancel(&mut self) -> Result<()> {
        match self {
            GpmcpRunnerWrapper::TraitBased(runner) => runner.cancel().await,
        }
    }
}

/// Wrapper enum for different service implementations
pub enum McpServiceWrapper {
    TraitBased(TraitBasedMcpService),
    // Legacy(OriginalMcpService), // Could be added later
}

impl McpServiceWrapper {
    /// List available tools
    pub async fn list_tools(&self) -> Result<rmcp::model::ListToolsResult> {
        match self {
            McpServiceWrapper::TraitBased(service) => service.list_tools().await,
        }
    }
    
    /// List available prompts
    pub async fn list_prompts(&self) -> Result<rmcp::model::ListPromptsResult> {
        match self {
            McpServiceWrapper::TraitBased(service) => service.list_prompts().await,
        }
    }
    
    /// List available resources
    pub async fn list_resources(&self) -> Result<rmcp::model::ListResourcesResult> {
        match self {
            McpServiceWrapper::TraitBased(service) => service.list_resources().await,
        }
    }
    
    /// Call a tool
    pub async fn call_tool(&self, request: rmcp::model::CallToolRequestParam) -> Result<rmcp::model::CallToolResult> {
        match self {
            McpServiceWrapper::TraitBased(service) => service.call_tool(request).await,
        }
    }
    
    /// Get server information
    pub fn peer_info(&self) -> Option<&rmcp::model::InitializeResult> {
        match self {
            McpServiceWrapper::TraitBased(service) => service.peer_info(),
        }
    }
    
    /// Cancel the service
    pub async fn cancel(self) -> Result<()> {
        match self {
            McpServiceWrapper::TraitBased(service) => service.cancel().await,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpmcp_domain::blueprint::{CommandRunner, Runner};
    use std::collections::HashMap;

    #[tokio::test]
    async fn test_factory_creates_trait_based_runner() {
        let command_runner = CommandRunner {
            command: "echo".to_string(),
            args: vec!["test".to_string()],
            workdir: String::new(),
        };

        let server_definition = ServerDefinition {
            runner: Runner::Stdio { command_runner },
            env: HashMap::new(),
            ..Default::default()
        };

        let result = GpmcpRunnerFactory::create_default_runner(server_definition).await;
        assert!(result.is_ok(), "Failed to create runner: {:?}", result.err());
        
        let mut runner = result.unwrap();
        
        // Test that we can initialize (might fail due to echo not being MCP server)
        let _ = runner.initialize().await;
        let _ = runner.cancel().await;
    }
    
    #[tokio::test]
    async fn test_factory_routing() {
        let command_runner = CommandRunner {
            command: "echo".to_string(),
            args: vec!["test".to_string()],
            workdir: String::new(),
        };

        let server_definition = ServerDefinition {
            runner: Runner::Stdio { command_runner },
            env: HashMap::new(),
            ..Default::default()
        };

        // Test trait-based creation
        let trait_based = GpmcpRunnerFactory::create_runner(
            server_definition.clone(), 
            RunnerImplementation::TraitBased
        ).await;
        assert!(trait_based.is_ok());
        
        // Test legacy creation (currently routes to trait-based)
        let legacy = GpmcpRunnerFactory::create_runner(
            server_definition, 
            RunnerImplementation::Legacy
        ).await;
        assert!(legacy.is_ok());
    }
}