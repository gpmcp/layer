use anyhow::{Context, Result};
use async_trait::async_trait;
use gpmcp_layer_core::layer::{LayerStdErr, LayerStdOut};
use gpmcp_layer_core::process::{ProcessHandle, ProcessId, ProcessManager, TerminationResult};
use gpmcp_layer_core::process_manager_trait::stream;
use std::collections::HashMap;
use std::time::Duration;
use sysinfo::System;
use tokio::process::{Child, Command};
use tracing::{info, warn};

/// Windows-specific process handle implementation
pub struct WindowsProcessHandle {
    child: Child,
    command: String,
}

impl WindowsProcessHandle {
    pub fn new(child: Child, command: String) -> Self {
        Self { child, command }
    }
}

#[async_trait]
impl ProcessHandle for WindowsProcessHandle {
    fn get_pid(&self) -> Option<ProcessId> {
        self.child.id()
    }

    fn get_command(&self) -> &str {
        &self.command
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
        match self.taskkill(pid, false).await {
            Ok(true) => {
                info!("Sent graceful termination to process {}", pid);

                // Wait briefly for graceful shutdown
                tokio::time::sleep(Duration::from_millis(1000)).await;

                // Check if process is still running, if so force kill
                let mut system = System::new();
                system.refresh_processes_specifics(
                    sysinfo::ProcessesToUpdate::All,
                    true,
                    sysinfo::ProcessRefreshKind::everything(),
                );

                if system.processes().iter().any(|(p, _)| p.as_u32() == pid) {
                    // Process still running, force kill
                    match self.taskkill(pid, true).await {
                        Ok(true) => {
                            info!("Force killed process {}", pid);
                            TerminationResult::Success
                        }
                        Ok(false) => {
                            info!("Process {} already terminated", pid);
                            TerminationResult::Success
                        }
                        Err(e) => {
                            warn!("Failed to force kill process {}: {}", pid, e);
                            TerminationResult::Failed(format!("Force kill failed: {e}"))
                        }
                    }
                } else {
                    info!("Process {} terminated gracefully", pid);
                    TerminationResult::Success
                }
            }
            Ok(false) => {
                info!("Process {} not found (already terminated)", pid);
                TerminationResult::Success
            }
            Err(e) => {
                warn!(
                    "Failed to send graceful termination to process {}: {}",
                    pid, e
                );
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
    type Handle = WindowsProcessHandle;

    fn new() -> Self {
        Self {
            system: std::sync::Mutex::new(System::new_all()),
        }
    }

    async fn cleanup(&self) -> Result<()> {
        info!("Windows process manager cleanup completed");
        Ok(())
    }

    async fn find_child_processes(&self, parent_pid: ProcessId) -> Result<Vec<ProcessId>> {
        let mut system = self.system.lock().unwrap();
        system.refresh_processes_specifics(
            sysinfo::ProcessesToUpdate::All,
            true,
            sysinfo::ProcessRefreshKind::everything(),
        );

        let mut children = Vec::new();
        Self::find_children_recursive(&system, parent_pid, &mut children);

        Ok(children.into_iter().collect())
    }

    async fn terminate_process_tree(&self, root_pid: ProcessId) -> TerminationResult {
        info!("Terminating process tree for root PID {}", root_pid);

        // On Windows, we can use taskkill with /T flag to kill process trees
        match self.taskkill_tree(root_pid).await {
            Ok(true) => {
                info!("Successfully terminated process tree for PID {}", root_pid);
                TerminationResult::Success
            }
            Ok(false) => {
                info!("Process tree for PID {} not found", root_pid);
                TerminationResult::ProcessNotFound
            }
            Err(e) => {
                warn!(
                    "Failed to terminate process tree for PID {}: {}",
                    root_pid, e
                );

                // Fallback: manual process tree termination
                let children = match self.find_child_processes(root_pid).await {
                    Ok(children) => children,
                    Err(e) => {
                        warn!("Failed to find child processes for PID {}: {}", root_pid, e);
                        return TerminationResult::Failed(format!(
                            "Failed to enumerate children: {e}"
                        ));
                    }
                };

                if !children.is_empty() {
                    info!(
                        "Found {} child processes to terminate manually",
                        children.len()
                    );

                    // Terminate children first (bottom-up approach)
                    for child_pid in children.iter().rev() {
                        match self.terminate_single_process(*child_pid).await {
                            TerminationResult::Success | TerminationResult::ProcessNotFound => {}
                            result => {
                                warn!(
                                    "Failed to terminate child process {}: {:?}",
                                    child_pid, result
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

    async fn spawn_process(
        &self,
        command: &str,
        args: &[String],
        working_dir: Option<&str>,
        env: &HashMap<String, String>,
        out: LayerStdOut,
        err: LayerStdErr,
    ) -> Result<Self::Handle> {
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

        // Configure stdout and stderr to be captured
        cmd.stdout(std::process::Stdio::piped());
        cmd.stderr(std::process::Stdio::piped());

        // On Windows, we create processes without a console window for background execution
        // This avoids the annoying console popup while maintaining process management capabilities
        #[cfg(windows)]
        {
            // CREATE_NO_WINDOW (0x08000000) - Creates a process without a console window
            // This is better than CREATE_NEW_CONSOLE for background processes
            cmd.creation_flags(0x08000000); // CREATE_NO_WINDOW
        }

        let mut child = cmd
            .spawn()
            .with_context(|| format!("Failed to spawn process: {command}"))?;

        // Log successful process creation
        if let Some(pid) = child.id() {
            info!(
                "Spawned Windows process: {} (PID: {}) with args: {:?}",
                command, pid, args
            );
        }

        // Capture and stream stdout - the stream function will detect it's stdout and route accordingly
        if let Some(stdout) = child.stdout.take() {
            let out_clone = out.clone();
            let err_clone = err.clone();
            tokio::spawn(async move {
                if let Err(e) = stream(&mut Some(stdout), out_clone, err_clone).await {
                    warn!("Error streaming stdout: {}", e);
                }
            });
        }

        // Capture and stream stderr - the stream function will detect it's stderr and route accordingly
        if let Some(stderr) = child.stderr.take() {
            let out_clone = out.clone();
            let err_clone = err.clone();
            tokio::spawn(async move {
                if let Err(e) = stream(&mut Some(stderr), out_clone, err_clone).await {
                    warn!("Error streaming stderr: {}", e);
                }
            });
        }

        Ok(WindowsProcessHandle::new(child, command.to_string()))
    }
}
