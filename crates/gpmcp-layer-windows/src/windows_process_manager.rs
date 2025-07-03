use anyhow::Result;
use async_trait::async_trait;
use std::collections::HashMap;
use std::time::Duration;
use sysinfo::System;
use tokio::process::{Child, Command};
use tracing::{info, warn};

use gpmcp_layer_core::*;

/// Windows-specific process handle implementation
pub struct WindowsProcessHandle {
    child: Child,
    command: String,
    args: Vec<String>,
}

impl WindowsProcessHandle {
    pub fn new(child: Child, command: String, args: Vec<String>) -> Self {
        Self {
            child,
            command,
            args,
        }
    }
}

#[async_trait]
impl ProcessHandle for WindowsProcessHandle {
    fn get_pid(&self) -> Option<ProcessId> {
        self.child.id().map(ProcessId::from)
    }

    fn get_command(&self) -> &str {
        &self.command
    }

    fn get_args(&self) -> &[String] {
        &self.args
    }

    async fn is_running(&self) -> bool {
        // On Windows, we can check if the process is running by attempting to get its status
        if let Some(_pid) = self.get_pid() {
            // Use sysinfo to check if process exists
            let mut system = System::new();
            system.refresh_processes_specifics(
                sysinfo::ProcessesToUpdate::All,
                true,
                sysinfo::ProcessRefreshKind::everything(),
            );
            if system.processes().iter().any(|(p, _)| p.as_u32() == _pid.0) {
                info!(pid=%_pid.0, "Windows process is no longer running");
                false
            } else {
                true
            }
        } else {
            warn!("Windows process handle has no PID - process may have exited");
            false
        }
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

    async fn kill(&mut self) -> Result<()> {
        self.child
            .kill()
            .await
            .map_err(|e| anyhow::anyhow!("Failed to kill process: {}", e))
    }
}

/// Windows-specific process manager with process tree management
pub struct WindowsProcessManager {
    system: std::sync::Mutex<System>,
}

impl Default for WindowsProcessManager {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl ProcessLifecycle for WindowsProcessManager {
    async fn spawn_process(
        &self,
        command: &str,
        args: &[String],
        working_dir: Option<&str>,
        env: &HashMap<String, String>,
    ) -> Result<Box<dyn ProcessHandle>, std::io::Error> {
        let mut cmd = Command::new(command);
        cmd.args(args);

        // Set working directory
        if let Some(dir) = working_dir {
            cmd.current_dir(dir);
        }

        // Set environment variables
        for (key, value) in env {
            cmd.env(key, value);
        }

        // On Windows, we create processes without a console window for background execution
        // This avoids the annoying console popup while maintaining process management capabilities
        #[cfg(windows)]
        {
            // CREATE_NO_WINDOW (0x08000000) - Creates a process without a console window
            // This is better than CREATE_NEW_CONSOLE for background processes
            cmd.creation_flags(0x08000000); // CREATE_NO_WINDOW
        }

        let child = cmd.spawn()?;

        // Log successful process creation
        if let Some(pid) = child.id() {
            info!(
                pid = %pid,
                command = %command,
                args = ?args,
                "Spawned Windows process"
            );
        }

        Ok(Box::new(WindowsProcessHandle::new(
            child,
            command.to_string(),
            args.to_vec(),
        )))
    }

    async fn is_process_healthy(&self, handle: &dyn ProcessHandle) -> bool {
        handle.is_running().await
    }

    async fn get_process_info(&self, handle: &dyn ProcessHandle) -> Result<ProcessInfo> {
        let pid = handle
            .get_pid()
            .ok_or_else(|| anyhow::anyhow!("Process has no PID"))?;

        let status = if handle.is_running().await {
            ProcessStatus::Running
        } else {
            ProcessStatus::Terminated
        };

        Ok(ProcessInfo {
            pid,
            status,
            command: handle.get_command().to_string(),
            args: handle.get_args().to_vec(),
        })
    }

    async fn wait_for_exit(
        &self,
        handle: &mut dyn ProcessHandle,
        timeout: Option<Duration>,
    ) -> Result<ProcessStatus> {
        match timeout {
            Some(duration) => tokio::time::timeout(duration, handle.wait())
                .await
                .map_err(|_| anyhow::anyhow!("Timeout waiting for process exit"))?,
            None => handle.wait().await,
        }
    }
}

#[async_trait]
impl ProcessTermination for WindowsProcessManager {
    async fn terminate_gracefully(&self, handle: &mut dyn ProcessHandle) -> TerminationResult {
        // On Windows, we'll use taskkill with /T flag for graceful termination
        if let Some(pid) = handle.get_pid() {
            match self.taskkill(pid.0, false).await {
                Ok(true) => {
                    info!(pid=%pid.0, "Successfully sent graceful termination to process");
                    TerminationResult::Success
                }
                Ok(false) => {
                    warn!(pid=%pid.0, "Process not found for graceful termination");
                    TerminationResult::ProcessNotFound
                }
                Err(e) => {
                    warn!(pid=%pid.0, error=%e, "Failed to gracefully terminate process");
                    TerminationResult::Failed(format!("Graceful termination failed: {e}"))
                }
            }
        } else {
            TerminationResult::ProcessNotFound
        }
    }

    async fn force_kill(&self, handle: &mut dyn ProcessHandle) -> TerminationResult {
        if let Some(pid) = handle.get_pid() {
            match self.taskkill(pid.0, true).await {
                Ok(true) => {
                    info!(pid=%pid.0, "Successfully force killed process");
                    // Also call handle's kill method for cleanup
                    if let Err(e) = handle.kill().await {
                        warn!(error=%e, "Handle kill cleanup failed");
                    }
                    TerminationResult::Success
                }
                Ok(false) => {
                    info!(pid=%pid.0, "Process not found for force kill");
                    TerminationResult::ProcessNotFound
                }
                Err(e) => {
                    warn!(pid=%pid.0, error=%e, "Failed to force kill process");
                    TerminationResult::Failed(format!("Force kill failed: {e}"))
                }
            }
        } else {
            TerminationResult::ProcessNotFound
        }
    }

    async fn find_child_processes(&self, parent_pid: ProcessId) -> Result<Vec<ProcessId>> {
        let mut system = self.system.lock().unwrap();
        system.refresh_processes_specifics(
            sysinfo::ProcessesToUpdate::All,
            true,
            sysinfo::ProcessRefreshKind::everything(),
        );

        let mut children = Vec::new();
        Self::find_children_recursive(&system, parent_pid.0, &mut children);

        Ok(children.into_iter().map(ProcessId::from).collect())
    }

    async fn terminate_process_tree(&self, root_pid: ProcessId) -> TerminationResult {
        info!(root_pid=%root_pid.0, "Terminating process tree for root PID");

        // On Windows, we can use taskkill with /T flag to kill process trees
        match self.taskkill_tree(root_pid.0).await {
            Ok(true) => {
                info!(
                    root_pid = %root_pid.0,
                    "Successfully terminated process tree for PID"
                );
                TerminationResult::Success
            }
            Ok(false) => {
                info!(root_pid=%root_pid.0, "Process tree for PID not found");
                TerminationResult::ProcessNotFound
            }
            Err(e) => {
                warn!(root_pid=%root_pid.0, e=%e, "Failed to terminate process tree for PID");

                // Fallback: manual process tree termination
                let children = match self.find_child_processes(root_pid).await {
                    Ok(children) => children,
                    Err(e) => {
                        warn!(root_pid=%root_pid.0, e=%e, "Failed to find child processes for PID");

                        return TerminationResult::Failed(format!(
                            "Failed to enumerate children: {e}"
                        ));
                    }
                };

                if !children.is_empty() {
                    info!(
                        count = children.len(),
                        "Found child processes to terminate manually"
                    );

                    // Terminate children first (bottom-up approach)
                    for child_pid in children.iter().rev() {
                        match self.terminate_single_process(*child_pid).await {
                            TerminationResult::Success | TerminationResult::ProcessNotFound => {}
                            result => {
                                warn!(
                                    pid = %child_pid.0,
                                    result = ?result,
                                    "Failed to terminate child process"
                                );
                            }
                        }
                    }
                }

                // Finally terminate the root process
                self.terminate_single_process(root_pid).await
            }
        }
    }

    async fn terminate_process_group(&self, _pid: ProcessId) -> TerminationResult {
        // Windows doesn't have Unix-style process groups
        // Return ProcessNotFound to indicate this method is not supported
        TerminationResult::ProcessNotFound
    }
}

impl WindowsProcessManager {
    /// Use taskkill to terminate a single process
    async fn taskkill(&self, pid: u32, force: bool) -> Result<bool> {
        let pid_string = pid.to_string();
        let mut args = vec!["/PID", &pid_string];
        if force {
            args.push("/F");
        }

        let output = Command::new("taskkill").args(&args).output().await?;

        Ok(output.status.success())
    }

    /// Use taskkill with /T to terminate a process tree
    async fn taskkill_tree(&self, pid: u32) -> Result<bool> {
        let output = Command::new("taskkill")
            .args(["/F", "/T", "/PID", &pid.to_string()])
            .output()
            .await?;

        Ok(output.status.success())
    }

    /// Terminate a single process by PID with escalation
    async fn terminate_single_process(&self, pid: ProcessId) -> TerminationResult {
        // Try graceful termination first
        match self.taskkill(pid.0, false).await {
            Ok(true) => {
                info!(pid=%pid.0, "Sent graceful termination to process");

                // Wait briefly for graceful shutdown
                tokio::time::sleep(Duration::from_millis(1000)).await;

                // Check if process is still running, if so force kill
                let mut system = System::new();
                system.refresh_processes_specifics(
                    sysinfo::ProcessesToUpdate::All,
                    true,
                    sysinfo::ProcessRefreshKind::everything(),
                );

                if system.processes().iter().any(|(p, _)| p.as_u32() == pid.0) {
                    // Process still running, force kill
                    match self.taskkill(pid.0, true).await {
                        Ok(true) => {
                            info!(pid=%pid.0, "Force killed process");
                            TerminationResult::Success
                        }
                        Ok(false) => {
                            info!(pid=%pid.0, "Process already terminated");
                            TerminationResult::Success
                        }
                        Err(e) => {
                            warn!(pid=%pid.0, error=%e, "Failed to force kill process");
                            TerminationResult::Failed(format!("Force kill failed: {e}"))
                        }
                    }
                } else {
                    info!(pid=%pid.0, "Process terminated gracefully");
                    TerminationResult::Success
                }
            }
            Ok(false) => {
                info!(pid=%pid.0, "Process not found (already terminated)");
                TerminationResult::Success
            }
            Err(e) => {
                warn!(pid=%pid.0, error=%e, "Failed to send graceful termination to process");
                TerminationResult::Failed(format!("Graceful termination failed: {e}"))
            }
        }
    }

    /// Recursively find all child processes
    fn find_children_recursive(system: &System, parent_pid: u32, result: &mut Vec<u32>) {
        for (pid, process) in system.processes() {
            #[allow(clippy::collapsible_if)]
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

#[async_trait]
impl ProcessManager for WindowsProcessManager {
    fn new() -> Self {
        info!("Initializing Windows process manager with system monitoring");
        Self {
            system: std::sync::Mutex::new(System::new_all()),
        }
    }
}
