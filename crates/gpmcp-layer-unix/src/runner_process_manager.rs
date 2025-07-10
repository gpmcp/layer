use crate::UnixProcessHandle;
use crate::unix_process_manager::UnixProcessManager;
use anyhow::{Context, Result};
use async_trait::async_trait;
use gpmcp_layer_core::config::RunnerConfig;
use gpmcp_layer_core::process::{ProcessHandle, ProcessId, ProcessManager, TerminationResult};
use gpmcp_layer_core::process_manager_trait::{RunnerProcessManager, RunnerProcessManagerFactory};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use gpmcp_layer_core::layer::{LayerStdErr, LayerStdOut};

/// Unix implementation of the RunnerProcessManager trait
///
/// This implementation composes the existing UnixProcessManager for low-level operations
/// while providing high-level orchestration capabilities including process tracking,
/// configuration handling, and emergency cleanup.
pub struct UnixRunnerProcessManager {
    /// The underlying platform-specific process manager
    platform_manager: Arc<UnixProcessManager>,
    /// Thread-safe tracking of active processes
    active_processes: Arc<Mutex<HashMap<ProcessId, String>>>,
    /// Stored runner configuration
    runner_config: RunnerConfig,
}

#[async_trait]
impl RunnerProcessManager for UnixRunnerProcessManager {
    type Handle = UnixProcessHandle;
    fn new(config: &RunnerConfig) -> Self {
        let platform_manager: Arc<UnixProcessManager> = Arc::new(UnixProcessManager::new());

        Self {
            platform_manager,
            active_processes: Arc::new(Mutex::new(HashMap::new())),
            runner_config: config.clone(),
        }
    }

    async fn start_server(&self, out: LayerStdOut, err: LayerStdErr) -> Result<Self::Handle> {
        let command = &self.runner_config.command;
        let args = &self.runner_config.args;
        let working_dir = self
            .runner_config
            .working_directory
            .as_ref()
            .and_then(|p| p.as_path().to_str());
        let env = &self.runner_config.env;

        // Use the platform manager to spawn the server process
        let handle = self
            .platform_manager
            .spawn_process(command, args, working_dir, env, out, err)
            .await
            .with_context(|| format!("Failed to start server with command: {command}"))?;

        // Track the server process
        if let Some(pid) = handle.get_pid() {
            let mut active = self.active_processes.lock().unwrap();
            active.insert(pid, format!("server:{command}"));
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
                    tracing::info!("Successfully terminated process tree for PID {}", pid);
                }
                TerminationResult::ProcessNotFound => {
                    tracing::info!("Process {} already terminated", pid);
                }
                other => {
                    tracing::warn!("Failed to terminate process {}: {:?}", pid, other);
                }
            }
        }

        // Clear the tracking map
        self.active_processes.lock().unwrap().clear();

        // Cleanup the platform manager
        self.platform_manager.cleanup().await
    }
}

impl Drop for UnixRunnerProcessManager {
    fn drop(&mut self) {
        // Emergency cleanup using Unix signals
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
                use nix::sys::signal::{self, Signal};
                use nix::unistd::Pid as NixPid;

                let nix_pid = NixPid::from_raw(pid as i32);

                // Try SIGTERM first
                if let Err(e) = signal::kill(nix_pid, Signal::SIGTERM) {
                    tracing::warn!(
                        "Failed to send SIGTERM to process {} during drop: {}",
                        pid,
                        e
                    );

                    // If SIGTERM fails, try SIGKILL
                    if let Err(e) = signal::kill(nix_pid, Signal::SIGKILL) {
                        tracing::error!(
                            "Failed to send SIGKILL to process {} during drop: {}",
                            pid,
                            e
                        );
                    }
                }
            }
        }
    }
}

/// Factory for creating Unix RunnerProcessManager instances
pub struct UnixRunnerProcessManagerFactory;

#[async_trait]
impl RunnerProcessManagerFactory for UnixRunnerProcessManagerFactory {
    type Manager = UnixRunnerProcessManager;

    fn create_process_manager(config: &RunnerConfig) -> Self::Manager {
        UnixRunnerProcessManager::new(config)
    }
}
