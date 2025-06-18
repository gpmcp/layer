use anyhow::Result;
use async_trait::async_trait;
use gpmcp_domain::blueprint::ServerDefinition;
use std::time::Duration;
use tokio::process::Child;
use tokio_util::sync::CancellationToken;

/// Represents a process identifier that can be platform-specific
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
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

/// Trait for creating processes from server definitions
#[async_trait]
pub trait ProcessCreator: Send + Sync {
    /// Create and start a new process based on the server definition
    async fn create_process(
        &self,
        server_definition: &ServerDefinition,
        cancellation_token: &CancellationToken,
    ) -> Result<Box<dyn ProcessHandle>>;
}

/// Trait for monitoring process health and status
#[async_trait]
pub trait ProcessMonitor: Send + Sync {
    /// Check if the process is still running and healthy
    async fn is_healthy(&self, process: &dyn ProcessHandle) -> bool;
    
    /// Get current process information
    async fn get_process_info(&self, process: &dyn ProcessHandle) -> Result<ProcessInfo>;
    
    /// Wait for process to exit with optional timeout
    async fn wait_for_exit(
        &self,
        process: &mut dyn ProcessHandle,
        timeout: Option<Duration>,
    ) -> Result<ProcessStatus>;
}

/// Trait representing a handle to a running process
#[async_trait]
pub trait ProcessHandle: Send + Sync {
    /// Get the process ID
    fn pid(&self) -> Option<ProcessId>;
    
    /// Get the command that started this process
    fn command(&self) -> &str;
    
    /// Get the arguments passed to this process
    fn args(&self) -> &[String];
    
    /// Check if the process is still running (non-blocking)
    async fn try_wait(&mut self) -> Result<Option<ProcessStatus>>;
    
    /// Wait for the process to exit
    async fn wait(&mut self) -> Result<ProcessStatus>;
    
    /// Terminate the process (implementation-specific strategy)
    async fn terminate(&mut self) -> Result<()>;
    
    /// Force kill the process immediately
    async fn kill(&mut self) -> Result<()>;
}

/// High-level trait that combines process creation, monitoring, and management
#[async_trait]
pub trait ProcessManager: ProcessCreator + ProcessMonitor + Send + Sync {
    /// Restart a process using the same configuration
    async fn restart_process(
        &self,
        process: &mut dyn ProcessHandle,
        server_definition: &ServerDefinition,
        cancellation_token: &CancellationToken,
    ) -> Result<Box<dyn ProcessHandle>>;
    
    /// Get the restart count for tracking process restarts
    async fn get_restart_count(&self, process: &dyn ProcessHandle) -> u32;
    
    /// Clean up resources associated with process management
    async fn cleanup(&self) -> Result<()>;
}

/// Wrapper around tokio::process::Child that implements ProcessHandle
pub struct TokioProcessHandle {
    child: Child,
    command: String,
    args: Vec<String>,
}

impl TokioProcessHandle {
    pub fn new(child: Child, command: String, args: Vec<String>) -> Self {
        Self { child, command, args }
    }
}

#[async_trait]
impl ProcessHandle for TokioProcessHandle {
    fn pid(&self) -> Option<ProcessId> {
        self.child.id().map(ProcessId::from)
    }
    
    fn command(&self) -> &str {
        &self.command
    }
    
    fn args(&self) -> &[String] {
        &self.args
    }
    
    async fn try_wait(&mut self) -> Result<Option<ProcessStatus>> {
        match self.child.try_wait()? {
            Some(status) => Ok(Some(ProcessStatus::Exited(status))),
            None => Ok(None),
        }
    }
    
    async fn wait(&mut self) -> Result<ProcessStatus> {
        let status = self.child.wait().await?;
        Ok(ProcessStatus::Exited(status))
    }
    
    async fn terminate(&mut self) -> Result<()> {
        // Default implementation - platform-specific implementations will override this
        self.child.kill().await?;
        Ok(())
    }
    
    async fn kill(&mut self) -> Result<()> {
        self.child.kill().await?;
        Ok(())
    }
}