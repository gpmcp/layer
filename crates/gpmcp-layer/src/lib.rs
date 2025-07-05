use anyhow::Result;
use async_trait::async_trait;

/// Platform-independent factory that selects the appropriate implementation at compile time
pub struct PlatformRunnerProcessManagerFactory;

#[async_trait]
impl gpmcp_layer_core::RunnerProcessManagerFactory for PlatformRunnerProcessManagerFactory {
    #[cfg(unix)]
    type Manager = gpmcp_layer_unix::UnixRunnerProcessManager;

    #[cfg(windows)]
    type Manager = gpmcp_layer_windows::WindowsRunnerProcessManager;

    async fn create_process_manager(
        config: &gpmcp_layer_core::RunnerConfig,
    ) -> Result<Self::Manager> {
        #[cfg(unix)]
        return gpmcp_layer_unix::UnixRunnerProcessManagerFactory::create_process_manager(config)
            .await;

        #[cfg(windows)]
        return gpmcp_layer_windows::WindowsRunnerProcessManagerFactory::create_process_manager(
            config,
        )
        .await;
    }

    fn platform_name() -> &'static str {
        #[cfg(unix)]
        return "unix";

        #[cfg(windows)]
        return "windows";
    }
}

/// Convenience function to create a platform-appropriate RunnerProcessManager
pub fn create_runner_process_manager(
    config: &gpmcp_layer_core::RunnerConfig,
) -> std::pin::Pin<Box<dyn Future<Output = Result<Box<dyn RunnerProcessManager>>> + Send>> {
    let config = config.clone();
    Box::pin(async move {
        let manager = PlatformRunnerProcessManagerFactory::create_process_manager(&config).await?;
        Ok(Box::new(manager) as Box<dyn gpmcp_layer_core::RunnerProcessManager>)
    })
}

/// High-level GpmcpRunner that uses the new trait-based architecture
pub struct GpmcpRunner {
    inner: gpmcp_layer_core::GpmcpRunnerInner<gpmcp_layer_core::Uninitialized>,
}

impl GpmcpRunner {
    /// Create a new GpmcpRunner with the given configuration
    pub fn new(config: gpmcp_layer_core::RunnerConfig) -> Self {
        Self {
            inner: gpmcp_layer_core::GpmcpRunnerInner::new(config),
        }
    }

    /// Connect to the MCP server using the new trait-based process manager
    pub async fn connect(self) -> Result<ConnectedGpmcpRunner, gpmcp_layer_core::GpmcpError> {
        let connected = self
            .inner
            .connect_with_factory(create_runner_process_manager)
            .await?;
        Ok(ConnectedGpmcpRunner { inner: connected })
    }
}

/// Connected GpmcpRunner that provides access to MCP operations
pub struct ConnectedGpmcpRunner {
    inner: gpmcp_layer_core::GpmcpRunnerInner<gpmcp_layer_core::Initialized>,
}

impl ConnectedGpmcpRunner {
    /// List available tools from the MCP server
    pub async fn list_tools(
        &self,
    ) -> Result<rmcp::model::ListToolsResult, gpmcp_layer_core::GpmcpError> {
        self.inner.list_tools().await
    }

    /// Call a tool on the MCP server
    pub async fn call_tool(
        &self,
        request: rmcp::model::CallToolRequestParam,
    ) -> Result<rmcp::model::CallToolResult, gpmcp_layer_core::GpmcpError> {
        self.inner.call_tool(request).await
    }

    /// List available prompts from the MCP server
    pub async fn list_prompts(
        &self,
    ) -> Result<rmcp::model::ListPromptsResult, gpmcp_layer_core::GpmcpError> {
        self.inner.list_prompts().await
    }

    /// List available resources from the MCP server
    pub async fn list_resources(
        &self,
    ) -> Result<rmcp::model::ListResourcesResult, gpmcp_layer_core::GpmcpError> {
        self.inner.list_resources().await
    }

    /// Get a prompt from the MCP server
    pub async fn get_prompt(
        &self,
        request: rmcp::model::GetPromptRequestParam,
    ) -> Result<rmcp::model::GetPromptResult, gpmcp_layer_core::GpmcpError> {
        self.inner.get_prompt(request).await
    }

    /// Read a resource from the MCP server
    pub async fn read_resource(
        &self,
        request: rmcp::model::ReadResourceRequestParam,
    ) -> Result<rmcp::model::ReadResourceResult, gpmcp_layer_core::GpmcpError> {
        self.inner.read_resource(request).await
    }

    /// Get server information
    pub async fn peer_info(&self) -> Option<rmcp::model::ServerInfo> {
        self.inner.peer_info().await
    }

    /// Cancel the runner and cleanup resources
    pub async fn cancel(self) -> Result<(), gpmcp_layer_core::GpmcpError> {
        self.inner.cancel().await
    }
}

// Re-export core functionality
pub use gpmcp_layer_core::*;
