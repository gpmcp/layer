use anyhow::Result;
use async_trait::async_trait;
use std::collections::HashMap;

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
}

/// Trait for comprehensive process termination including process trees
#[async_trait]
pub trait ProcessTermination: Send + Sync {

    /// Find all child processes of a given process
    async fn find_child_processes(&self, pid: ProcessId) -> Result<Vec<ProcessId>>;

    /// Terminate an entire process tree (parent and all descendants)
    async fn terminate_process_tree(&self, root_pid: ProcessId) -> TerminationResult;


}

/// Trait representing a handle to a running process
#[async_trait]
pub trait ProcessHandle: Send + Sync {
    /// Get the process ID (None if process has exited)
    fn get_pid(&self) -> Option<ProcessId>;

    /// Get the command that started this process
    fn get_command(&self) -> &str;

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

    async fn kill(&mut self) -> Result<()> {
        (**self).kill().await
    }
}
