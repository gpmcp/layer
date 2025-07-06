use anyhow::Result;
use async_trait::async_trait;
use std::collections::HashMap;
use std::time::Duration;

/// Unique identifier for a process
pub type ProcessId = u32;

/// Status of a process after termination
#[derive(Debug, Clone, PartialEq)]
pub enum ProcessStatus {
    /// Process is currently running
    Running,
    /// Process exited normally with the given exit code
    Success(i32),
    /// Process exited with status information
    Exited(std::process::ExitStatus),
    /// Process was terminated by a signal (Unix) or forcibly terminated (Windows)
    Terminated,
    /// Process failed to start or encountered an error
    Failed(String),
    /// Process status is unknown
    Unknown,
}

/// Result of a process termination operation
#[derive(Debug, Clone, PartialEq)]
pub enum TerminationResult {
    /// Process was successfully terminated
    Success,
    /// Process was not found (already exited)
    ProcessNotFound,
    /// Permission denied (insufficient privileges)
    PermissionDenied,
    /// Access denied (same as PermissionDenied, for backward compatibility)
    AccessDenied,
    /// Operation timed out
    Timeout,
    /// Other error occurred
    Error(String),
    /// Operation failed with specific error message
    Failed(String),
}

/// Information about a running process
#[derive(Debug, Clone)]
pub struct ProcessInfo {
    pub pid: ProcessId,
    pub command: String,
    pub args: Vec<String>,
    pub status: ProcessStatus,
}

/// Error types for process operations
#[derive(Debug, thiserror::Error)]
pub enum ProcessError {
    #[error("Failed to spawn process: {0}")]
    SpawnFailed(String),
    #[error("Process not found: {0}")]
    ProcessNotFound(ProcessId),
    #[error("Permission denied: {0}")]
    PermissionDenied(String),
    #[error("Operation timed out")]
    Timeout,
    #[error("IO error: {0}")]
    IoError(#[from] std::io::Error),
    #[error("Other error: {0}")]
    Other(String),
}

/// Core trait for process lifecycle management with associated types for better performance
#[async_trait]
pub trait ProcessLifecycle: Send + Sync {
    /// The type of process handle this lifecycle manager produces
    type Handle: ProcessHandle;

    /// Spawn a new process with the given command and arguments
    async fn spawn_process(
        &self,
        command: &str,
        args: &[String],
        working_dir: Option<&str>,
        env: &HashMap<String, String>,
    ) -> Result<Self::Handle>;

    /// Check if a process is still running and healthy
    async fn is_process_healthy(&self, handle: &dyn ProcessHandle) -> bool;

    /// Get detailed information about a process
    async fn get_process_info(&self, handle: &dyn ProcessHandle) -> Result<ProcessInfo>;

    /// Wait for a process to exit with optional timeout
    async fn wait_for_exit(
        &self,
        handle: &mut dyn ProcessHandle,
        timeout: Option<Duration>,
    ) -> Result<ProcessStatus>;
}

/// Trait for comprehensive process termination including process trees
#[async_trait]
pub trait ProcessTermination: Send + Sync {
    /// Terminate a single process gracefully (SIGTERM on Unix)
    async fn terminate_gracefully(&self, handle: &mut dyn ProcessHandle) -> TerminationResult;

    /// Force kill a single process (SIGKILL on Unix)
    async fn force_kill(&self, handle: &mut dyn ProcessHandle) -> TerminationResult;

    /// Find all child processes of a given process
    async fn find_child_processes(&self, pid: ProcessId) -> Result<Vec<ProcessId>>;

    /// Terminate an entire process tree (parent and all descendants)
    async fn terminate_process_tree(&self, root_pid: ProcessId) -> TerminationResult;

    /// Terminate a process group (Unix only, returns ProcessNotFound on Windows)
    async fn terminate_process_group(&self, pid: ProcessId) -> TerminationResult;

    /// Complete termination strategy: process group -> process tree -> individual process
    async fn terminate_completely(&self, handle: &mut dyn ProcessHandle) -> TerminationResult {
        if let Some(pid) = handle.get_pid() {
            // Step 1: Try process group termination (Unix only)
            match self.terminate_process_group(pid).await {
                TerminationResult::Success => return TerminationResult::Success,
                TerminationResult::ProcessNotFound => {
                    // Process group not found, continue to next step
                }
                _ => {
                    // Process group termination failed, try process tree
                }
            }

            // Step 2: Try process tree termination
            match self.terminate_process_tree(pid).await {
                TerminationResult::Success => return TerminationResult::Success,
                TerminationResult::ProcessNotFound => {
                    // Process tree not found, continue to individual termination
                }
                _ => {
                    // Process tree termination failed, continue to individual termination
                }
            }
        }

        // Step 3: Individual process termination with escalation
        match self.terminate_gracefully(handle).await {
            TerminationResult::Success => {
                // Wait a bit to see if process exits gracefully
                tokio::time::sleep(Duration::from_millis(1000)).await;

                // If still running, force kill
                if handle.is_running().await {
                    self.force_kill(handle).await
                } else {
                    TerminationResult::Success
                }
            }
            TerminationResult::ProcessNotFound => TerminationResult::Success,
            _ => {
                // Graceful termination failed, try force kill
                self.force_kill(handle).await
            }
        }
    }
}

/// Trait representing a handle to a running process
#[async_trait]
pub trait ProcessHandle: Send + Sync {
    /// Get the process ID (None if process has exited)
    fn get_pid(&self) -> Option<ProcessId>;

    /// Get the command that started this process
    fn get_command(&self) -> &str;

    /// Get the arguments passed to this process
    fn get_args(&self) -> &[String];

    /// Check if the process is still running (non-blocking)
    async fn is_running(&self) -> bool;

    /// Try to get exit status without blocking
    async fn try_wait(&mut self) -> Result<Option<ProcessStatus>>;

    /// Wait for the process to exit (blocking)
    async fn wait(&mut self) -> Result<ProcessStatus>;

    /// Kill the process (platform-specific implementation)
    async fn kill(&mut self) -> Result<()>;
}

/// High-level process manager trait that combines lifecycle and termination
#[async_trait]
pub trait ProcessManager: ProcessLifecycle + ProcessTermination {
    /// Create a new process manager instance
    fn new() -> Self
    where
        Self: Sized;

    /// Cleanup any resources held by the process manager
    async fn cleanup(&self) -> Result<()>;
}

/// Factory trait for creating platform-specific process managers
pub trait ProcessManagerFactory {
    /// The type of process manager this factory creates
    type Manager: ProcessManager;

    /// Create a process manager for the current platform
    fn create_process_manager() -> Self::Manager;

    /// Get the platform name for logging and debugging
    fn platform_name() -> &'static str;
}

/// Implementation of ProcessHandle for boxed trait objects to enable associated type usage
#[async_trait]
impl ProcessHandle for Box<dyn ProcessHandle> {
    fn get_pid(&self) -> Option<ProcessId> {
        (**self).get_pid()
    }

    fn get_command(&self) -> &str {
        (**self).get_command()
    }

    fn get_args(&self) -> &[String] {
        (**self).get_args()
    }

    async fn is_running(&self) -> bool {
        (**self).is_running().await
    }

    async fn try_wait(&mut self) -> Result<Option<ProcessStatus>> {
        (**self).try_wait().await
    }

    async fn wait(&mut self) -> Result<ProcessStatus> {
        (**self).wait().await
    }

    async fn kill(&mut self) -> Result<()> {
        (**self).kill().await
    }
}
