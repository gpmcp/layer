use anyhow::{Context, Result};
use async_trait::async_trait;
use gpmcp_domain::blueprint::ServerDefinition;
use nix::sys::signal::{self, Signal};
use nix::unistd::Pid as NixPid;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;
use sysinfo::System;
use tokio::process::Command;
use tokio::sync::Mutex;
use tokio_util::sync::CancellationToken;
use tracing::{info, warn};

use crate::runner::traits::{
    process_lifecycle::{ProcessCreator, ProcessHandle, ProcessId, ProcessInfo, ProcessManager, ProcessMonitor, ProcessStatus, TokioProcessHandle},
    termination::{ProcessGroupTerminator, ProcessKiller, ProcessTreeTerminator, TerminationResult, DefaultTerminationStrategy},
};

/// Unix-specific process group terminator using nix signals
pub struct UnixProcessGroupTerminator;

#[async_trait]
impl ProcessGroupTerminator for UnixProcessGroupTerminator {
    async fn terminate_process_group(&self, pid: ProcessId) -> TerminationResult {
        let pgid = NixPid::from_raw(pid.0 as i32);
        
        // Try SIGTERM first for graceful shutdown
        match signal::killpg(pgid, Signal::SIGTERM) {
            Ok(()) => {
                info!("Sent SIGTERM to process group {}", pid.0);
                
                // Wait for graceful shutdown
                tokio::time::sleep(self.graceful_timeout()).await;
                
                // Check if processes are still running, if so use SIGKILL
                match signal::killpg(pgid, Signal::SIGKILL) {
                    Ok(()) => {
                        info!("Sent SIGKILL to process group {}", pid.0);
                        TerminationResult::Success
                    }
                    Err(nix::errno::Errno::ESRCH) => {
                        info!("Process group {} already terminated", pid.0);
                        TerminationResult::Success
                    }
                    Err(e) => {
                        warn!("Failed to send SIGKILL to process group {}: {}", pid.0, e);
                        TerminationResult::Failed(format!("SIGKILL failed: {}", e))
                    }
                }
            }
            Err(nix::errno::Errno::ESRCH) => {
                info!("Process group {} not found (already terminated)", pid.0);
                TerminationResult::Success
            }
            Err(nix::errno::Errno::EPERM) => {
                warn!("Permission denied to terminate process group {}", pid.0);
                TerminationResult::AccessDenied
            }
            Err(e) => {
                warn!("Failed to send SIGTERM to process group {}: {}", pid.0, e);
                TerminationResult::Failed(format!("SIGTERM failed: {}", e))
            }
        }
    }
    
    fn supports_process_groups(&self) -> bool {
        true
    }
}

/// Unix-specific process tree terminator
pub struct UnixProcessTreeTerminator;

#[async_trait]
impl ProcessTreeTerminator for UnixProcessTreeTerminator {
    async fn find_child_processes(&self, parent_pid: ProcessId) -> Result<Vec<ProcessId>> {
        let mut system = System::new_all();
        system.refresh_processes_specifics(
            sysinfo::ProcessesToUpdate::All,
            true,
            sysinfo::ProcessRefreshKind::new(),
        );
        
        let mut child_processes = Vec::new();
        Self::find_children_recursive(&system, parent_pid.0, &mut child_processes);
        
        Ok(child_processes.into_iter().map(ProcessId::from).collect())
    }
    
    async fn terminate_process_tree(&self, root_pid: ProcessId) -> TerminationResult {
        info!("Enumerating and killing process tree for PID {}", root_pid.0);
        
        let child_processes = match self.find_child_processes(root_pid).await {
            Ok(children) => children,
            Err(e) => {
                warn!("Failed to enumerate child processes for PID {}: {}", root_pid.0, e);
                return TerminationResult::Failed(format!("Failed to enumerate children: {}", e));
            }
        };
        
        if child_processes.is_empty() {
            info!("No child processes found for PID {}", root_pid.0);
        } else {
            info!("Found {} child processes to terminate", child_processes.len());
            
            // Kill children first (bottom-up approach)
            for pid in child_processes.iter().rev() {
                match self.terminate_single_process(*pid).await {
                    TerminationResult::Success => {},
                    TerminationResult::ProcessNotFound => {},
                    result => warn!("Failed to terminate child process {}: {:?}", pid.0, result),
                }
            }
        }
        
        // Finally terminate the root process
        self.terminate_single_process(root_pid).await
    }
    
    async fn terminate_single_process(&self, pid: ProcessId) -> TerminationResult {
        let nix_pid = NixPid::from_raw(pid.0 as i32);
        
        // Try SIGTERM first
        match signal::kill(nix_pid, Signal::SIGTERM) {
            Ok(()) => {
                info!("Sent SIGTERM to process {}", pid.0);
                
                // Wait briefly for graceful shutdown
                tokio::time::sleep(self.termination_delay()).await;
                
                // Then SIGKILL if still running
                match signal::kill(nix_pid, Signal::SIGKILL) {
                    Ok(()) => {
                        info!("Sent SIGKILL to process {}", pid.0);
                        TerminationResult::Success
                    }
                    Err(nix::errno::Errno::ESRCH) => {
                        info!("Process {} already terminated", pid.0);
                        TerminationResult::Success
                    }
                    Err(e) => {
                        warn!("Failed to kill process {}: {}", pid.0, e);
                        TerminationResult::Failed(format!("SIGKILL failed: {}", e))
                    }
                }
            }
            Err(nix::errno::Errno::ESRCH) => {
                info!("Process {} not found (already terminated)", pid.0);
                TerminationResult::Success
            }
            Err(nix::errno::Errno::EPERM) => {
                warn!("Permission denied to terminate process {}", pid.0);
                TerminationResult::AccessDenied
            }
            Err(e) => {
                warn!("Failed to send SIGTERM to process {}: {}", pid.0, e);
                TerminationResult::Failed(format!("SIGTERM failed: {}", e))
            }
        }
    }
}

impl UnixProcessTreeTerminator {
    fn find_children_recursive(system: &System, parent_pid: u32, result: &mut Vec<u32>) {
        for (pid, process) in system.processes() {
            if let Some(ppid) = process.parent() {
                if ppid.as_u32() == parent_pid {
                    let child_pid = pid.as_u32();
                    // Recursively find grandchildren first
                    Self::find_children_recursive(system, child_pid, result);
                    // Then add this child
                    result.push(child_pid);
                }
            }
        }
    }
}

/// Unix-specific process killer
pub struct UnixProcessKiller;

#[async_trait]
impl ProcessKiller for UnixProcessKiller {
    async fn graceful_kill(&self, process: &mut dyn ProcessHandle) -> TerminationResult {
        if let Some(pid) = process.pid() {
            let nix_pid = NixPid::from_raw(pid.0 as i32);
            
            match signal::kill(nix_pid, Signal::SIGTERM) {
                Ok(()) => {
                    info!("Sent SIGTERM to process {}", pid.0);
                    TerminationResult::Success
                }
                Err(nix::errno::Errno::ESRCH) => {
                    info!("Process {} not found (already terminated)", pid.0);
                    TerminationResult::ProcessNotFound
                }
                Err(nix::errno::Errno::EPERM) => {
                    warn!("Permission denied to terminate process {}", pid.0);
                    TerminationResult::AccessDenied
                }
                Err(e) => {
                    warn!("Failed to send SIGTERM to process {}: {}", pid.0, e);
                    TerminationResult::Failed(format!("SIGTERM failed: {}", e))
                }
            }
        } else {
            TerminationResult::ProcessNotFound
        }
    }
    
    async fn force_kill(&self, process: &mut dyn ProcessHandle) -> TerminationResult {
        if let Some(pid) = process.pid() {
            let nix_pid = NixPid::from_raw(pid.0 as i32);
            
            match signal::kill(nix_pid, Signal::SIGKILL) {
                Ok(()) => {
                    info!("Sent SIGKILL to process {}", pid.0);
                    // Also call the process handle's kill method for cleanup
                    if let Err(e) = process.kill().await {
                        warn!("Process handle kill failed: {}", e);
                    }
                    TerminationResult::Success
                }
                Err(nix::errno::Errno::ESRCH) => {
                    info!("Process {} not found (already terminated)", pid.0);
                    TerminationResult::ProcessNotFound
                }
                Err(nix::errno::Errno::EPERM) => {
                    warn!("Permission denied to kill process {}", pid.0);
                    TerminationResult::AccessDenied
                }
                Err(e) => {
                    warn!("Failed to send SIGKILL to process {}: {}", pid.0, e);
                    TerminationResult::Failed(format!("SIGKILL failed: {}", e))
                }
            }
        } else {
            TerminationResult::ProcessNotFound
        }
    }
}

/// Unix-specific process creator
pub struct UnixProcessCreator;

#[async_trait]
impl ProcessCreator for UnixProcessCreator {
    async fn create_process(
        &self,
        server_definition: &ServerDefinition,
        _cancellation_token: &CancellationToken,
    ) -> Result<Box<dyn ProcessHandle>> {
        let command_runner = match &server_definition.runner {
            gpmcp_domain::blueprint::Runner::Stdio { command_runner } => command_runner,
            gpmcp_domain::blueprint::Runner::Sse { command_runner, .. } => command_runner,
        };

        let mut cmd = Command::new(&command_runner.command);
        cmd.args(&command_runner.args);

        // Set working directory if specified
        if !command_runner.workdir.is_empty() {
            cmd.current_dir(&command_runner.workdir);
        }

        // Add environment variables
        for (key, value) in &server_definition.env {
            cmd.env(key, value);
        }

        // Create a new process group for better process tree management
        cmd.process_group(0);

        let child = cmd
            .spawn()
            .with_context(|| format!("Failed to start command: {}", command_runner.command))?;

        // Log the PID if available
        match child.id() {
            Some(pid) => {
                info!(
                    "Started process: {} with args: {:?}, PID: {}",
                    command_runner.command, command_runner.args, pid
                );
            }
            None => {
                warn!(
                    "Started process: {} with args: {:?}, but PID is not available",
                    command_runner.command, command_runner.args
                );
            }
        }

        Ok(Box::new(TokioProcessHandle::new(
            child,
            command_runner.command.clone(),
            command_runner.args.clone(),
        )))
    }
}

/// Unix-specific process monitor
pub struct UnixProcessMonitor;

#[async_trait]
impl ProcessMonitor for UnixProcessMonitor {
    async fn is_healthy(&self, process: &dyn ProcessHandle) -> bool {
        if let Some(pid) = process.pid() {
            // Check if process is still running by sending signal 0
            let nix_pid = NixPid::from_raw(pid.0 as i32);
            match signal::kill(nix_pid, None) {
                Ok(()) => true,
                Err(nix::errno::Errno::ESRCH) => false,
                Err(_) => false, // Other errors also indicate unhealthy state
            }
        } else {
            false
        }
    }
    
    async fn get_process_info(&self, process: &dyn ProcessHandle) -> Result<ProcessInfo> {
        let pid = process.pid().ok_or_else(|| anyhow::anyhow!("No PID available"))?;
        
        let status = if self.is_healthy(process).await {
            ProcessStatus::Running
        } else {
            ProcessStatus::Terminated
        };
        
        Ok(ProcessInfo {
            pid,
            status,
            command: process.command().to_string(),
            args: process.args().to_vec(),
        })
    }
    
    async fn wait_for_exit(
        &self,
        process: &mut dyn ProcessHandle,
        timeout: Option<Duration>,
    ) -> Result<ProcessStatus> {
        match timeout {
            Some(duration) => {
                tokio::time::timeout(duration, process.wait())
                    .await
                    .map_err(|_| anyhow::anyhow!("Timeout waiting for process exit"))?
            }
            None => process.wait().await,
        }
    }
}

/// Unix-specific process manager that combines all Unix implementations
pub struct UnixProcessManager {
    creator: UnixProcessCreator,
    monitor: UnixProcessMonitor,
    restart_counts: Arc<Mutex<HashMap<u32, u32>>>,
}

impl UnixProcessManager {
    pub fn new() -> Self {
        Self {
            creator: UnixProcessCreator,
            monitor: UnixProcessMonitor,
            restart_counts: Arc::new(Mutex::new(HashMap::new())),
        }
    }
}

#[async_trait]
impl ProcessCreator for UnixProcessManager {
    async fn create_process(
        &self,
        server_definition: &ServerDefinition,
        cancellation_token: &CancellationToken,
    ) -> Result<Box<dyn ProcessHandle>> {
        self.creator.create_process(server_definition, cancellation_token).await
    }
}

#[async_trait]
impl ProcessMonitor for UnixProcessManager {
    async fn is_healthy(&self, process: &dyn ProcessHandle) -> bool {
        self.monitor.is_healthy(process).await
    }
    
    async fn get_process_info(&self, process: &dyn ProcessHandle) -> Result<ProcessInfo> {
        self.monitor.get_process_info(process).await
    }
    
    async fn wait_for_exit(
        &self,
        process: &mut dyn ProcessHandle,
        timeout: Option<Duration>,
    ) -> Result<ProcessStatus> {
        self.monitor.wait_for_exit(process, timeout).await
    }
}

#[async_trait]
impl ProcessManager for UnixProcessManager {
    async fn restart_process(
        &self,
        process: &mut dyn ProcessHandle,
        server_definition: &ServerDefinition,
        cancellation_token: &CancellationToken,
    ) -> Result<Box<dyn ProcessHandle>> {
        // Increment restart count
        if let Some(pid) = process.pid() {
            let mut counts = self.restart_counts.lock().await;
            let count = counts.entry(pid.0).or_insert(0);
            *count += 1;
            info!("Process restart count for PID {}: {}", pid.0, *count);
        }
        
        // Terminate the old process
        if let Err(e) = process.terminate().await {
            warn!("Failed to terminate process during restart: {}", e);
        }
        
        // Wait a bit before restarting
        tokio::time::sleep(Duration::from_millis(500)).await;
        
        // Create new process
        self.create_process(server_definition, cancellation_token).await
    }
    
    async fn get_restart_count(&self, process: &dyn ProcessHandle) -> u32 {
        if let Some(pid) = process.pid() {
            let counts = self.restart_counts.lock().await;
            *counts.get(&pid.0).unwrap_or(&0)
        } else {
            0
        }
    }
    
    async fn cleanup(&self) -> Result<()> {
        let mut counts = self.restart_counts.lock().await;
        counts.clear();
        info!("Unix process manager cleanup completed");
        Ok(())
    }
}

/// Type alias for Unix termination strategy
pub type UnixTerminationStrategy = DefaultTerminationStrategy<
    UnixProcessGroupTerminator,
    UnixProcessTreeTerminator,
    UnixProcessKiller,
>;

impl UnixTerminationStrategy {
    pub fn new_unix() -> Self {
        DefaultTerminationStrategy::new(
            Some(UnixProcessGroupTerminator),
            UnixProcessTreeTerminator,
            UnixProcessKiller,
        )
    }
}

impl Default for UnixTerminationStrategy {
    fn default() -> Self {
        Self::new_unix()
    }
}