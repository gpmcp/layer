use anyhow::{Context, Result};
use async_trait::async_trait;
use gpmcp_layer_core::process::{ProcessHandle, ProcessId, ProcessManager, TerminationResult};
use std::collections::HashMap;
use std::time::Duration;

use gpmcp_layer_core::layer::{LayerStdErr, LayerStdOut};
use gpmcp_layer_core::process_manager_trait::stream;
use nix::sys::signal::{self, Signal};
use nix::unistd::Pid as NixPid;
use sysinfo::System;
use tokio::process::{Child, Command};
use tracing::{info, warn};

/// Unix-specific process handle implementation
pub struct UnixProcessHandle {
    child: Child,
    command: String,
}

/// Unix-specific process manager with comprehensive process tree management
pub struct UnixProcessManager {
    system: std::sync::Mutex<System>,
}

impl UnixProcessHandle {
    pub fn new(child: Child, command: String) -> Self {
        Self { child, command }
    }
}

#[async_trait]
impl ProcessHandle for UnixProcessHandle {
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
impl Default for UnixProcessManager {
    fn default() -> Self {
        Self::new()
    }
}
impl UnixProcessManager {
    /// Terminate a single process by PID with escalation
    async fn terminate_single_process(&self, pid: ProcessId) -> TerminationResult {
        let nix_pid = NixPid::from_raw(pid as i32);

        // Try SIGTERM first
        match signal::kill(nix_pid, Signal::SIGTERM) {
            Ok(()) => {
                info!("Sent SIGTERM to process {}", pid);

                // Wait briefly for graceful shutdown
                tokio::time::sleep(Duration::from_millis(500)).await;

                // Then SIGKILL if still running
                match signal::kill(nix_pid, Signal::SIGKILL) {
                    Ok(()) => {
                        info!("Sent SIGKILL to process {}", pid);
                        TerminationResult::Success
                    }
                    Err(nix::errno::Errno::ESRCH) => {
                        info!("Process {} already terminated", pid);
                        TerminationResult::Success
                    }
                    Err(e) => {
                        warn!("Failed to kill process {}: {}", pid, e);
                        TerminationResult::Failed(format!("SIGKILL failed: {e}"))
                    }
                }
            }
            Err(nix::errno::Errno::ESRCH) => {
                info!("Process {} not found (already terminated)", pid);
                TerminationResult::Success
            }
            Err(nix::errno::Errno::EPERM) => {
                warn!("Permission denied to terminate process {}", pid);
                TerminationResult::AccessDenied
            }
            Err(e) => {
                warn!("Failed to send SIGTERM to process {}: {e}", pid,);
                TerminationResult::Failed(format!("SIGTERM failed: {e}",))
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
impl ProcessManager for UnixProcessManager {
    fn new() -> Self {
        Self {
            system: std::sync::Mutex::new(System::new_all()),
        }
    }

    async fn cleanup(&self) -> Result<()> {
        info!("Unix process manager cleanup completed");
        Ok(())
    }

    async fn find_child_processes(&self, parent_pid: ProcessId) -> Result<Vec<ProcessId>> {
        let mut system = self.system.lock().unwrap();
        system.refresh_processes_specifics(
            sysinfo::ProcessesToUpdate::All,
            true,
            sysinfo::ProcessRefreshKind::default(),
        );

        let mut children = Vec::new();
        Self::find_children_recursive(&system, parent_pid, &mut children);

        Ok(children.into_iter().collect())
    }

    async fn terminate_process_tree(&self, root_pid: ProcessId) -> TerminationResult {
        info!("Terminating process tree for root PID {}", root_pid);

        // Find all child processes
        let children = match self.find_child_processes(root_pid).await {
            Ok(children) => children,
            Err(e) => {
                warn!("Failed to find child processes for PID {}: {}", root_pid, e);
                return TerminationResult::Failed(format!("Failed to enumerate children: {e}"));
            }
        };

        if children.is_empty() {
            info!("No child processes found for PID {}", root_pid);
        } else {
            info!("Found {} child processes to terminate", children.len());

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
    type Handle = UnixProcessHandle;

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

        // Create new process group for better process tree management
        cmd.process_group(0);

        let mut child = cmd
            .spawn()
            .with_context(|| format!("Failed to spawn process: {command}"))?;

        // Log successful process creation
        if let Some(pid) = child.id() {
            info!(
                "Spawned Unix process: {} (PID: {}) with args: {:?}",
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
        Ok(UnixProcessHandle::new(child, command.to_string()))
    }
}
