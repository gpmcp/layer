use anyhow::Result;
use async_trait::async_trait;
use std::time::Duration;

/// Represents a process identifier
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ProcessId(pub u32);

impl From<u32> for ProcessId {
    fn from(pid: u32) -> Self {
        Self(pid)
    }
}

impl From<ProcessId> for u32 {
    fn from(pid: ProcessId) -> Self {
        pid.0
    }
}

/// Represents the status of a process
#[derive(Debug, Clone)]
pub enum ProcessStatus {
    Running,
    Exited(std::process::ExitStatus),
    Terminated,
    Unknown,
}

/// Information about a running process
#[derive(Debug, Clone)]
pub struct ProcessInfo {
    pub pid: ProcessId,
    pub status: ProcessStatus,
    pub command: String,
    pub args: Vec<String>,
}

/// Result of a termination attempt
#[derive(Debug, Clone)]
pub enum TerminationResult {
    Success,
    ProcessNotFound,
    AccessDenied,
    Timeout,
    Failed(String),
}

/// Core trait for process lifecycle management
#[async_trait]
pub trait ProcessLifecycle: Send + Sync {
    /// Spawn a new process with the given command and arguments
    async fn spawn_process(
        &self,
        command: &str,
        args: &[String],
        working_dir: Option<&str>,
        env: &std::collections::HashMap<String, String>,
    ) -> Result<Box<dyn ProcessHandle>>;

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
