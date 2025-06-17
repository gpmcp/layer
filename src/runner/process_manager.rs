use anyhow::{Context, Result};
use gpmcp_domain::blueprint::ServerDefinition;
use std::sync::Arc;
use std::time::Duration;
use sysinfo::System;
use tokio::process::{Child, Command};
use tokio_util::sync::CancellationToken;
use tracing::{error, info, warn};

#[cfg(unix)]
#[allow(unused_imports)]
use std::os::unix::process::CommandExt;

/// ProcessManager handles the lifecycle of subprocess execution.
/// It provides functionality to start, monitor, restart, and cleanup processes.
pub struct ProcessManager {
    server_definition: ServerDefinition,
    child_handle: Arc<tokio::sync::Mutex<Option<Child>>>,
    monitor_handle: Option<tokio::task::JoinHandle<()>>,
    cancellation_token: Arc<CancellationToken>,
    restart_count: Arc<tokio::sync::Mutex<u32>>,
}

impl ProcessManager {
    /// Creates a new ProcessManager and starts the subprocess
    pub async fn new(
        server_definition: ServerDefinition,
        cancellation_token: Arc<CancellationToken>,
    ) -> Result<Self> {
        let child_handle = Arc::new(tokio::sync::Mutex::new(None));
        let restart_count = Arc::new(tokio::sync::Mutex::new(0));

        let mut manager = Self {
            server_definition,
            child_handle,
            monitor_handle: None,
            cancellation_token,
            restart_count,
        };

        manager.start_process().await?;
        Ok(manager)
    }

    /// Starts the subprocess based on the server definition
    async fn start_process(&mut self) -> Result<()> {
        let command_runner = self.get_command_runner()?;

        let mut cmd = Command::new(&command_runner.command);
        cmd.args(&command_runner.args);

        // Set working directory if specified
        if !command_runner.workdir.is_empty() {
            cmd.current_dir(&command_runner.workdir);
        }

        // Add environment variables
        for (key, value) in &self.server_definition.env {
            cmd.env(key, value);
        }

        // Create a new process group for better process tree management
        #[cfg(unix)]
        {
            cmd.process_group(0);
        }

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
                    "Started process: {} with args: {:?}, but PID is not available (process may have exited quickly)",
                    command_runner.command, command_runner.args
                );
            }
        }

        // Store the child process
        {
            let mut child_guard = self.child_handle.lock().await;
            *child_guard = Some(child);
        }

        // Start monitoring the process
        self.start_monitor().await;

        Ok(())
    }

    /// Starts the process monitor task
    async fn start_monitor(&mut self) {
        let child_handle = self.child_handle.clone();
        let cancellation_token = self.cancellation_token.clone();

        let monitor_handle = tokio::spawn(async move {
            tokio::select! {
                _ = cancellation_token.cancelled() => {
                    info!("Cancellation requested, terminating child process");
                    Self::terminate_child(&child_handle).await;
                }
                result = Self::wait_for_child(&child_handle) => {
                    // Get PID before the process exits (if still available)
                    let pid_info = {
                        let child_guard = child_handle.lock().await;
                        child_guard.as_ref()
                            .and_then(|child| child.id())
                            .map(|pid| format!(" (PID: {})", pid))
                            .unwrap_or_default()
                    };
                    
                    match result {
                        Ok(Some(status)) => {
                            if status.success() {
                                info!("Child process{} exited successfully with status: {}", pid_info, status);
                            } else {
                                warn!("Child process{} exited with non-zero status: {}", pid_info, status);
                            }
                        }
                        Ok(None) => {
                            info!("Child process{} was already terminated", pid_info);
                        }
                        Err(e) => {
                            error!("Error waiting for child process{}: {}", pid_info, e);
                        }
                    }
                    // Remove the child from the handle since it's no longer running
                    let mut child_guard = child_handle.lock().await;
                    *child_guard = None;
                }
            }
        });

        self.monitor_handle = Some(monitor_handle);
    }

    /// Waits for the child process to exit
    async fn wait_for_child(
        child_handle: &Arc<tokio::sync::Mutex<Option<Child>>>,
    ) -> Result<Option<std::process::ExitStatus>> {
        let mut child_guard = child_handle.lock().await;
        if let Some(ref mut child) = child_guard.as_mut() {
            let status = child.wait().await?;
            Ok(Some(status))
        } else {
            Ok(None)
        }
    }

    /// Terminates the child process and its entire process tree using a three-step approach:
    /// 1. Try to kill the process group (if supported)
    /// 2. Recursively kill the process tree
    /// 3. Use graceful (SIGTERM) then forceful (SIGKILL) termination
    async fn terminate_child(child_handle: &Arc<tokio::sync::Mutex<Option<Child>>>) {
        let mut child_guard = child_handle.lock().await;
        if let Some(mut child) = child_guard.take() {
            let pid = child.id();
            let pid_info = pid.map(|p| format!(" (PID: {})", p)).unwrap_or_default();
            
            info!("Starting three-step termination for child process{}", pid_info);

            // Step 1: Try to kill the process group (Unix only)
            let mut process_group_killed = false;
            #[cfg(unix)]
            if let Some(pid) = pid {
                process_group_killed = Self::kill_process_group(pid).await;
            }

            // Step 2: If process group kill failed, try recursive process tree termination
            if !process_group_killed {
                if let Some(pid) = pid {
                    Self::kill_process_tree(pid).await;
                }
            }

            // Step 3: Finally, kill the main process with escalating signals
            Self::kill_process_with_escalation(&mut child, &pid_info).await;
        }
    }

    /// Step 1: Kill the entire process group (Unix only)
    #[cfg(unix)]
    async fn kill_process_group(pid: u32) -> bool {
        use nix::sys::signal::{self, Signal};
        use nix::unistd::Pid as NixPid;

        let pgid = NixPid::from_raw(pid as i32);
        
        // Try SIGTERM first for graceful shutdown
        match signal::killpg(pgid, Signal::SIGTERM) {
            Ok(()) => {
                info!("Sent SIGTERM to process group {}", pid);
                
                // Wait a bit for graceful shutdown
                tokio::time::sleep(Duration::from_millis(2000)).await;
                
                // Check if processes are still running, if so use SIGKILL
                match signal::killpg(pgid, Signal::SIGKILL) {
                    Ok(()) => {
                        info!("Sent SIGKILL to process group {}", pid);
                        true
                    }
                    Err(nix::errno::Errno::ESRCH) => {
                        info!("Process group {} already terminated", pid);
                        true
                    }
                    Err(e) => {
                        warn!("Failed to send SIGKILL to process group {}: {}", pid, e);
                        false
                    }
                }
            }
            Err(nix::errno::Errno::ESRCH) => {
                info!("Process group {} not found (already terminated)", pid);
                true
            }
            Err(e) => {
                warn!("Failed to send SIGTERM to process group {}: {}", pid, e);
                false
            }
        }
    }

    /// Step 1: Fallback for non-Unix systems
    #[cfg(not(unix))]
    async fn kill_process_group(_pid: u32) -> bool {
        warn!("Process group termination not supported on this platform");
        false
    }

    /// Step 2: Recursively kill the process tree
    async fn kill_process_tree(root_pid: u32) {
        info!("Enumerating and killing process tree for PID {}", root_pid);
        
        let mut system = System::new_all();
        system.refresh_processes_specifics(
            sysinfo::ProcessesToUpdate::All, 
            true, 
            sysinfo::ProcessRefreshKind::new()
        );
        
        // Find all descendant processes
        let mut processes_to_kill = Vec::new();
        Self::find_child_processes(&system, root_pid, &mut processes_to_kill);
        
        if processes_to_kill.is_empty() {
            info!("No child processes found for PID {}", root_pid);
            return;
        }

        info!("Found {} child processes to terminate", processes_to_kill.len());
        
        // Kill children first (bottom-up approach)
        for &pid in processes_to_kill.iter().rev() {
            Self::kill_single_process(pid).await;
        }
        
        // Finally kill the root process
        Self::kill_single_process(root_pid).await;
    }

    /// Recursively find all child processes
    fn find_child_processes(system: &System, parent_pid: u32, result: &mut Vec<u32>) {
        for (pid, process) in system.processes() {
            if let Some(ppid) = process.parent() {
                if ppid.as_u32() == parent_pid {
                    let child_pid = pid.as_u32();
                    // Recursively find grandchildren first
                    Self::find_child_processes(system, child_pid, result);
                    // Then add this child
                    result.push(child_pid);
                }
            }
        }
    }

    /// Kill a single process by PID
    async fn kill_single_process(pid: u32) {
        #[cfg(unix)]
        {
            use nix::sys::signal::{self, Signal};
            use nix::unistd::Pid as NixPid;

            let nix_pid = NixPid::from_raw(pid as i32);
            
            // Try SIGTERM first
            match signal::kill(nix_pid, Signal::SIGTERM) {
                Ok(()) => {
                    info!("Sent SIGTERM to process {}", pid);
                    
                    // Wait briefly for graceful shutdown
                    tokio::time::sleep(Duration::from_millis(500)).await;
                    
                    // Then SIGKILL if still running
                    match signal::kill(nix_pid, Signal::SIGKILL) {
                        Ok(()) => info!("Sent SIGKILL to process {}", pid),
                        Err(nix::errno::Errno::ESRCH) => info!("Process {} already terminated", pid),
                        Err(e) => warn!("Failed to kill process {}: {}", pid, e),
                    }
                }
                Err(nix::errno::Errno::ESRCH) => {
                    info!("Process {} not found (already terminated)", pid);
                }
                Err(e) => {
                    warn!("Failed to send SIGTERM to process {}: {}", pid, e);
                }
            }
        }

        #[cfg(windows)]
        {
            // Windows process termination
            use tokio::process::Command as TokioCommand;
            
            let output = TokioCommand::new("taskkill")
                .args(["/F", "/PID", &pid.to_string()])
                .output()
                .await;
                
            match output {
                Ok(output) => {
                    if output.status.success() {
                        info!("Successfully terminated process {} on Windows", pid);
                    } else {
                        warn!("Failed to terminate process {} on Windows", pid);
                    }
                }
                Err(e) => {
                    warn!("Error running taskkill for process {}: {}", pid, e);
                }
            }
        }
    }

    /// Step 3: Kill the main process with signal escalation
    async fn kill_process_with_escalation(child: &mut Child, pid_info: &str) {
        // First try the tokio kill method (SIGKILL on Unix)
        match child.kill().await {
            Ok(()) => {
                info!("Child process{} killed successfully", pid_info);
            }
            Err(e) => {
                warn!("Failed to kill child process{}: {}", pid_info, e);
            }
        }

        // Wait for the process to exit to clean up resources
        match child.wait().await {
            Ok(status) => {
                info!("Child process{} exited with status: {}", pid_info, status);
            }
            Err(e) => {
                warn!("Error waiting for child process{} to exit: {}", pid_info, e);
            }
        }
    }

    /// Gets the command runner from the server definition
    fn get_command_runner(&self) -> Result<&gpmcp_domain::blueprint::CommandRunner> {
        match &self.server_definition.runner {
            gpmcp_domain::blueprint::Runner::Stdio { command_runner } => Ok(command_runner),
            gpmcp_domain::blueprint::Runner::Sse { command_runner, .. } => Ok(command_runner),
        }
    }

    /// Restarts the process (useful for retry logic)
    pub async fn restart(&mut self) -> Result<()> {
        info!("Restarting process");

        // Increment restart count
        {
            let mut count = self.restart_count.lock().await;
            *count += 1;
            info!("Process restart count: {}", *count);
        }

        // Stop current process if running
        self.stop_process().await?;

        // Wait a bit before restarting
        tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;

        // Start new process
        self.start_process().await?;

        info!("Process restarted successfully");
        Ok(())
    }

    /// Stops the current process
    async fn stop_process(&mut self) -> Result<()> {
        // Cancel the monitor task
        if let Some(handle) = self.monitor_handle.take() {
            handle.abort();
        }

        // Terminate the child process
        Self::terminate_child(&self.child_handle).await;

        Ok(())
    }

    /// Checks if the process is healthy (still running)
    pub async fn is_healthy(&self) -> bool {
        let mut child_guard = self.child_handle.lock().await;
        match child_guard.as_mut() {
            Some(child) => {
                // Try to get the exit status without waiting
                match child.try_wait() {
                    Ok(None) => true, // Process is still running
                    Ok(Some(status)) => {
                        // Process has exited, remove it from the handle
                        let pid_info = child.id().map(|pid| format!(" (PID: {})", pid)).unwrap_or_default();
                        info!("Process{} has exited with status: {}", pid_info, status);
                        *child_guard = None;
                        false
                    }
                    Err(_) => false, // Error checking status
                }
            }
            None => false, // No process
        }
    }

    /// Gets the current restart count
    pub async fn restart_count(&self) -> u32 {
        *self.restart_count.lock().await
    }

    /// Cleanup the process manager
    pub async fn cleanup(mut self) -> Result<()> {
        info!("Cleaning up ProcessManager");

        // Stop the process
        self.stop_process().await?;

        info!("ProcessManager cleanup completed");
        Ok(())
    }
}

impl Drop for ProcessManager {
    fn drop(&mut self) {
        // Cancel the cancellation token to trigger cleanup
        self.cancellation_token.cancel();

        // Abort monitor task if it exists
        if let Some(handle) = self.monitor_handle.take() {
            handle.abort();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpmcp_domain::blueprint::{CommandRunner, Runner};
    use std::collections::HashMap;
    use tokio::time::{Duration, sleep};

    #[tokio::test]
    async fn test_process_manager_creation() {
        let command_runner = CommandRunner {
            command: "echo".to_string(),
            args: vec!["hello".to_string()],
            workdir: String::new(),
        };

        let server_definition = ServerDefinition {
            runner: Runner::Stdio { command_runner },
            env: HashMap::new(),
            ..Default::default()
        };

        let cancellation_token = Arc::new(CancellationToken::new());
        let result = ProcessManager::new(server_definition, cancellation_token).await;

        match result {
            Ok(manager) => {
                let _ = manager.cleanup().await;
            }
            Err(e) => {
                println!("Expected error for echo command: {}", e);
            }
        }
    }

    #[tokio::test]
    async fn test_process_health_check() {
        let command_runner = CommandRunner {
            command: "sleep".to_string(),
            args: vec!["2".to_string()],
            workdir: String::new(),
        };

        let server_definition = ServerDefinition {
            runner: Runner::Stdio { command_runner },
            env: HashMap::new(),
            ..Default::default()
        };

        let cancellation_token = Arc::new(CancellationToken::new());
        let result = ProcessManager::new(server_definition, cancellation_token).await;

        match result {
            Ok(manager) => {
                // Process should be healthy initially
                assert!(manager.is_healthy().await, "Process should be healthy");

                // Wait for process to exit
                sleep(Duration::from_millis(2500)).await;

                // Process should no longer be healthy
                assert!(
                    !manager.is_healthy().await,
                    "Process should not be healthy after exit"
                );

                let _ = manager.cleanup().await;
            }
            Err(e) => {
                println!("Failed to create process manager: {}", e);
            }
        }
    }

    #[tokio::test]
    async fn test_process_restart() {
        let command_runner = CommandRunner {
            command: "sleep".to_string(),
            args: vec!["1".to_string()],
            workdir: String::new(),
        };

        let server_definition = ServerDefinition {
            runner: Runner::Stdio { command_runner },
            env: HashMap::new(),
            ..Default::default()
        };

        let cancellation_token = Arc::new(CancellationToken::new());
        let result = ProcessManager::new(server_definition, cancellation_token).await;

        match result {
            Ok(mut manager) => {
                // Initial restart count should be 0
                assert_eq!(manager.restart_count().await, 0);

                // Restart the process
                let restart_result = manager.restart().await;
                assert!(restart_result.is_ok(), "Restart should succeed");

                // Restart count should be 1
                assert_eq!(manager.restart_count().await, 1);

                let _ = manager.cleanup().await;
            }
            Err(e) => {
                println!("Failed to create process manager: {}", e);
            }
        }
    }
}
