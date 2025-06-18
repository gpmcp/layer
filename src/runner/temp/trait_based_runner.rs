use anyhow::{Context, Result};
use gpmcp_domain::blueprint::ServerDefinition;
use rmcp::service::RunningService;
use std::sync::Arc;
use tokio::sync::Mutex;
use tokio_util::sync::CancellationToken;
use tracing::{info, warn};

use crate::runner::traits::{
    DefaultTransportManager, TransportManager, TransportType, TransportHandle, TransportVariant,
    ProcessManager, ProcessHandle
};
use crate::runner::platforms::factory::create_platform_process_manager;

/// Trait-based GpmcpRunner that uses the new transport abstraction system.
/// This is the new implementation that routes through the trait-based transport layer.
pub struct TraitBasedGpmcpRunner {
    transport_manager: DefaultTransportManager,
    process_manager: Box<dyn ProcessManager>,
    transport_handle: Option<Arc<Mutex<dyn TransportHandle>>>,
    process_handle: Option<Box<dyn ProcessHandle>>,
    cancellation_token: Arc<CancellationToken>,
    server_definition: ServerDefinition,
}

impl TraitBasedGpmcpRunner {
    /// Creates a new TraitBasedGpmcpRunner from a ServerDefinition.
    /// This uses the new trait-based transport and process management system.
    pub async fn new(server_definition: ServerDefinition) -> Result<Self> {
        let cancellation_token = Arc::new(CancellationToken::new());
        let transport_manager = DefaultTransportManager::new();
        let process_manager = create_platform_process_manager();
        
        info!("Creating TraitBasedGpmcpRunner for {:?}", server_definition.runner);
        
        Ok(Self {
            transport_manager,
            process_manager,
            transport_handle: None,
            process_handle: None,
            cancellation_token,
            server_definition,
        })
    }
    
    /// Initialize the runner by creating the process and transport
    pub async fn initialize(&mut self) -> Result<()> {
        // Step 1: Create the process if needed
        let transport_type = TransportType::from_runner(&self.server_definition.runner);
        
        let process_handle = if transport_type.needs_subprocess() {
            info!("Creating subprocess for transport type: {:?}", transport_type);
            let handle = self.process_manager
                .create_process(&self.server_definition, &self.cancellation_token)
                .await
                .context("Failed to create process")?;
            Some(handle)
        } else {
            None
        };
        
        // Step 2: Create the transport
        info!("Creating transport for type: {:?}", transport_type);
        let transport_handle = self.transport_manager
            .create_transport(
                transport_type,
                self.server_definition.clone(),
                process_handle.as_ref().map(|h| h.as_ref())
            )
            .await
            .context("Failed to create transport")?;
        
        // Store handles
        self.transport_handle = Some(transport_handle);
        self.process_handle = process_handle;
        
        info!("✅ TraitBasedGpmcpRunner initialized successfully");
        Ok(())
    }
    
    /// Get the transport handle for MCP operations
    pub fn transport_handle(&self) -> Option<&Arc<Mutex<dyn TransportHandle>>> {
        self.transport_handle.as_ref()
    }
    
    /// Create an MCP service from the transport
    pub async fn create_service(&mut self) -> Result<TraitBasedMcpService> {
        let _transport_handle = self.transport_handle
            .take()
            .ok_or_else(|| anyhow::anyhow!("No transport handle available. Call initialize() first."))?;
        
        // TODO: Service creation needs to be restructured
        // The current design makes it difficult to extract the TransportVariant from Arc<Mutex<dyn TransportHandle>>
        // This is a known limitation that needs architectural changes
        Err(anyhow::anyhow!("Service creation needs to be restructured - transport handle cannot be easily extracted"))
    }
    
    /// Check if the process is healthy (if we have one)
    pub async fn is_process_healthy(&self) -> Result<bool> {
        if let Some(process_handle) = &self.process_handle {
            Ok(self.process_manager.is_healthy(process_handle.as_ref()).await)
        } else {
            // No process means we're healthy (e.g., for pure network transports)
            Ok(true)
        }
    }
    
    /// Get process information (if we have a process)
    pub async fn get_process_info(&self) -> Result<Option<crate::runner::traits::ProcessInfo>> {
        if let Some(process_handle) = &self.process_handle {
            let info = self.process_manager.get_process_info(process_handle.as_ref()).await?;
            Ok(Some(info))
        } else {
            Ok(None)
        }
    }
    
    /// Cancel the runner and clean up resources
    pub async fn cancel(&mut self) -> Result<()> {
        info!("Cancelling TraitBasedGpmcpRunner");
        
        // Cancel the cancellation token
        self.cancellation_token.cancel();
        
        // Close transport if we have one
        if let Some(transport_handle) = &self.transport_handle {
            let mut handle = transport_handle.lock().await;
            if let Err(e) = handle.close() {
                warn!("Failed to close transport: {}", e);
            }
        }
        
        // Terminate the process if we have one (CRITICAL FIX)
        if let Some(ref mut process_handle) = self.process_handle {
            info!("Terminating process during cleanup");
            if let Err(e) = process_handle.terminate().await {
                warn!("Failed to terminate process: {}", e);
            } else {
                info!("Process terminated successfully");
            }
        }
        
        // Cleanup process manager
        if let Err(e) = self.process_manager.cleanup().await {
            warn!("Failed to cleanup process manager: {}", e);
        }
        
        info!("TraitBasedGpmcpRunner cancelled successfully");
        Ok(())
    }
}

impl Drop for TraitBasedGpmcpRunner {
    fn drop(&mut self) {
        // Ensure cancellation token is cancelled when dropped
        self.cancellation_token.cancel();
    }
}

/// Trait-based MCP service that wraps the actual rmcp service
pub struct TraitBasedMcpService {
    service: McpServiceVariant,
}

/// Enum to hold different service types based on transport
pub enum McpServiceVariant {
    Stdio(RunningService<rmcp::RoleClient, rmcp::model::InitializeRequestParam>),
    Sse(RunningService<rmcp::RoleClient, rmcp::model::InitializeRequestParam>),
}

impl TraitBasedMcpService {
    /// Create a new service from a transport variant
    pub async fn new(transport_variant: TransportVariant) -> Result<Self> {
        use rmcp::{ServiceExt, model::{ClientCapabilities, ClientInfo, Implementation}};
        
        // Create client info
        let client_info = ClientInfo {
            protocol_version: Default::default(),
            capabilities: ClientCapabilities::default(),
            client_info: Implementation {
                name: "trait-based-gpmcp-runner".to_string(),
                version: "1.0.0".to_string(),
            },
        };
        
        let service = match transport_variant {
            TransportVariant::Stdio(transport) => {
                info!("Creating stdio MCP service");
                let service = client_info.serve(transport)
                    .await
                    .context("Failed to create stdio service")?;
                McpServiceVariant::Stdio(service)
            }
            TransportVariant::Sse(transport) => {
                info!("Creating SSE MCP service");
                let service = client_info.serve(transport)
                    .await
                    .context("Failed to create SSE service")?;
                McpServiceVariant::Sse(service)
            }
        };
        
        info!("✅ TraitBasedMcpService created successfully");
        
        Ok(Self { service })
    }
    
    /// List available tools from the MCP server
    pub async fn list_tools(&self) -> Result<rmcp::model::ListToolsResult> {
        match &self.service {
            McpServiceVariant::Stdio(service) => service.list_tools(Default::default()).await,
            McpServiceVariant::Sse(service) => service.list_tools(Default::default()).await,
        }
        .context("Failed to list tools")
    }
    
    /// List available prompts from the MCP server
    pub async fn list_prompts(&self) -> Result<rmcp::model::ListPromptsResult> {
        match &self.service {
            McpServiceVariant::Stdio(service) => service.list_prompts(Default::default()).await,
            McpServiceVariant::Sse(service) => service.list_prompts(Default::default()).await,
        }
        .context("Failed to list prompts")
    }
    
    /// List available resources from the MCP server
    pub async fn list_resources(&self) -> Result<rmcp::model::ListResourcesResult> {
        match &self.service {
            McpServiceVariant::Stdio(service) => service.list_resources(Default::default()).await,
            McpServiceVariant::Sse(service) => service.list_resources(Default::default()).await,
        }
        .context("Failed to list resources")
    }
    
    /// Call a tool on the MCP server
    pub async fn call_tool(&self, request: rmcp::model::CallToolRequestParam) -> Result<rmcp::model::CallToolResult> {
        match &self.service {
            McpServiceVariant::Stdio(service) => service.call_tool(request).await,
            McpServiceVariant::Sse(service) => service.call_tool(request).await,
        }
        .context("Failed to call tool")
    }
    
    /// Get server information
    pub fn peer_info(&self) -> Option<&rmcp::model::InitializeResult> {
        match &self.service {
            McpServiceVariant::Stdio(service) => service.peer_info(),
            McpServiceVariant::Sse(service) => service.peer_info(),
        }
    }
    
    /// Cancel the service
    pub async fn cancel(self) -> Result<()> {
        let _quit_reason = match self.service {
            McpServiceVariant::Stdio(service) => service.cancel().await,
            McpServiceVariant::Sse(service) => service.cancel().await,
        }
        .context("Failed to cancel service")?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpmcp_domain::blueprint::{CommandRunner, Runner};
    use std::collections::HashMap;

    #[tokio::test]
    async fn test_trait_based_runner_creation() {
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

        let result = TraitBasedGpmcpRunner::new(server_definition).await;
        assert!(result.is_ok(), "Failed to create TraitBasedGpmcpRunner: {:?}", result.err());
        
        let mut runner = result.unwrap();
        
        // Test initialization
        let init_result = runner.initialize().await;
        // This might fail due to echo not being an MCP server, but creation should work
        match init_result {
            Ok(_) => {
                // If successful, clean up
                let _ = runner.cancel().await;
            }
            Err(e) => {
                // Expected for echo command as it's not an MCP server
                println!("Expected error for echo command: {}", e);
            }
        }
    }
    
    #[tokio::test]
    async fn test_transport_type_detection() {
        let stdio_runner = Runner::Stdio {
            command_runner: CommandRunner {
                command: "test".to_string(),
                args: vec![],
                workdir: String::new(),
            }
        };
        
        let sse_runner = Runner::Sse {
            url: "http://localhost:8080".to_string(),
            command_runner: CommandRunner {
                command: "test".to_string(),
                args: vec![],
                workdir: String::new(),
            }
        };
        
        let stdio_type = TransportType::from_runner(&stdio_runner);
        let sse_type = TransportType::from_runner(&sse_runner);
        
        assert!(matches!(stdio_type, TransportType::Stdio));
        assert!(matches!(sse_type, TransportType::Sse { .. }));
        
        assert!(stdio_type.needs_subprocess());
        assert!(sse_type.needs_subprocess());
    }
}