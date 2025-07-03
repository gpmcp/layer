use anyhow::Result;
use async_trait::async_trait;
use gpmcp_layer_core::{
    ProcessHandle, ProcessId, ProcessInfo, ProcessLifecycle, ProcessManager, ProcessStatus,
    ProcessTermination, TerminationResult,
};
use std::collections::HashMap;
use std::time::Duration;
use tracing::{info, warn};

#[cfg(unix)]
mod unix_impl {
    use super::*;
    use nix::sys::signal::{self, Signal};
    use nix::unistd::Pid as NixPid;
    use sysinfo::System;
    use tokio::process::{Child, Command};

    /// Unix-specific process handle implementation
    pub struct UnixProcessHandle {
        child: Child,
        command: String,
        args: Vec<String>,
    }

    impl UnixProcessHandle {
        pub fn new(child: Child, command: String, args: Vec<String>) -> Self {
            Self {
                child,
                command,
                args,
            }
        }
    }

    #[async_trait]
    impl ProcessHandle for UnixProcessHandle {
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
            if let Some(pid) = self.get_pid() {
                let nix_pid = NixPid::from_raw(pid.0 as i32);
                // Send signal 0 to check if process exists

                if !signal::kill(nix_pid, None).is_ok() {
                    info!(pid=%pid.0, "Unix process is no longer running");
                    false
                } else {
                    info!(pid=%pid.0, "Unix process is still running");
                    true
                }
            } else {
                warn!("Unix process handle has no PID - process may have exited");
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

    /// Unix-specific process manager with comprehensive process tree management
    pub struct UnixProcessManager {
        system: std::sync::Mutex<System>,
    }

    impl Default for UnixProcessManager {
        fn default() -> Self {
            Self::new()
        }
    }

    #[async_trait]
    impl ProcessLifecycle for UnixProcessManager {
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

            // Create new process group for better process tree management
            cmd.process_group(0);

            let child = cmd.spawn()?;

            // Log successful process creation
            if let Some(pid) = child.id() {
                info!(
                    command = %command,
                    pid = %pid,
                    args = ?args,
                    "Spawned Unix process"
                );
            }

            Ok(Box::new(UnixProcessHandle::new(
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
    impl ProcessTermination for UnixProcessManager {
        async fn terminate_gracefully(&self, handle: &mut dyn ProcessHandle) -> TerminationResult {
            if let Some(pid) = handle.get_pid() {
                let nix_pid = NixPid::from_raw(pid.0 as i32);

                match signal::kill(nix_pid, Signal::SIGTERM) {
                    Ok(()) => {
                        info!(pid=%pid.0, "Sent SIGTERM to process");
                        TerminationResult::Success
                    }
                    Err(nix::errno::Errno::ESRCH) => {
                        info!(pid=%pid.0, "Process not found (already terminated)");
                        TerminationResult::ProcessNotFound
                    }
                    Err(nix::errno::Errno::EPERM) => {
                        warn!(pid=%pid.0, "Permission denied to terminate process");
                        TerminationResult::AccessDenied
                    }
                    Err(e) => {
                        warn!(pid=%pid.0, error=%e, "Failed to send SIGTERM to process");
                        TerminationResult::Failed(format!("SIGTERM failed: {e}"))
                    }
                }
            } else {
                TerminationResult::ProcessNotFound
            }
        }

        async fn force_kill(&self, handle: &mut dyn ProcessHandle) -> TerminationResult {
            if let Some(pid) = handle.get_pid() {
                let nix_pid = NixPid::from_raw(pid.0 as i32);

                match signal::kill(nix_pid, Signal::SIGKILL) {
                    Ok(()) => {
                        info!(pid=%pid.0, "Sent SIGKILL to process");
                        // Also call handle's kill method for cleanup
                        if let Err(e) = handle.kill().await {
                            warn!(error=%e, "Handle kill cleanup failed");
                        }
                        TerminationResult::Success
                    }
                    Err(nix::errno::Errno::ESRCH) => {
                        info!(pid=%pid.0, "Process not found (already terminated)");
                        TerminationResult::ProcessNotFound
                    }
                    Err(nix::errno::Errno::EPERM) => {
                        warn!(pid=%pid.0, "Permission denied to kill process");
                        TerminationResult::AccessDenied
                    }
                    Err(e) => {
                        warn!(pid=%pid.0, error=%e, "Failed to send SIGKILL to process");
                        TerminationResult::Failed(format!("SIGKILL failed: {e}"))
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
                sysinfo::ProcessRefreshKind::default(),
            );

            let mut children = Vec::new();
            Self::find_children_recursive(&system, parent_pid.0, &mut children);

            Ok(children.into_iter().map(ProcessId::from).collect())
        }

        async fn terminate_process_tree(&self, root_pid: ProcessId) -> TerminationResult {
            info!(root_pid=%root_pid.0, "Terminating process tree for root PID");

            // Find all child processes
            let children = match self.find_child_processes(root_pid).await {
                Ok(children) => children,
                Err(e) => {
                    warn!(
                        root_pid=%root_pid.0, error=%e,
                        "Failed to find child processes for PID"
                    );
                    return TerminationResult::Failed(format!("Failed to enumerate children: {e}"));
                }
            };

            if children.is_empty() {
                info!(root_pid=%root_pid.0, "No child processes found for PID");
            } else {
                info!(root_pid=%root_pid.0, count=%children.len(), "Found child processes to terminate");

                // Terminate children first (bottom-up approach)
                for child_pid in children.iter().rev() {
                    match self.terminate_single_process(*child_pid).await {
                        TerminationResult::Success | TerminationResult::ProcessNotFound => {}
                        result => {
                            warn!(child_pid = %child_pid.0, result = ?result, "Failed to terminate child process");
                        }
                    }
                }
            }

            // Finally terminate the root process
            self.terminate_single_process(root_pid).await
        }

        async fn terminate_process_group(&self, pid: ProcessId) -> TerminationResult {
            let pgid = NixPid::from_raw(pid.0 as i32);

            // Try SIGTERM first for graceful shutdown
            match signal::killpg(pgid, Signal::SIGTERM) {
                Ok(()) => {
                    info!(pid=%pid.0, "Sent SIGTERM to process group");

                    // Wait for graceful shutdown
                    tokio::time::sleep(Duration::from_millis(2000)).await;

                    // Check if processes are still running, if so use SIGKILL
                    match signal::killpg(pgid, Signal::SIGKILL) {
                        Ok(()) => {
                            info!(pid=%pid.0, "Sent SIGKILL to process group");
                            TerminationResult::Success
                        }
                        Err(nix::errno::Errno::ESRCH) => {
                            info!(pid=%pid.0, "Process group already terminated");
                            TerminationResult::Success
                        }
                        Err(e) => {
                            warn!(pid=%pid.0, error=%e, "Failed to send SIGKILL to process group");
                            TerminationResult::Failed(format!(
                                "SIGKILL to process group failed: {e}"
                            ))
                        }
                    }
                }
                Err(nix::errno::Errno::ESRCH) => {
                    info!(pid=%pid.0, "Process group not found (already terminated)");
                    TerminationResult::Success
                }
                Err(nix::errno::Errno::EPERM) => {
                    warn!(pid=%pid.0, "Permission denied to terminate process group");
                    TerminationResult::AccessDenied
                }
                Err(e) => {
                    warn!(pid=%pid.0, error=%e, "Failed to send SIGTERM to process group");
                    TerminationResult::Failed(format!("SIGTERM to process group failed: {e}"))
                }
            }
        }
    }

    impl UnixProcessManager {
        /// Terminate a single process by PID with escalation
        async fn terminate_single_process(&self, pid: ProcessId) -> TerminationResult {
            let nix_pid = NixPid::from_raw(pid.0 as i32);

            // Try SIGTERM first
            match signal::kill(nix_pid, Signal::SIGTERM) {
                Ok(()) => {
                    info!(pid=%pid.0, "Sent SIGTERM to process");

                    // Wait briefly for graceful shutdown
                    tokio::time::sleep(Duration::from_millis(500)).await;

                    // Then SIGKILL if still running
                    match signal::kill(nix_pid, Signal::SIGKILL) {
                        Ok(()) => {
                            info!(pid=%pid.0, "Sent SIGKILL to process");
                            TerminationResult::Success
                        }
                        Err(nix::errno::Errno::ESRCH) => {
                            info!(pid=%pid.0, "Process already terminated");
                            TerminationResult::Success
                        }
                        Err(e) => {
                            warn!(pid=%pid.0, error=%e, "Failed to kill process");
                            TerminationResult::Failed(format!("SIGKILL failed: {e}"))
                        }
                    }
                }
                Err(nix::errno::Errno::ESRCH) => {
                    info!(pid=%pid.0, "Process not found (already terminated)");
                    TerminationResult::Success
                }
                Err(nix::errno::Errno::EPERM) => {
                    warn!(pid=%pid.0, "Permission denied to terminate process");
                    TerminationResult::AccessDenied
                }
                Err(e) => {
                    warn!(pid=%pid.0, error=%e, "Failed to send SIGTERM to process");
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
            info!("Initializing Unix process manager with system monitoring");
            Self {
                system: std::sync::Mutex::new(System::new_all()),
            }
        }
    }
}

// Re-export the Unix implementation when on Unix systems
#[cfg(unix)]
pub use unix_impl::{UnixProcessHandle, UnixProcessManager};

// Provide stub implementations for non-Unix systems
#[cfg(not(unix))]
pub struct UnixProcessHandle;

#[cfg(not(unix))]
pub struct UnixProcessManager;

#[cfg(not(unix))]
impl UnixProcessManager {
    pub fn new() -> Self {
        Self
    }
}

#[cfg(not(unix))]
impl Default for UnixProcessManager {
    fn default() -> Self {
        Self::new()
    }
}
