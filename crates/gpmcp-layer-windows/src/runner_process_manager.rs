use anyhow::{Context, Result};
use async_trait::async_trait;
use gpmcp_layer_core::{
    config::RunnerConfig,
    process::{ProcessHandle, ProcessId, ProcessManager, TerminationResult},
    process_manager_trait::{RunnerProcessManager, RunnerProcessManagerFactory},
};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use crate::windows_process_manager::WindowsProcessManager;

/// Windows implementation of the RunnerProcessManager trait
/// 
/// This implementation composes the existing WindowsProcessManager for low-level operations
/// while providing high-level orchestration capabilities including process tracking,
/// configuration handling, and emergency cleanup.
pub struct WindowsRunnerProcessManager {
    /// The underlying platform-specific process manager
    platform_manager: Arc<dyn ProcessManager + Send + Sync>,
    /// Thread-safe tracking of active processes
    active_processes: Arc<Mutex<HashMap<ProcessId, String>>>,
    /// Stored runner configuration
    runner_config: RunnerConfig,
}

#[async_trait]
impl RunnerProcessManager for WindowsRunnerProcessManager {
    async fn new(config: &RunnerConfig) -> Result<Self> {
        let platform_manager: Arc<dyn ProcessManager + Send + Sync> = 
            Arc::new(WindowsProcessManager::new());
        
        Ok(Self {
            platform_manager,
            active_processes: Arc::new(Mutex::new(HashMap::new())),
            runner_config: config.clone(),
        })
    }

    async fn start_server(&self) -> Result<Box<dyn ProcessHandle>> {
        let command = &self.runner_config.command;
        let args = &self.runner_config.args;
        let working_dir = self.runner_config.working_directory.as_ref().map(|p| p.as_path().to_str()).flatten();
        let env = &self.runner_config.env;

        // Use the platform manager to spawn the server process
        let handle = self.platform_manager
            .spawn_process(command, args, working_dir, env)
            .await
            .with_context(|| format!("Failed to start server with command: {}", command))?;

        // Track the server process
        if let Some(pid) = handle.get_pid() {
            let mut active = self.active_processes.lock().unwrap();
            active.insert(pid, format!("server:{}", command));
        }

        Ok(handle)
    }

    async fn spawn_process(
        &self,
        command: &str,
        args: &[String],
        working_dir: Option<&str>,
        env: Option<&HashMap<String, String>>,
    ) -> Result<Box<dyn ProcessHandle>> {
        // Use empty environment if none provided
        let default_env = HashMap::new();
        let env_map = env.unwrap_or(&default_env);

        // Delegate to the platform manager
        let handle = self.platform_manager
            .spawn_process(command, args, working_dir, env_map)
            .await
            .with_context(|| format!("Failed to spawn process: {}", command))?;

        // Track the process
        if let Some(pid) = handle.get_pid() {
            let mut active = self.active_processes.lock().unwrap();
            active.insert(pid, command.to_string());
        }

        Ok(handle)
    }

    async fn cleanup(&self) -> Result<()> {
        // Get a snapshot of tracked processes
        let active_processes = {
            let active = self.active_processes.lock().unwrap();
            active.keys().copied().collect::<Vec<_>>()
        };

        // Terminate all tracked processes using the platform manager
        for pid in active_processes {
            let result = self.platform_manager.terminate_process_tree(pid).await;
            match result {
                TerminationResult::Success => {
                    tracing::info!("Successfully terminated process tree for PID {}", pid.0);
                }
                TerminationResult::ProcessNotFound => {
                    tracing::info!("Process {} already terminated", pid.0);
                }
                other => {
                    tracing::warn!("Failed to terminate process {}: {:?}", pid.0, other);
                }
            }
        }

        // Clear the tracking map
        self.active_processes.lock().unwrap().clear();

        // Cleanup the platform manager
        self.platform_manager.cleanup().await
    }

    fn active_process_count(&self) -> usize {
        self.active_processes.lock().unwrap().len()
    }

    fn get_tracked_processes(&self) -> Vec<(ProcessId, String)> {
        let active = self.active_processes.lock().unwrap();
        active.iter().map(|(pid, cmd)| (*pid, cmd.clone())).collect()
    }
}

impl Drop for WindowsRunnerProcessManager {
    fn drop(&mut self) {
        // Emergency cleanup using Windows taskkill
        let active_processes = {
            let active = self.active_processes.lock().unwrap();
            active.keys().copied().collect::<Vec<_>>()
        };

        if !active_processes.is_empty() {
            tracing::warn!(
                "Emergency cleanup: terminating {} processes during drop", 
                active_processes.len()
            );

            for pid in active_processes {
                // Use taskkill with force flag for emergency cleanup
                let result = std::process::Command::new("taskkill")
                    .args(&["/F", "/T", "/PID", &pid.0.to_string()])
                    .output();

                match result {
                    Ok(output) => {
                        if !output.status.success() {
                            tracing::warn!(
                                "Failed to kill process {} during drop: {}",
                                pid.0,
                                String::from_utf8_lossy(&output.stderr)
                            );
                        }
                    }
                    Err(e) => {
                        tracing::error!("Failed to execute taskkill for process {} during drop: {}", pid.0, e);
                    }
                }
            }
        }
    }
}

/// Factory for creating Windows RunnerProcessManager instances
pub struct WindowsRunnerProcessManagerFactory;

#[async_trait]
impl RunnerProcessManagerFactory for WindowsRunnerProcessManagerFactory {
    type Manager = WindowsRunnerProcessManager;

    async fn create_process_manager(config: &RunnerConfig) -> Result<Self::Manager> {
        WindowsRunnerProcessManager::new(config).await
    }

    fn platform_name() -> &'static str {
        "windows"
    }
}

#[cfg(test)]
mod tests {

    
    // Note: Tests will be added in a future task as requested by the user
    // This module is here to show the structure for when tests are implemented
}