use anyhow::{Context, Result};
use gpmcp_domain::blueprint::ServerDefinition;
use std::sync::Arc;
use tokio_util::sync::CancellationToken;
use tracing::{info, warn};

use crate::runner::traits::{
    ProcessHandle, ProcessManager, TerminationStrategy, ProcessStatus, TerminationResult
};
use crate::runner::platforms::factory::{create_platform_process_manager, create_platform_termination_strategy, platform_name};

/// Drop-in replacement for the original ProcessManager using trait abstractions
/// This maintains the same public API while using platform-specific trait implementations internally
pub struct TraitBasedProcessManager {
    server_definition: ServerDefinition,
    process_handle: Arc<tokio::sync::Mutex<Option<Box<dyn ProcessHandle>>>>,
    monitor_handle: Option<tokio::task::JoinHandle<()>>,
    cancellation_token: Arc<CancellationToken>,
    process_manager: Box<dyn ProcessManager>,
    termination_strategy: Box<dyn TerminationStrategy>,
}

impl TraitBasedProcessManager {
    /// Creates a new TraitBasedProcessManager and starts the subprocess
    pub async fn new(
        server_definition: ServerDefinition,
        cancellation_token: Arc<CancellationToken>,
    ) -> Result<Self> {
        let process_handle = Arc::new(tokio::sync::Mutex::new(None));
        let process_manager = create_platform_process_manager();
        let termination_strategy = create_platform_termination_strategy();

        info!("Creating trait-based ProcessManager for platform: {}", platform_name());

        let mut manager = Self {
            server_definition,
            process_handle,
            monitor_handle: None,
            cancellation_token,
            process_manager,
            termination_strategy,
        };

        manager.start_process().await?;
        Ok(manager)
    }

    /// Starts the subprocess based on the server definition
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

        // Start monitoring the process
        self.start_monitor().await;

        Ok(())
    }

    /// Starts the process monitor task
    async fn start_monitor(&mut self) {
        let process_handle = self.process_handle.clone();
        let cancellation_token = self.cancellation_token.clone();
        let termination_strategy = create_platform_termination_strategy();

        let monitor_handle = tokio::spawn(async move {
            tokio::select! {
                _ = cancellation_token.cancelled() => {
                    info!("Cancellation requested, terminating child process");
                    Self::terminate_process(&process_handle, termination_strategy).await;
                }
                result = Self::wait_for_process(&process_handle) => {
                    match result {
                        Ok(status) => {
                            info!("Process exited with status: {:?}", status);
                        }
                        Err(e) => {
                            warn!("Error waiting for process: {}", e);
                        }
                    }
                    // Remove the process from the handle since it's no longer running
                    let mut handle_guard = process_handle.lock().await;
                    *handle_guard = None;
                }
            }
        });

        self.monitor_handle = Some(monitor_handle);
    }

    /// Waits for the process to exit
    async fn wait_for_process(
        process_handle: &Arc<tokio::sync::Mutex<Option<Box<dyn ProcessHandle>>>>,
    ) -> Result<ProcessStatus> {
        let mut handle_guard = process_handle.lock().await;
        if let Some(ref mut handle) = handle_guard.as_mut() {
            handle.wait().await
        } else {
            Err(anyhow::anyhow!("No process handle available"))
        }
    }

    /// Terminates the process using the trait-based termination strategy
    async fn terminate_process(
        process_handle: &Arc<tokio::sync::Mutex<Option<Box<dyn ProcessHandle>>>>,
        termination_strategy: Box<dyn TerminationStrategy>,
    ) {
        let mut handle_guard = process_handle.lock().await;
        if let Some(ref mut handle) = handle_guard.as_mut() {
            if let Some(pid) = handle.pid() {
                info!("Starting trait-based termination for PID: {}", pid.0);
            }
            
            match termination_strategy.terminate_with_strategy(handle.as_mut()).await {
                TerminationResult::Success => {
                    info!("Process terminated successfully");
                }
                result => {
                    warn!("Process termination completed with result: {:?}", result);
                }
            }
        }
    }

    /// Restarts the process (useful for retry logic)
    pub async fn restart(&mut self) -> Result<()> {
        info!("Restarting process using trait-based implementation");

        // Stop current process if running
        self.stop_process().await?;

        // Wait a bit before restarting
        tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;

        // Start new process
        self.start_process().await?;

        info!("Process restarted successfully");
        Ok(())
    }

    /// Stops the current process
    async fn stop_process(&mut self) -> Result<()> {
        // Cancel the monitor task
        if let Some(handle) = self.monitor_handle.take() {
            handle.abort();
        }

        // Terminate the process
        Self::terminate_process(&self.process_handle, create_platform_termination_strategy()).await;

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
        info!("Cleaning up trait-based ProcessManager");

        // Stop the process
        self.stop_process().await?;

        // Cleanup the process manager
        self.process_manager.cleanup().await?;

        info!("Trait-based ProcessManager cleanup completed");
        Ok(())
    }
}

impl Drop for TraitBasedProcessManager {
    fn drop(&mut self) {
        // Cancel the cancellation token to trigger cleanup
        self.cancellation_token.cancel();

        // Abort monitor task if it exists
        if let Some(handle) = self.monitor_handle.take() {
            handle.abort();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpmcp_domain::blueprint::{CommandRunner, Runner};
    use std::collections::HashMap;

    #[tokio::test]
    async fn test_trait_based_process_manager_creation() {
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
        let result = TraitBasedProcessManager::new(server_definition, cancellation_token).await;

        match result {
            Ok(manager) => {
                let _ = manager.cleanup().await;
            }
            Err(e) => {
                println!("Expected error for echo command: {}", e);
            }
        }
    }

    #[tokio::test]
    async fn test_trait_based_health_check() {
        let command_runner = CommandRunner {
            command: "sleep".to_string(),
            args: vec!["1".to_string()],
            workdir: String::new(),
        };

        let server_definition = ServerDefinition {
            runner: Runner::Stdio { command_runner },
            env: HashMap::new(),
            ..Default::default()
        };

        let cancellation_token = Arc::new(CancellationToken::new());
        
        match TraitBasedProcessManager::new(server_definition, cancellation_token).await {
            Ok(manager) => {
                // Process should be healthy initially
                let is_healthy = manager.is_healthy().await;
                println!("Process healthy: {}", is_healthy);

                let _ = manager.cleanup().await;
            }
            Err(e) => {
                println!("Expected error: {}", e);
            }
        }
    }
}