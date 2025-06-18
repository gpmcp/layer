use anyhow::Result;
use async_trait::async_trait;
use std::time::Duration;
use super::process_lifecycle::{ProcessHandle, ProcessId};

/// Result of a termination attempt
#[derive(Debug, Clone)]
pub enum TerminationResult {
    Success,
    ProcessNotFound,
    AccessDenied,
    Timeout,
    Failed(String),
}

/// Trait for terminating process groups (Step 1 of three-step termination)
#[async_trait]
pub trait ProcessGroupTerminator: Send + Sync {
    /// Attempt to terminate an entire process group
    /// Returns true if the process group was successfully terminated
    async fn terminate_process_group(&self, pid: ProcessId) -> TerminationResult;
    
    /// Check if process group termination is supported on this platform
    fn supports_process_groups(&self) -> bool;
    
    /// Get the recommended timeout for graceful process group termination
    fn graceful_timeout(&self) -> Duration {
        Duration::from_millis(2000)
    }
}

/// Trait for terminating process trees recursively (Step 2 of three-step termination)
#[async_trait]
pub trait ProcessTreeTerminator: Send + Sync {
    /// Find all child processes of a given parent process
    async fn find_child_processes(&self, parent_pid: ProcessId) -> Result<Vec<ProcessId>>;
    
    /// Recursively terminate a process tree (children first, then parent)
    async fn terminate_process_tree(&self, root_pid: ProcessId) -> TerminationResult;
    
    /// Terminate a single process by PID
    async fn terminate_single_process(&self, pid: ProcessId) -> TerminationResult;
    
    /// Get the recommended timeout between termination attempts
    fn termination_delay(&self) -> Duration {
        Duration::from_millis(500)
    }
}

/// Trait for force-killing processes (Step 3 of three-step termination)
#[async_trait]
pub trait ProcessKiller: Send + Sync {
    /// Attempt graceful termination (SIGTERM on Unix, polite close on Windows)
    async fn graceful_kill(&self, process: &mut dyn ProcessHandle) -> TerminationResult;
    
    /// Force kill the process immediately (SIGKILL on Unix, TerminateProcess on Windows)
    async fn force_kill(&self, process: &mut dyn ProcessHandle) -> TerminationResult;
    
    /// Kill with escalating signals/methods
    async fn escalating_kill(&self, process: &mut dyn ProcessHandle) -> TerminationResult {
        // Default implementation: try graceful first, then force
        match self.graceful_kill(process).await {
            TerminationResult::Success => TerminationResult::Success,
            TerminationResult::ProcessNotFound => TerminationResult::ProcessNotFound,
            _ => {
                // Wait a bit before force kill
                tokio::time::sleep(Duration::from_millis(1000)).await;
                self.force_kill(process).await
            }
        }
    }
}

/// High-level trait that implements the complete three-step termination strategy
#[async_trait]
pub trait TerminationStrategy: Send + Sync {
    /// Execute the complete three-step termination process:
    /// 1. Try to kill the process group (if supported)
    /// 2. Recursively kill the process tree
    /// 3. Use graceful then forceful termination on the main process
    async fn terminate_with_strategy(&self, process: &mut dyn ProcessHandle) -> TerminationResult;
    
    /// Get the components used for each step (for testing and customization)
    fn process_group_terminator(&self) -> Option<&dyn ProcessGroupTerminator>;
    fn process_tree_terminator(&self) -> &dyn ProcessTreeTerminator;
    fn process_killer(&self) -> &dyn ProcessKiller;
}

/// Default implementation of the three-step termination strategy
pub struct DefaultTerminationStrategy<G, T, K> 
where
    G: ProcessGroupTerminator,
    T: ProcessTreeTerminator,
    K: ProcessKiller,
{
    group_terminator: Option<G>,
    tree_terminator: T,
    process_killer: K,
}

impl<G, T, K> DefaultTerminationStrategy<G, T, K>
where
    G: ProcessGroupTerminator,
    T: ProcessTreeTerminator,
    K: ProcessKiller,
{
    pub fn new(group_terminator: Option<G>, tree_terminator: T, process_killer: K) -> Self {
        Self {
            group_terminator,
            tree_terminator,
            process_killer,
        }
    }
    
    pub fn without_process_groups(tree_terminator: T, process_killer: K) -> DefaultTerminationStrategy<G, T, K> {
        Self {
            group_terminator: None,
            tree_terminator,
            process_killer,
        }
    }
}

#[async_trait]
impl<G, T, K> TerminationStrategy for DefaultTerminationStrategy<G, T, K>
where
    G: ProcessGroupTerminator + Send + Sync,
    T: ProcessTreeTerminator + Send + Sync,
    K: ProcessKiller + Send + Sync,
{
    async fn terminate_with_strategy(&self, process: &mut dyn ProcessHandle) -> TerminationResult {
        let pid = match process.pid() {
            Some(pid) => pid,
            None => return TerminationResult::ProcessNotFound,
        };
        
        tracing::info!("Starting three-step termination for PID: {}", pid.0);
        
        // Step 1: Try to kill the process group (if supported and available)
        let mut process_group_killed = false;
        if let Some(ref group_terminator) = self.group_terminator {
            if group_terminator.supports_process_groups() {
                match group_terminator.terminate_process_group(pid).await {
                    TerminationResult::Success => {
                        tracing::info!("Process group termination succeeded for PID: {}", pid.0);
                        process_group_killed = true;
                    }
                    TerminationResult::ProcessNotFound => {
                        tracing::info!("Process group not found for PID: {}", pid.0);
                        process_group_killed = true; // Consider this success
                    }
                    result => {
                        tracing::warn!("Process group termination failed for PID: {}: {:?}", pid.0, result);
                    }
                }
            }
        }
        
        // Step 2: If process group kill failed, try recursive process tree termination
        if !process_group_killed {
            match self.tree_terminator.terminate_process_tree(pid).await {
                TerminationResult::Success => {
                    tracing::info!("Process tree termination succeeded for PID: {}", pid.0);
                }
                TerminationResult::ProcessNotFound => {
                    tracing::info!("Process tree not found for PID: {}", pid.0);
                }
                result => {
                    tracing::warn!("Process tree termination failed for PID: {}: {:?}", pid.0, result);
                }
            }
        }
        
        // Step 3: Finally, kill the main process with escalating signals
        match self.process_killer.escalating_kill(process).await {
            TerminationResult::Success => {
                tracing::info!("Process termination completed successfully for PID: {}", pid.0);
                TerminationResult::Success
            }
            TerminationResult::ProcessNotFound => {
                tracing::info!("Process already terminated for PID: {}", pid.0);
                TerminationResult::Success
            }
            result => {
                tracing::error!("Final process termination failed for PID: {}: {:?}", pid.0, result);
                result
            }
        }
    }
    
    fn process_group_terminator(&self) -> Option<&dyn ProcessGroupTerminator> {
        self.group_terminator.as_ref().map(|g| g as &dyn ProcessGroupTerminator)
    }
    
    fn process_tree_terminator(&self) -> &dyn ProcessTreeTerminator {
        &self.tree_terminator
    }
    
    fn process_killer(&self) -> &dyn ProcessKiller {
        &self.process_killer
    }
}