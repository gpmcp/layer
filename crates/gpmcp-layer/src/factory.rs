use async_trait::async_trait;
use gpmcp_layer_core::config::RunnerConfig;
use gpmcp_layer_core::process_manager_trait::RunnerProcessManagerFactory;

/// Platform-independent factory that selects the appropriate implementation at compile time
pub struct PlatformRunnerProcessManagerFactory;

#[async_trait]
impl RunnerProcessManagerFactory for PlatformRunnerProcessManagerFactory {
    #[cfg(unix)]
    type Manager = gpmcp_layer_unix::UnixRunnerProcessManager;

    #[cfg(windows)]
    type Manager = gpmcp_layer_windows::WindowsRunnerProcessManager;

    fn create_process_manager(config: &RunnerConfig) -> Self::Manager {
        #[cfg(unix)]
        return gpmcp_layer_unix::UnixRunnerProcessManagerFactory::create_process_manager(config);

        #[cfg(windows)]
        return gpmcp_layer_windows::WindowsRunnerProcessManagerFactory::create_process_manager(
            config,
        );
    }
}
