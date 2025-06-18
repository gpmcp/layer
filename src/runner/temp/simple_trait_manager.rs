use anyhow::{Context, Result};
use gpmcp_domain::blueprint::ServerDefinition;
use std::sync::Arc;
use tokio_util::sync::CancellationToken;
use tracing::info;

use crate::runner::traits::{ProcessHandle, ProcessManager, TerminationStrategy, TerminationResult};
use crate::runner::platforms::factory::{create_platform_process_manager, create_platform_termination_strategy, platform_name};

/// Simplified trait-based ProcessManager that focuses on process management
/// This is a minimal implementation that demonstrates the trait abstraction concept
pub struct SimpleTraitProcessManager {
    server_definition: ServerDefinition,
    process_handle: Arc<tokio::sync::Mutex<Option<Box<dyn ProcessHandle>>>>,
    cancellation_token: Arc<CancellationToken>,
    process_manager: Box<dyn ProcessManager>,
    termination_strategy: Box<dyn TerminationStrategy>,
}

impl SimpleTraitProcessManager {
    /// Creates a new SimpleTraitProcessManager
    pub async fn new(
        server_definition: ServerDefinition,
        cancellation_token: Arc<CancellationToken>,
    ) -> Result<Self> {
        let process_handle = Arc::new(tokio::sync::Mutex::new(None));
        let process_manager = create_platform_process_manager();
        let termination_strategy = create_platform_termination_strategy();

        info!("Creating simple trait-based ProcessManager for platform: {}", platform_name());

        let mut manager = Self {
            server_definition,
            process_handle,
            cancellation_token,
            process_manager,
            termination_strategy,
        };

        manager.start_process().await?;
        Ok(manager)
    }

    /// Starts the subprocess
    async fn start_process(&mut self) -> Result<()> {
        info!("Starting process using trait-based implementation");

        let process_handle = self
            .process_manager
            .create_process(&self.server_definition, &self.cancellation_token)
            .await
            .context("Failed to create process")?;

        if let Some(pid) = process_handle.pid() {
            info!("Process started with PID: {}", pid.0);
        }

        // Store the process handle
        {
            let mut handle_guard = self.process_handle.lock().await;
            *handle_guard = Some(process_handle);
        }

        Ok(())
    }

    /// Checks if the process is healthy (still running)
    pub async fn is_healthy(&self) -> bool {
        let handle_guard = self.process_handle.lock().await;
        if let Some(ref handle) = handle_guard.as_ref() {
            self.process_manager.is_healthy(handle.as_ref()).await
        } else {
            false
        }
    }

    /// Terminates the process using the trait-based strategy
    pub async fn terminate(&mut self) -> Result<()> {
        let mut handle_guard = self.process_handle.lock().await;
        if let Some(ref mut handle) = handle_guard.as_mut() {
            if let Some(pid) = handle.pid() {
                info!("Terminating process with PID: {} using trait-based strategy", pid.0);
            }
            
            match self.termination_strategy.terminate_with_strategy(handle.as_mut()).await {
                TerminationResult::Success => {
                    info!("Process terminated successfully");
                    Ok(())
                }
                result => {
                    let msg = format!("Process termination completed with result: {:?}", result);
                    tracing::warn!("{}", msg);
                    Err(anyhow::anyhow!(msg))
                }
            }
        } else {
            Ok(()) // No process to terminate
        }
    }

    /// Gets the current restart count
    pub async fn restart_count(&self) -> u32 {
        let handle_guard = self.process_handle.lock().await;
        if let Some(ref handle) = handle_guard.as_ref() {
            self.process_manager.get_restart_count(handle.as_ref()).await
        } else {
            0
        }
    }

    /// Cleanup the process manager
    pub async fn cleanup(mut self) -> Result<()> {
        info!("Cleaning up simple trait-based ProcessManager");

        // Terminate the process first
        self.terminate().await?;

        // Cleanup the process manager
        self.process_manager.cleanup().await?;

        info!("Simple trait-based ProcessManager cleanup completed");
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpmcp_domain::blueprint::{CommandRunner, Runner};
    use std::collections::HashMap;

    #[tokio::test]
    async fn test_simple_trait_process_manager() {
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

        let cancellation_token = Arc::new(CancellationToken::new());
        let result = SimpleTraitProcessManager::new(server_definition, cancellation_token).await;

        match result {
            Ok(manager) => {
                // Test health check
                let is_healthy = manager.is_healthy().await;
                println!("Process healthy: {}", is_healthy);

                // Test cleanup
                let _ = manager.cleanup().await;
            }
            Err(e) => {
                println!("Expected error for echo command: {}", e);
            }
        }
    }

    #[tokio::test]
    async fn test_termination_strategy() {
        let command_runner = CommandRunner {
            command: "sleep".to_string(),
            args: vec!["2".to_string()],
            workdir: String::new(),
        };

        let server_definition = ServerDefinition {
            runner: Runner::Stdio { command_runner },
            env: HashMap::new(),
            ..Default::default()
        };

        let cancellation_token = Arc::new(CancellationToken::new());
        
        match SimpleTraitProcessManager::new(server_definition, cancellation_token).await {
            Ok(mut manager) => {
                // Process should be healthy initially
                let is_healthy = manager.is_healthy().await;
                println!("Process healthy before termination: {}", is_healthy);

                // Test termination strategy
                let result = manager.terminate().await;
                println!("Termination result: {:?}", result);

                let _ = manager.cleanup().await;
            }
            Err(e) => {
                println!("Error creating manager: {}", e);
            }
        }
    }
}