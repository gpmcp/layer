use anyhow::{Context, Result};
use std::collections::HashMap;
use std::sync::Arc;
use tracing::{debug, error, info, warn};

use super::platform_factory::PlatformProcessManagerFactory;
use gpmcp_core::process::{
    ProcessHandle, ProcessId, ProcessManager as ProcessManagerTrait,
    TerminationResult, ProcessManagerFactory,
};
use crate::RunnerConfig;

/// High-level process manager that wraps platform-specific implementations
/// This is the main interface used by the MCP middleware
#[derive(Clone)]
pub struct ProcessManager {
    platform_manager: Arc<dyn ProcessManagerTrait>,
    active_processes: Arc<std::sync::Mutex<HashMap<ProcessId, String>>>,
    runner_config: RunnerConfig,
}

impl ProcessManager {
    /// Create a new ProcessManager with platform-specific implementation
    pub async fn new(
        runner_config: &RunnerConfig,
    ) -> Result<Self> {
        let platform_manager = PlatformProcessManagerFactory::create_process_manager();
        info!(
            "Created ProcessManager with platform: {}",
            PlatformProcessManagerFactory::platform_name()
        );

        Ok(Self {
            platform_manager: Arc::from(platform_manager),
            active_processes: Arc::new(std::sync::Mutex::new(HashMap::new())),
            runner_config: runner_config.clone(),
        })
    }

    /// Start the server process from the runner configuration
    pub async fn start_server(&self) -> Result<Box<dyn ProcessHandle>> {
        info!("Starting server process from configuration");

        let config = &self.runner_config;
        let working_dir = config.working_directory.as_ref().and_then(|p| p.to_str());

        self.spawn_process(
            &config.command,
            &config.args,
            working_dir,
            Some(&config.env),
        )
        .await
    }

    /// Spawn a new process and track it
    pub async fn spawn_process(
        &self,
        command: &str,
        args: &[String],
        working_dir: Option<&str>,
        env: Option<&HashMap<String, String>>,
    ) -> Result<Box<dyn ProcessHandle>> {
        let env = env.cloned().unwrap_or_default();

        debug!("Spawning process: {} with args: {:?}", command, args);

        let handle = self
            .platform_manager
            .spawn_process(command, args, working_dir, &env)
            .await
            .with_context(|| format!("Failed to spawn process: {command}"))?;

        // Track the process
        if let Some(pid) = handle.get_pid() {
            let mut active = self.active_processes.lock().unwrap();
            active.insert(pid, command.to_string());
            info!("Tracking new process: {} (PID: {})", command, pid.0);
        }

        Ok(handle)
    }

    /// Restart the process manager (useful for retry logic)
    pub async fn restart(&mut self) -> Result<()> {
        info!("Restarting ProcessManager");

        // First cleanup existing processes
        self.cleanup().await?;

        // Create a new platform manager
        let platform_manager = PlatformProcessManagerFactory::create_process_manager();
        self.platform_manager = Arc::from(platform_manager);

        info!("ProcessManager restarted successfully");
        Ok(())
    }

    /// Cleanup all tracked processes and resources
    pub async fn cleanup(&self) -> Result<()> {
        info!("Starting ProcessManager cleanup");

        let active_processes = {
            let active = self.active_processes.lock().unwrap();
            active.keys().copied().collect::<Vec<_>>()
        };

        if !active_processes.is_empty() {
            warn!("Cleaning up {} active processes", active_processes.len());

            for pid in active_processes {
                match self.platform_manager.terminate_process_tree(pid).await {
                    TerminationResult::Success | TerminationResult::ProcessNotFound => {
                        debug!("Successfully cleaned up process {}", pid.0);
                    }
                    result => {
                        error!("Failed to cleanup process {}: {:?}", pid.0, result);
                    }
                }
            }
        }

        // Clear tracking
        {
            let mut active = self.active_processes.lock().unwrap();
            active.clear();
        }

        // Platform-specific cleanup
        self.platform_manager.cleanup().await?;

        info!("ProcessManager cleanup completed");
        Ok(())
    }
}

// Implement Drop to ensure cleanup on drop
impl Drop for ProcessManager {
    fn drop(&mut self) {
        let active_processes = {
            let active = self.active_processes.lock().unwrap();
            active.keys().copied().collect::<Vec<_>>()
        };

        if !active_processes.is_empty() {
            warn!(
                "ProcessManager dropped with {} active processes - attempting emergency cleanup",
                active_processes.len()
            );

            // Note: We can't use async in Drop, so we'll do synchronous cleanup
            // This is a best-effort emergency cleanup
            for pid in active_processes {
                #[cfg(unix)]
                {
                    use nix::sys::signal::{self, Signal};
                    use nix::unistd::Pid as NixPid;

                    let nix_pid = NixPid::from_raw(pid.0 as i32);
                    if let Err(e) = signal::kill(nix_pid, Signal::SIGTERM) {
                        warn!("Emergency cleanup failed for process {}: {}", pid.0, e);
                    }
                }

                #[cfg(windows)]
                {
                    use std::process::Command;

                    if let Err(e) = Command::new("taskkill")
                        .args(&["/F", "/T", "/PID", &pid.0.to_string()])
                        .output()
                    {
                        warn!("Emergency cleanup failed for process {}: {}", pid.0, e);
                    }
                }
            }
        }
    }
}
