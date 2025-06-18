use anyhow::{Context, Result};
use async_trait::async_trait;
use gpmcp_domain::blueprint::ServerDefinition;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;
use sysinfo::System;
use tokio::process::Command;
use tokio::sync::Mutex;
use tokio_util::sync::CancellationToken;
use tracing::{error, info, warn};

use crate::runner::traits::{
    process_lifecycle::{
        ProcessCreator, ProcessHandle, ProcessId, ProcessInfo, ProcessManager, ProcessMonitor,
        ProcessStatus, TokioProcessHandle,
    },
    termination::{
        DefaultTerminationStrategy, ProcessGroupTerminator, ProcessKiller, ProcessTreeTerminator,
        TerminationResult,
    },
};

/// Windows-specific process group terminator (limited support)
pub struct WindowsProcessGroupTerminator;

#[async_trait]
impl ProcessGroupTerminator for WindowsProcessGroupTerminator {
    async fn terminate_process_group(&self, pid: ProcessId) -> TerminationResult {
        // Windows doesn't have direct process group termination like Unix
        // We'll try to terminate the process tree instead
        warn!(
            "Process group termination not directly supported on Windows, falling back to process tree termination"
        );

        let tree_terminator = WindowsProcessTreeTerminator;
        tree_terminator.terminate_process_tree(pid).await
    }

    fn supports_process_groups(&self) -> bool {
        false // Windows doesn't have Unix-style process groups
    }
}

/// Windows-specific process tree terminator
pub struct WindowsProcessTreeTerminator;

#[async_trait]
impl ProcessTreeTerminator for WindowsProcessTreeTerminator {
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
        info!(
            "Enumerating and killing process tree for PID {}",
            root_pid.0
        );

        let child_processes = match self.find_child_processes(root_pid).await {
            Ok(children) => children,
            Err(e) => {
                warn!(
                    "Failed to enumerate child processes for PID {}: {}",
                    root_pid.0, e
                );
                return TerminationResult::Failed(format!("Failed to enumerate children: {}", e));
            }
        };

        if child_processes.is_empty() {
            info!("No child processes found for PID {}", root_pid.0);
        } else {
            info!(
                "Found {} child processes to terminate",
                child_processes.len()
            );

            // Kill children first (bottom-up approach)
            for pid in child_processes.iter().rev() {
                match self.terminate_single_process(*pid).await {
                    TerminationResult::Success => {}
                    TerminationResult::ProcessNotFound => {}
                    result => warn!("Failed to terminate child process {}: {:?}", pid.0, result),
                }
            }
        }

        // Finally terminate the root process
        self.terminate_single_process(root_pid).await
    }

    async fn terminate_single_process(&self, pid: ProcessId) -> TerminationResult {
        // Use taskkill command for Windows process termination
        let output = tokio::process::Command::new("taskkill")
            .args(["/F", "/PID", &pid.0.to_string()])
            .output()
            .await;

        match output {
            Ok(output) => {
                if output.status.success() {
                    info!("Successfully terminated process {} on Windows", pid.0);
                    TerminationResult::Success
                } else {
                    let stderr = String::from_utf8_lossy(&output.stderr);
                    if stderr.contains("not found") || stderr.contains("not exist") {
                        info!("Process {} not found (already terminated)", pid.0);
                        TerminationResult::ProcessNotFound
                    } else if stderr.contains("Access is denied") {
                        warn!("Access denied when terminating process {}", pid.0);
                        TerminationResult::AccessDenied
                    } else {
                        warn!(
                            "Failed to terminate process {} on Windows: {}",
                            pid.0, stderr
                        );
                        TerminationResult::Failed(format!("taskkill failed: {}", stderr))
                    }
                }
            }
            Err(e) => {
                error!("Error running taskkill for process {}: {}", pid.0, e);
                TerminationResult::Failed(format!("Failed to run taskkill: {}", e))
            }
        }
    }
}

impl WindowsProcessTreeTerminator {
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

/// Windows-specific process killer
pub struct WindowsProcessKiller;

#[async_trait]
impl ProcessKiller for WindowsProcessKiller {
    async fn graceful_kill(&self, process: &mut dyn ProcessHandle) -> TerminationResult {
        // On Windows, we'll try the process handle's terminate method first
        // which should be more graceful than taskkill /F
        match process.terminate().await {
            Ok(()) => {
                if let Some(pid) = process.pid() {
                    info!("Gracefully terminated process {}", pid.0);
                }
                TerminationResult::Success
            }
            Err(e) => {
                if let Some(pid) = process.pid() {
                    warn!("Failed to gracefully terminate process {}: {}", pid.0, e);

                    // Try normal taskkill without /F flag for graceful termination
                    let output = tokio::process::Command::new("taskkill")
                        .args(["/PID", &pid.0.to_string()])
                        .output()
                        .await;

                    match output {
                        Ok(output) => {
                            if output.status.success() {
                                info!("Gracefully terminated process {} with taskkill", pid.0);
                                TerminationResult::Success
                            } else {
                                let stderr = String::from_utf8_lossy(&output.stderr);
                                if stderr.contains("not found") || stderr.contains("not exist") {
                                    TerminationResult::ProcessNotFound
                                } else if stderr.contains("Access is denied") {
                                    TerminationResult::AccessDenied
                                } else {
                                    TerminationResult::Failed(format!(
                                        "Graceful taskkill failed: {}",
                                        stderr
                                    ))
                                }
                            }
                        }
                        Err(e) => TerminationResult::Failed(format!(
                            "Failed to run graceful taskkill: {}",
                            e
                        )),
                    }
                } else {
                    TerminationResult::ProcessNotFound
                }
            }
        }
    }

    async fn force_kill(&self, process: &mut dyn ProcessHandle) -> TerminationResult {
        // First try the process handle's kill method
        if let Err(e) = process.kill().await {
            warn!("Process handle kill failed: {}", e);
        }

        if let Some(pid) = process.pid() {
            // Use taskkill with /F flag for force termination
            let output = tokio::process::Command::new("taskkill")
                .args(["/F", "/PID", &pid.0.to_string()])
                .output()
                .await;

            match output {
                Ok(output) => {
                    if output.status.success() {
                        info!("Force killed process {} on Windows", pid.0);
                        TerminationResult::Success
                    } else {
                        let stderr = String::from_utf8_lossy(&output.stderr);
                        if stderr.contains("not found") || stderr.contains("not exist") {
                            info!("Process {} not found (already terminated)", pid.0);
                            TerminationResult::ProcessNotFound
                        } else if stderr.contains("Access is denied") {
                            warn!("Access denied when force killing process {}", pid.0);
                            TerminationResult::AccessDenied
                        } else {
                            warn!(
                                "Failed to force kill process {} on Windows: {}",
                                pid.0, stderr
                            );
                            TerminationResult::Failed(format!("Force taskkill failed: {}", stderr))
                        }
                    }
                }
                Err(e) => {
                    error!("Error running force taskkill for process {}: {}", pid.0, e);
                    TerminationResult::Failed(format!("Failed to run force taskkill: {}", e))
                }
            }
        } else {
            TerminationResult::ProcessNotFound
        }
    }
}

/// Windows-specific process creator
pub struct WindowsProcessCreator;

#[async_trait]
impl ProcessCreator for WindowsProcessCreator {
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

        // On Windows, we don't have Unix-style process groups, but we can create
        // a new process group using CREATE_NEW_PROCESS_GROUP
        // This is handled automatically by tokio::process::Command on Windows

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

/// Windows-specific process monitor
pub struct WindowsProcessMonitor;

#[async_trait]
impl ProcessMonitor for WindowsProcessMonitor {
    async fn is_healthy(&self, process: &dyn ProcessHandle) -> bool {
        if let Some(pid) = process.pid() {
            // Use tasklist to check if process is still running
            let output = tokio::process::Command::new("tasklist")
                .args(["/FI", &format!("PID eq {}", pid.0), "/FO", "CSV"])
                .output()
                .await;

            match output {
                Ok(output) => {
                    if output.status.success() {
                        let stdout = String::from_utf8_lossy(&output.stdout);
                        // If process exists, tasklist will include it in the output
                        stdout.lines().count() > 1 // Header + process line
                    } else {
                        false
                    }
                }
                Err(_) => false,
            }
        } else {
            false
        }
    }

    async fn get_process_info(&self, process: &dyn ProcessHandle) -> Result<ProcessInfo> {
        let pid = process
            .pid()
            .ok_or_else(|| anyhow::anyhow!("No PID available"))?;

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
            Some(duration) => tokio::time::timeout(duration, process.wait())
                .await
                .map_err(|_| anyhow::anyhow!("Timeout waiting for process exit"))?,
            None => process.wait().await,
        }
    }
}

/// Windows-specific process manager that combines all Windows implementations
pub struct WindowsProcessManager {
    creator: WindowsProcessCreator,
    monitor: WindowsProcessMonitor,
    restart_counts: Arc<Mutex<HashMap<u32, u32>>>,
}

impl WindowsProcessManager {
    pub fn new() -> Self {
        Self {
            creator: WindowsProcessCreator,
            monitor: WindowsProcessMonitor,
            restart_counts: Arc::new(Mutex::new(HashMap::new())),
        }
    }
}

#[async_trait]
impl ProcessCreator for WindowsProcessManager {
    async fn create_process(
        &self,
        server_definition: &ServerDefinition,
        cancellation_token: &CancellationToken,
    ) -> Result<Box<dyn ProcessHandle>> {
        self.creator
            .create_process(server_definition, cancellation_token)
            .await
    }
}

#[async_trait]
impl ProcessMonitor for WindowsProcessManager {
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
impl ProcessManager for WindowsProcessManager {
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
        self.create_process(server_definition, cancellation_token)
            .await
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
        info!("Windows process manager cleanup completed");
        Ok(())
    }
}

/// Type alias for Windows termination strategy
pub type WindowsTerminationStrategy = DefaultTerminationStrategy<
    WindowsProcessGroupTerminator,
    WindowsProcessTreeTerminator,
    WindowsProcessKiller,
>;

impl WindowsTerminationStrategy {
    pub fn new_windows() -> Self {
        DefaultTerminationStrategy::new(
            Some(WindowsProcessGroupTerminator),
            WindowsProcessTreeTerminator,
            WindowsProcessKiller,
        )
    }
}

impl Default for WindowsTerminationStrategy {
    fn default() -> Self {
        Self::new_windows()
    }
}
