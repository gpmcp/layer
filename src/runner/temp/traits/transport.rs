use anyhow::{Context, Result};
use async_trait::async_trait;
use gpmcp_domain::blueprint::{Runner, ServerDefinition};
use std::sync::Arc;
use tokio::sync::Mutex;
use super::process_lifecycle::ProcessHandle;

/// Enum representing different transport types
#[derive(Debug, Clone)]
pub enum TransportType {
    Stdio,
    Sse { url: String },
    Custom(String),
}

impl TransportType {
    pub fn from_runner(runner: &Runner) -> Self {
        match runner {
            Runner::Stdio { .. } => TransportType::Stdio,
            Runner::Sse { url, .. } => TransportType::Sse { url: url.clone() },
        }
    }
    
    pub fn needs_subprocess(&self) -> bool {
        match self {
            TransportType::Stdio => true,
            TransportType::Sse { .. } => true,
            TransportType::Custom(_) => false, // Depends on implementation
        }
    }
}

/// Trait for creating different types of transports
/// Using Arc<Mutex<>> to handle non-Sync transports
#[async_trait]
pub trait TransportCreator: Send + Sync {
    /// Create a transport based on the transport type and server definition
    async fn create_transport(
        &self,
        transport_type: &TransportType,
        server_definition: &ServerDefinition,
        process_handle: Option<&dyn ProcessHandle>,
    ) -> Result<Arc<Mutex<dyn TransportHandle>>>;
    
    /// Check if this creator supports the given transport type
    fn supports_transport_type(&self, transport_type: &TransportType) -> bool;
}

/// Non-async trait for transport operations to avoid Send/Sync issues
/// Wrapped in Arc<Mutex<>> for thread safety
pub trait TransportHandle: Send {
    /// Get the transport type
    fn transport_type(&self) -> &TransportType;
    
    /// Check if the transport is ready for communication (non-async)
    fn is_ready(&self) -> bool;
    
    /// Get any associated process handle (for transports that manage processes)
    fn process_handle(&self) -> Option<&dyn ProcessHandle>;
    
    /// Close the transport and clean up resources (non-async)
    fn close(&mut self) -> Result<()>;
    
    /// Get the underlying transport for service creation
    fn take_inner(self: Box<Self>) -> Result<TransportVariant>;
}

/// High-level trait for managing transports
#[async_trait]
pub trait TransportManager: Send + Sync {
    /// Create and initialize a transport
    async fn create_transport(
        &self,
        transport_type: TransportType,
        server_definition: ServerDefinition,
        process_handle: Option<&dyn ProcessHandle>,
    ) -> Result<Arc<Mutex<dyn TransportHandle>>>;
    
    /// Get supported transport types
    fn supported_transport_types(&self) -> Vec<TransportType>;
    
    /// Clean up resources
    async fn cleanup(&self) -> Result<()>;
}

/// Wrapper for TokioChildProcess transport
pub struct StdioTransportHandle {
    transport: Option<rmcp::transport::TokioChildProcess>,
    transport_type: TransportType,
}

impl StdioTransportHandle {
    pub fn new(transport: rmcp::transport::TokioChildProcess) -> Self {
        Self {
            transport: Some(transport),
            transport_type: TransportType::Stdio,
        }
    }
}

impl TransportHandle for StdioTransportHandle {
    fn transport_type(&self) -> &TransportType {
        &self.transport_type
    }
    
    fn is_ready(&self) -> bool {
        self.transport.is_some()
    }
    
    fn process_handle(&self) -> Option<&dyn ProcessHandle> {
        // TokioChildProcess doesn't expose the underlying process handle
        None
    }
    
    fn close(&mut self) -> Result<()> {
        if let Some(_transport) = self.transport.take() {
            // TokioChildProcess doesn't have an explicit close method
            // The transport will be dropped and cleaned up automatically
        }
        Ok(())
    }
    
    fn take_inner(mut self: Box<Self>) -> Result<TransportVariant> {
        if let Some(transport) = self.transport.take() {
            Ok(TransportVariant::Stdio(transport))
        } else {
            Err(anyhow::anyhow!("Transport already consumed"))
        }
    }
}

/// Wrapper for SSE transport that handles the non-Sync issue
pub struct SseTransportHandle {
    transport: Option<rmcp::transport::SseClientTransport<reqwest::Client>>,
    transport_type: TransportType,
}

impl SseTransportHandle {
    pub fn new(transport: rmcp::transport::SseClientTransport<reqwest::Client>, url: String) -> Self {
        Self {
            transport: Some(transport),
            transport_type: TransportType::Sse { url },
        }
    }
}

impl TransportHandle for SseTransportHandle {
    fn transport_type(&self) -> &TransportType {
        &self.transport_type
    }
    
    fn is_ready(&self) -> bool {
        self.transport.is_some()
    }
    
    fn process_handle(&self) -> Option<&dyn ProcessHandle> {
        // SSE transport doesn't manage processes directly
        None
    }
    
    fn close(&mut self) -> Result<()> {
        if let Some(_transport) = self.transport.take() {
            // SSE transport cleanup happens automatically on drop
        }
        Ok(())
    }
    
    fn take_inner(mut self: Box<Self>) -> Result<TransportVariant> {
        if let Some(transport) = self.transport.take() {
            Ok(TransportVariant::Sse(transport))
        } else {
            Err(anyhow::anyhow!("Transport already consumed"))
        }
    }
}

/// Enum to hold different transport implementations for service creation
pub enum TransportVariant {
    Stdio(rmcp::transport::TokioChildProcess),
    Sse(rmcp::transport::SseClientTransport<reqwest::Client>),
}

/// Default transport creator implementation
pub struct DefaultTransportCreator;

#[async_trait]
impl TransportCreator for DefaultTransportCreator {
    async fn create_transport(
        &self,
        transport_type: &TransportType,
        server_definition: &ServerDefinition,
        process_handle: Option<&dyn ProcessHandle>,
    ) -> Result<Arc<Mutex<dyn TransportHandle>>> {
        match transport_type {
            TransportType::Stdio => {
                let transport = self.create_stdio_transport(server_definition).await?;
                Ok(Arc::new(Mutex::new(StdioTransportHandle::new(transport))))
            }
            TransportType::Sse { url } => {
                let transport = self.create_sse_transport(url.clone(), process_handle).await?;
                Ok(Arc::new(Mutex::new(SseTransportHandle::new(transport, url.clone()))))
            }
            TransportType::Custom(_) => {
                Err(anyhow::anyhow!("Custom transport types not yet supported"))
            }
        }
    }
    
    fn supports_transport_type(&self, transport_type: &TransportType) -> bool {
        matches!(transport_type, TransportType::Stdio | TransportType::Sse { .. })
    }
}

impl DefaultTransportCreator {
    /// Creates a stdio transport using TokioChildProcess
    async fn create_stdio_transport(
        &self,
        server_definition: &ServerDefinition,
    ) -> Result<rmcp::transport::TokioChildProcess> {
        let Runner::Stdio { command_runner } = &server_definition.runner else {
            return Err(anyhow::anyhow!("Expected stdio runner"));
        };

        // Create the command
        let mut cmd = tokio::process::Command::new(&command_runner.command);
        cmd.args(&command_runner.args);

        // Set working directory if specified
        if !command_runner.workdir.is_empty() {
            cmd.current_dir(&command_runner.workdir);
        }

        // Add environment variables
        for (key, value) in &server_definition.env {
            cmd.env(key, value);
        }

        // Configure command for stdio communication and create transport
        use rmcp::transport::ConfigureCommandExt;
        let configured_cmd = cmd.configure(|_| {});

        tracing::info!(
            "Creating STDIO transport for command: {} with args: {:?}",
            command_runner.command, command_runner.args
        );

        let transport = rmcp::transport::TokioChildProcess::new(configured_cmd)
            .with_context(|| "Failed to create TokioChildProcess")?;

        Ok(transport)
    }

    /// Creates an SSE transport
    async fn create_sse_transport(
        &self,
        url: String,
        process_handle: Option<&dyn ProcessHandle>,
    ) -> Result<rmcp::transport::SseClientTransport<reqwest::Client>> {
        // Wait a bit for the server to start up (if process handle provided)
        if process_handle.is_some() {
            tokio::time::sleep(tokio::time::Duration::from_millis(1000)).await;
        }

        // Create SSE transport
        let transport = rmcp::transport::SseClientTransport::start(url.as_str())
            .await
            .with_context(|| "Failed to create SSE transport")?;

        Ok(transport)
    }
}

/// Default transport manager implementation
pub struct DefaultTransportManager {
    creator: DefaultTransportCreator,
}

impl DefaultTransportManager {
    pub fn new() -> Self {
        Self {
            creator: DefaultTransportCreator,
        }
    }
}

impl Default for DefaultTransportManager {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl TransportManager for DefaultTransportManager {
    async fn create_transport(
        &self,
        transport_type: TransportType,
        server_definition: ServerDefinition,
        process_handle: Option<&dyn ProcessHandle>,
    ) -> Result<Arc<Mutex<dyn TransportHandle>>> {
        self.creator.create_transport(&transport_type, &server_definition, process_handle).await
    }
    
    fn supported_transport_types(&self) -> Vec<TransportType> {
        vec![
            TransportType::Stdio,
            TransportType::Sse { url: "example".to_string() },
        ]
    }
    
    async fn cleanup(&self) -> Result<()> {
        // No cleanup needed for default implementation
        Ok(())
    }
}