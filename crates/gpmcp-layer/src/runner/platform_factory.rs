use anyhow::Result;
use async_trait::async_trait;
use gpmcp_layer_core::{
    ProcessHandle, ProcessId, ProcessInfo, ProcessLifecycle, ProcessStatus, ProcessTermination,
    TerminationResult,
};
use std::collections::HashMap;
use std::time::Duration;
use tracing::info;

/// Platform-specific process manager implementations
#[derive(Clone)]
pub enum PlatformProcessManager {
    #[cfg(unix)]
    Unix(std::sync::Arc<gpmcp_layer_unix::UnixProcessManager>),
    #[cfg(windows)]
    Windows(std::sync::Arc<gpmcp_layer_windows::WindowsProcessManager>),
}

impl PlatformProcessManager {
    pub fn new() -> Self {
        #[cfg(unix)]
        {
            info!("Creating Unix process manager");
            Self::Unix(std::sync::Arc::new(
                gpmcp_layer_unix::UnixProcessManagerFactory::create_process_manager(),
            ))
        }

        #[cfg(windows)]
        {
            info!("Creating Windows process manager");
            Self::Windows(std::sync::Arc::new(
                gpmcp_layer_windows::WindowsProcessManagerFactory::create_process_manager(),
            ))
        }

        #[cfg(not(any(unix, windows)))]
        {
            compile_error!("Unsupported platform: only Unix and Windows are currently supported");
        }
    }

    pub fn platform_name() -> &'static str {
        #[cfg(unix)]
        {
            gpmcp_layer_unix::UnixProcessManagerFactory::platform_name()
        }

        #[cfg(windows)]
        {
            gpmcp_layer_windows::WindowsProcessManagerFactory::platform_name()
        }

        #[cfg(not(any(unix, windows)))]
        {
            "Unknown"
        }
    }
}

#[async_trait]
impl ProcessLifecycle for PlatformProcessManager {
    async fn spawn_process(
        &self,
        command: &str,
        args: &[String],
        working_dir: Option<&str>,
        env: &HashMap<String, String>,
    ) -> Result<Box<dyn ProcessHandle>, std::io::Error> {
        match self {
            #[cfg(unix)]
            Self::Unix(manager) => manager.spawn_process(command, args, working_dir, env).await,
            #[cfg(windows)]
            Self::Windows(manager) => manager.spawn_process(command, args, working_dir, env).await,
        }
    }

    async fn is_process_healthy(&self, handle: &dyn ProcessHandle) -> bool {
        match self {
            #[cfg(unix)]
            Self::Unix(manager) => manager.is_process_healthy(handle).await,
            #[cfg(windows)]
            Self::Windows(manager) => manager.is_process_healthy(handle).await,
        }
    }

    async fn get_process_info(&self, handle: &dyn ProcessHandle) -> Result<ProcessInfo> {
        match self {
            #[cfg(unix)]
            Self::Unix(manager) => manager.get_process_info(handle).await,
            #[cfg(windows)]
            Self::Windows(manager) => manager.get_process_info(handle).await,
        }
    }

    async fn wait_for_exit(
        &self,
        handle: &mut dyn ProcessHandle,
        timeout: Option<Duration>,
    ) -> Result<ProcessStatus> {
        match self {
            #[cfg(unix)]
            Self::Unix(manager) => manager.wait_for_exit(handle, timeout).await,
            #[cfg(windows)]
            Self::Windows(manager) => manager.wait_for_exit(handle, timeout).await,
        }
    }
}

#[async_trait]
impl ProcessTermination for PlatformProcessManager {
    async fn terminate_gracefully(&self, handle: &mut dyn ProcessHandle) -> TerminationResult {
        match self {
            #[cfg(unix)]
            Self::Unix(manager) => manager.terminate_gracefully(handle).await,
            #[cfg(windows)]
            Self::Windows(manager) => manager.terminate_gracefully(handle).await,
        }
    }

    async fn force_kill(&self, handle: &mut dyn ProcessHandle) -> TerminationResult {
        match self {
            #[cfg(unix)]
            Self::Unix(manager) => manager.force_kill(handle).await,
            #[cfg(windows)]
            Self::Windows(manager) => manager.force_kill(handle).await,
        }
    }

    async fn find_child_processes(&self, parent_pid: ProcessId) -> Result<Vec<ProcessId>> {
        match self {
            #[cfg(unix)]
            Self::Unix(manager) => manager.find_child_processes(parent_pid).await,
            #[cfg(windows)]
            Self::Windows(manager) => manager.find_child_processes(parent_pid).await,
        }
    }

    async fn terminate_process_tree(&self, root_pid: ProcessId) -> TerminationResult {
        match self {
            #[cfg(unix)]
            Self::Unix(manager) => manager.terminate_process_tree(root_pid).await,
            #[cfg(windows)]
            Self::Windows(manager) => manager.terminate_process_tree(root_pid).await,
        }
    }

    async fn terminate_process_group(&self, pid: ProcessId) -> TerminationResult {
        match self {
            #[cfg(unix)]
            Self::Unix(manager) => manager.terminate_process_group(pid).await,
            #[cfg(windows)]
            Self::Windows(manager) => manager.terminate_process_group(pid).await,
        }
    }
}

/// Platform-agnostic factory that selects the appropriate implementation at compile time
pub struct PlatformProcessManagerFactory;

impl PlatformProcessManagerFactory {
    pub fn create_process_manager() -> std::sync::Arc<PlatformProcessManager> {
        std::sync::Arc::new(PlatformProcessManager::new())
    }

    pub fn platform_name() -> &'static str {
        PlatformProcessManager::platform_name()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_platform_detection() {
        let platform = PlatformProcessManagerFactory::platform_name();
        println!("Running on platform: {platform}");

        // Ensure we can create platform-specific managers
        let _process_manager = PlatformProcessManagerFactory::create_process_manager();
    }
}
