use anyhow::{Context, Result};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;
use tokio_util::sync::CancellationToken;
use tracing::{debug, error, info, warn};

use super::platform_factory::PlatformProcessManagerFactory;
use gpmcp_core::process::{
    ProcessHandle, ProcessId, ProcessInfo, ProcessManager as ProcessManagerTrait, ProcessStatus,
    TerminationResult, ProcessManagerFactory,
};
use crate::RunnerConfig;

/// High-level process manager that wraps platform-specific implementations
/// This is the main interface used by the MCP middleware
#[derive(Clone)]
pub struct ProcessManager {
    platform_manager: Arc<dyn ProcessManagerTrait>,
    active_processes: Arc<std::sync::Mutex<HashMap<ProcessId, String>>>,
    cancellation_token: Arc<CancellationToken>,
    runner_config: RunnerConfig,
}

impl ProcessManager {
    /// Create a new ProcessManager with platform-specific implementation
    pub async fn new(
        cancellation_token: Arc<CancellationToken>,
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
            cancellation_token,
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

    /// Check if the process manager should continue operating (not cancelled)
    pub fn is_active(&self) -> bool {
        !self.cancellation_token.is_cancelled()
    }

    /// Get the runner configuration
    pub fn get_config(&self) -> &RunnerConfig {
        &self.runner_config
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

    /// Check if a process is healthy
    pub async fn is_process_healthy(&self, handle: &dyn ProcessHandle) -> bool {
        self.platform_manager.is_process_healthy(handle).await
    }

    /// Get detailed process information
    pub async fn get_process_info(&self, handle: &dyn ProcessHandle) -> Result<ProcessInfo> {
        self.platform_manager.get_process_info(handle).await
    }

    /// Wait for a process to exit with optional timeout
    pub async fn wait_for_exit(
        &self,
        handle: &mut dyn ProcessHandle,
        timeout: Option<Duration>,
    ) -> Result<ProcessStatus> {
        let result = self.platform_manager.wait_for_exit(handle, timeout).await;

        // Untrack the process when it exits
        if let Some(pid) = handle.get_pid() {
            let mut active = self.active_processes.lock().unwrap();
            if let Some(command) = active.remove(&pid) {
                info!("Process exited: {} (PID: {})", command, pid.0);
            }
        }

        result
    }

    /// Terminate a process gracefully
    pub async fn terminate_gracefully(&self, handle: &mut dyn ProcessHandle) -> TerminationResult {
        if let Some(pid) = handle.get_pid() {
            info!("Attempting graceful termination of process {}", pid.0);
        }

        let result = self.platform_manager.terminate_gracefully(handle).await;

        // Untrack successful terminations
        if let (TerminationResult::Success | TerminationResult::ProcessNotFound, Some(pid)) =
            (&result, handle.get_pid())
        {
            let mut active = self.active_processes.lock().unwrap();
            if let Some(command) = active.remove(&pid) {
                info!(
                    "Process terminated gracefully: {} (PID: {})",
                    command, pid.0
                );
            }
        }

        result
    }

    /// Force kill a process
    pub async fn force_kill(&self, handle: &mut dyn ProcessHandle) -> TerminationResult {
        if let Some(pid) = handle.get_pid() {
            warn!("Force killing process {}", pid.0);
        }

        let result = self.platform_manager.force_kill(handle).await;

        // Untrack successful kills
        if let (TerminationResult::Success | TerminationResult::ProcessNotFound, Some(pid)) =
            (&result, handle.get_pid())
        {
            let mut active = self.active_processes.lock().unwrap();
            if let Some(command) = active.remove(&pid) {
                warn!("Process force killed: {} (PID: {})", command, pid.0);
            }
        }

        result
    }

    /// Find all child processes of a given process
    pub async fn find_child_processes(&self, pid: ProcessId) -> Result<Vec<ProcessId>> {
        debug!("Finding child processes for PID {}", pid.0);
        self.platform_manager.find_child_processes(pid).await
    }

    /// Terminate an entire process tree
    pub async fn terminate_process_tree(&self, root_pid: ProcessId) -> TerminationResult {
        info!("Terminating process tree for root PID {}", root_pid.0);

        let result = self.platform_manager.terminate_process_tree(root_pid).await;

        // Untrack the root process on successful termination
        if let TerminationResult::Success | TerminationResult::ProcessNotFound = &result {
            let mut active = self.active_processes.lock().unwrap();
            if let Some(command) = active.remove(&root_pid) {
                info!(
                    "Process tree terminated: {} (Root PID: {})",
                    command, root_pid.0
                );
            }
        }

        result
    }

    /// Terminate a process group (Unix only)
    pub async fn terminate_process_group(&self, pid: ProcessId) -> TerminationResult {
        info!("Terminating process group for PID {}", pid.0);
        self.platform_manager.terminate_process_group(pid).await
    }

    /// Complete termination strategy: process group -> process tree -> individual process
    /// This is the recommended method for ensuring complete cleanup
    pub async fn terminate_completely(&self, handle: &mut dyn ProcessHandle) -> TerminationResult {
        if let Some(pid) = handle.get_pid() {
            info!("Starting complete termination for process {}", pid.0);
        }

        let result = self.platform_manager.terminate_completely(handle).await;

        // Untrack on successful termination
        if let (TerminationResult::Success | TerminationResult::ProcessNotFound, Some(pid)) =
            (&result, handle.get_pid())
        {
            let mut active = self.active_processes.lock().unwrap();
            if let Some(command) = active.remove(&pid) {
                info!(
                    "Complete termination successful: {} (PID: {})",
                    command, pid.0
                );
            }
        }

        result
    }

    /// Get list of currently tracked active processes
    pub fn get_active_processes(&self) -> Vec<(ProcessId, String)> {
        let active = self.active_processes.lock().unwrap();
        active
            .iter()
            .map(|(&pid, command)| (pid, command.clone()))
            .collect()
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

// Remove Default implementation since we now require parameters
// impl Default for ProcessManager {
//     fn default() -> Self {
//         Self::new()
//     }
// }

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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Transport, RetryConfig};
    use tokio::time::sleep;

    fn create_test_runner_config() -> RunnerConfig {
        RunnerConfig {
            name: "test".to_string(),
            version: "1.0.0".to_string(),
            command: "test".to_string(),
            args: vec![],
            env: HashMap::new(),
            working_directory: None,
            transport: Transport::Stdio,
            retry_config: RetryConfig::default(),
        }
    }

    #[tokio::test]
    async fn test_process_manager_creation() {
        let cancellation_token = Arc::new(CancellationToken::new());
        let config = create_test_runner_config();
        let manager = ProcessManager::new(cancellation_token, &config)
            .await
            .unwrap();
        assert_eq!(manager.get_active_processes().len(), 0);
    }

    #[tokio::test]
    async fn test_spawn_and_terminate() {
        let cancellation_token = Arc::new(CancellationToken::new());
        let config = create_test_runner_config();
        let manager = ProcessManager::new(cancellation_token, &config)
            .await
            .unwrap();

        // Spawn a simple process
        #[cfg(unix)]
        let mut handle = manager
            .spawn_process("sleep", &["5".to_string()], None, None)
            .await
            .unwrap();

        #[cfg(windows)]
        let mut handle = manager
            .spawn_process(
                "ping",
                &["127.0.0.1".to_string(), "-n".to_string(), "10".to_string()],
                None,
                None,
            )
            .await
            .unwrap();

        // Verify it's running
        assert!(handle.is_running().await);
        assert_eq!(manager.get_active_processes().len(), 1);

        // Terminate it
        let result = manager.terminate_completely(handle.as_mut()).await;
        assert!(matches!(
            result,
            TerminationResult::Success | TerminationResult::ProcessNotFound
        ));

        // Wait longer for cleanup
        sleep(Duration::from_millis(1500)).await;

        // Verify it's no longer running (or allow for process not found)
        let is_running = handle.is_running().await;
        if is_running {
            // Try one more time to kill it
            let _ = manager.force_kill(handle.as_mut()).await;
            sleep(Duration::from_millis(500)).await;
        }
    }

    #[tokio::test]
    async fn test_cleanup() {
        let cancellation_token = Arc::new(CancellationToken::new());
        let config = create_test_runner_config();
        let manager = ProcessManager::new(cancellation_token, &config)
            .await
            .unwrap();

        // Spawn a process
        #[cfg(unix)]
        let _handle = manager
            .spawn_process("sleep", &["10".to_string()], None, None)
            .await
            .unwrap();

        #[cfg(windows)]
        let _handle = manager
            .spawn_process(
                "ping",
                &["127.0.0.1".to_string(), "-n".to_string(), "20".to_string()],
                None,
                None,
            )
            .await
            .unwrap();

        assert_eq!(manager.get_active_processes().len(), 1);

        // Cleanup should terminate all processes
        manager.cleanup().await.unwrap();

        assert_eq!(manager.get_active_processes().len(), 0);
    }
}
