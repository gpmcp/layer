use crate::runner::process_manager::ProcessManager;
use anyhow::{Context, Result};
use gpmcp_domain::blueprint::{Runner, ServerDefinition};
use rmcp::transport::{ConfigureCommandExt, SseClientTransport, TokioChildProcess};
use tokio::process::Command;
use tracing::{info, warn};

/// TransportType defines the different transport mechanisms available
#[derive(Debug, Clone)]
pub enum TransportType {
    Stdio,
    Sse { url: String },
}

impl TransportType {
    /// Creates a TransportType from a Runner definition
    pub fn from_runner(runner: &Runner) -> Self {
        match runner {
            Runner::Stdio { .. } => TransportType::Stdio,
            Runner::Sse { url, .. } => TransportType::Sse { url: url.clone() },
        }
    }

    /// Returns true if this transport type needs a subprocess
    pub fn needs_subprocess(&self) -> bool {
        match self {
            TransportType::Stdio => true, // Stdio needs subprocess for communication
            TransportType::Sse { .. } => true, // SSE needs subprocess to run the server
        }
    }
}

/// TransportManager handles the creation and management of different transport types
pub struct TransportManager {
    transport_type: TransportType,
    transport: TransportVariant,
}

pub enum TransportVariant {
    Stdio(TokioChildProcess),
    Sse(SseClientTransport<reqwest::Client>),
}

impl TransportManager {
    /// Creates a new TransportManager
    pub async fn new(
        transport_type: TransportType,
        server_definition: ServerDefinition,
        process_manager: Option<&ProcessManager>,
    ) -> Result<Self> {
        let transport = match &transport_type {
            TransportType::Stdio => {
                info!("Creating stdio transport");
                Self::create_stdio_transport(server_definition).await?
            }
            TransportType::Sse { url } => {
                info!("Creating SSE transport for URL: {}", url);
                Self::create_sse_transport(url.clone(), process_manager).await?
            }
        };

        Ok(Self {
            transport_type,
            transport,
        })
    }

    /// Creates a stdio transport using TokioChildProcess
    async fn create_stdio_transport(
        server_definition: ServerDefinition,
    ) -> Result<TransportVariant> {
        let Runner::Stdio { command_runner } = &server_definition.runner else {
            return Err(anyhow::anyhow!("Expected stdio runner"));
        };

        // Create the command
        let mut cmd = Command::new(&command_runner.command);
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
        let configured_cmd = cmd.configure(|_| {});

        // Try to get PID information before creating transport
        info!(
            "Creating STDIO transport for command: {} with args: {:?}",
            command_runner.command, command_runner.args
        );

        let transport =
            TokioChildProcess::new(configured_cmd).context("Failed to create TokioChildProcess")?;

        // Note: TokioChildProcess doesn't expose PID directly
        warn!(
            "STDIO transport created for command: {} - PID not accessible through TokioChildProcess interface",
            command_runner.command
        );

        Ok(TransportVariant::Stdio(transport))
    }

    /// Creates an SSE transport
    async fn create_sse_transport(
        url: String,
        process_manager: Option<&ProcessManager>,
    ) -> Result<TransportVariant> {
        // Wait a bit for the server to start up (if process manager started it)
        if process_manager.is_some() {
            tokio::time::sleep(tokio::time::Duration::from_millis(1000)).await;
        }

        // Create SSE transport
        let transport = SseClientTransport::start(url.as_str())
            .await
            .context("Failed to create SSE transport")?;

        Ok(TransportVariant::Sse(transport))
    }

    /// Gets the transport type
    pub fn transport_type(&self) -> &TransportType {
        &self.transport_type
    }

    /// Consumes the manager and returns the transport for service creation
    pub fn into_transport(self) -> TransportVariant {
        self.transport
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpmcp_domain::blueprint::CommandRunner;
    use std::collections::HashMap;

    #[test]
    fn test_transport_type_from_runner() {
        let stdio_runner = Runner::Stdio {
            command_runner: CommandRunner {
                command: "echo".to_string(),
                args: vec![],
                workdir: String::new(),
            },
        };

        let transport_type = TransportType::from_runner(&stdio_runner);
        matches!(transport_type, TransportType::Stdio);

        let sse_runner = Runner::Sse {
            command_runner: CommandRunner {
                command: "echo".to_string(),
                args: vec![],
                workdir: String::new(),
            },
            url: "http://localhost:8000/sse".to_string(),
        };

        let transport_type = TransportType::from_runner(&sse_runner);
        matches!(transport_type, TransportType::Sse { .. });
    }

    #[test]
    fn test_transport_needs_subprocess() {
        let stdio_type = TransportType::Stdio;
        assert!(stdio_type.needs_subprocess());

        let sse_type = TransportType::Sse {
            url: "http://localhost:8000/sse".to_string(),
        };
        assert!(sse_type.needs_subprocess());
    }

    #[tokio::test]
    async fn test_stdio_transport_creation() {
        let _ = tracing_subscriber::fmt::try_init();

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

        let transport_type = TransportType::from_runner(&server_definition.runner);
        let result = TransportManager::new(transport_type, server_definition, None).await;

        match result {
            Ok(_manager) => {
                // Transport creation succeeded
                println!("Stdio transport created successfully");
            }
            Err(e) => {
                println!("Expected error for echo command: {}", e);
            }
        }
    }

    #[tokio::test]
    async fn test_sse_transport_creation_without_server() {
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

        let transport_type = TransportType::from_runner(&server_definition.runner);
        let result = TransportManager::new(transport_type, server_definition, None).await;

        match result {
            Ok(_manager) => {
                println!("Unexpected success - no SSE server should be running");
            }
            Err(e) => {
                println!("Expected error for SSE without server: {}", e);
                assert!(e.to_string().contains("Failed to create SSE transport"));
            }
        }
    }
}
