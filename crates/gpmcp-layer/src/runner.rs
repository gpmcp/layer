use crate::factory::PlatformRunnerProcessManagerFactory;
use gpmcp_layer_core::error::GpmcpError;
use gpmcp_layer_core::layer::{GpmcpLayer, Initialized, Uninitialized};
use gpmcp_layer_core::process_manager_trait::RunnerProcessManagerFactory;
use gpmcp_layer_core::{LayerStdErr, LayerStdOut};
use std::sync::Arc;

/// High-level platform-independent GpmcpRunner
pub struct GpmcpCrossLayer<Status> {
    inner: GpmcpLayer<
        Status,
        <PlatformRunnerProcessManagerFactory as RunnerProcessManagerFactory>::Manager,
    >,
}

impl GpmcpCrossLayer<Uninitialized> {
    /// Create a new GpmcpRunner with the given configuration
    pub fn new(config: gpmcp_layer_core::config::RunnerConfig) -> Self {
        let manager = PlatformRunnerProcessManagerFactory::create_process_manager(&config);
        Self {
            inner: GpmcpLayer::new(config, Arc::new(manager)),
        }
    }

    pub fn new_with_buffer(
        config: gpmcp_layer_core::config::RunnerConfig,
        out: LayerStdOut,
        err: LayerStdErr,
    ) -> Self {
        let manager = PlatformRunnerProcessManagerFactory::create_process_manager(&config);
        Self {
            inner: GpmcpLayer::new_with_buffers(config, Arc::new(manager), out, err),
        }
    }

    /// Connect to the MCP server using the new trait-based process manager
    pub async fn connect(self) -> anyhow::Result<GpmcpCrossLayer<Initialized>, GpmcpError> {
        let connected = self.inner.connect().await?;
        Ok(GpmcpCrossLayer { inner: connected })
    }
}

impl GpmcpCrossLayer<Initialized> {
    /// List available tools from the MCP server
    pub async fn list_tools(&self) -> anyhow::Result<rmcp::model::ListToolsResult, GpmcpError> {
        self.inner.list_tools().await
    }

    /// Call a tool on the MCP server
    pub async fn call_tool(
        &self,
        request: rmcp::model::CallToolRequestParam,
    ) -> anyhow::Result<rmcp::model::CallToolResult, GpmcpError> {
        self.inner.call_tool(request).await
    }

    /// List available prompts from the MCP server
    pub async fn list_prompts(&self) -> anyhow::Result<rmcp::model::ListPromptsResult, GpmcpError> {
        self.inner.list_prompts().await
    }

    /// List available resources from the MCP server
    pub async fn list_resources(
        &self,
    ) -> anyhow::Result<rmcp::model::ListResourcesResult, GpmcpError> {
        self.inner.list_resources().await
    }

    /// Get a prompt from the MCP server
    pub async fn get_prompt(
        &self,
        request: rmcp::model::GetPromptRequestParam,
    ) -> anyhow::Result<rmcp::model::GetPromptResult, GpmcpError> {
        self.inner.get_prompt(request).await
    }

    /// Read a resource from the MCP server
    pub async fn read_resource(
        &self,
        request: rmcp::model::ReadResourceRequestParam,
    ) -> anyhow::Result<rmcp::model::ReadResourceResult, GpmcpError> {
        self.inner.read_resource(request).await
    }

    /// Get server information
    pub async fn peer_info(&self) -> Option<rmcp::model::ServerInfo> {
        self.inner.peer_info().await
    }

    /// Cancel the runner and cleanup resources
    pub async fn cancel(self) -> anyhow::Result<(), GpmcpError> {
        self.inner.cancel().await
    }
}
