use anyhow::Result;
use async_trait::async_trait;
use gpmcp_domain::blueprint::ServerDefinition;
use std::sync::Arc;
use rmcp::model::{
    CallToolRequestParam, CallToolResult, GetPromptRequestParam, GetPromptResult,
    ListPromptsResult, ListResourcesResult, ListToolsResult, ReadResourceRequestParam,
    ReadResourceResult, ServerInfo,
};
use super::transport::TransportHandle;

/// Trait for managing MCP service connections and operations
#[async_trait]
pub trait ServiceManager: Send + Sync {
    /// Initialize the service with the given transport and server definition
    async fn initialize(
        &mut self,
        transport: Arc<tokio::sync::Mutex<dyn TransportHandle>>,
        server_definition: &ServerDefinition,
    ) -> Result<()>;
    
    /// Check if the service is ready for operations
    async fn is_ready(&self) -> bool;
    
    /// Get server information
    fn peer_info(&self) -> Option<&ServerInfo>;
    
    /// List available tools from the MCP server
    async fn list_tools(&self) -> Result<ListToolsResult>;
    
    /// List available prompts from the MCP server
    async fn list_prompts(&self) -> Result<ListPromptsResult>;
    
    /// List available resources from the MCP server
    async fn list_resources(&self) -> Result<ListResourcesResult>;
    
    /// Call a tool on the MCP server
    async fn call_tool(&self, request: CallToolRequestParam) -> Result<CallToolResult>;
    
    /// Get a prompt from the MCP server
    async fn get_prompt(&self, request: GetPromptRequestParam) -> Result<GetPromptResult>;
    
    /// Read a resource from the MCP server
    async fn read_resource(&self, request: ReadResourceRequestParam) -> Result<ReadResourceResult>;
    
    /// Cancel the service and clean up resources
    async fn cancel(self: Box<Self>) -> Result<()>;
}

/// High-level trait that coordinates service management with transport and process management
#[async_trait]
pub trait ServiceCoordinator: Send + Sync {
    /// Create a new service coordinator
    async fn new(
        transport: Arc<tokio::sync::Mutex<dyn TransportHandle>>,
        server_definition: ServerDefinition,
    ) -> Result<Box<dyn ServiceCoordinator>>
    where
        Self: Sized;
    
    /// Get the underlying service manager
    fn service_manager(&self) -> &dyn ServiceManager;
    
    /// Get the transport being used
    fn transport(&self) -> &Arc<tokio::sync::Mutex<dyn TransportHandle>>;
    
    /// List available tools from the MCP server
    async fn list_tools(&self) -> Result<ListToolsResult> {
        self.service_manager().list_tools().await
    }
    
    /// List available prompts from the MCP server
    async fn list_prompts(&self) -> Result<ListPromptsResult> {
        self.service_manager().list_prompts().await
    }
    
    /// List available resources from the MCP server
    async fn list_resources(&self) -> Result<ListResourcesResult> {
        self.service_manager().list_resources().await
    }
    
    /// Call a tool on the MCP server
    async fn call_tool(&self, request: CallToolRequestParam) -> Result<CallToolResult> {
        self.service_manager().call_tool(request).await
    }
    
    /// Get a prompt from the MCP server
    async fn get_prompt(&self, request: GetPromptRequestParam) -> Result<GetPromptResult> {
        self.service_manager().get_prompt(request).await
    }
    
    /// Read a resource from the MCP server
    async fn read_resource(&self, request: ReadResourceRequestParam) -> Result<ReadResourceResult> {
        self.service_manager().read_resource(request).await
    }
    
    /// Get server information
    fn peer_info(&self) -> Option<&ServerInfo> {
        self.service_manager().peer_info()
    }
    
    /// Cancel the service coordinator and clean up all resources
    async fn cancel(self: Box<Self>) -> Result<()>;
}