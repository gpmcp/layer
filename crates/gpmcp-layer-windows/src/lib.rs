//! Windows-specific process management implementation

mod windows_process_manager;
mod runner_process_manager;

pub use windows_process_manager::WindowsProcessManager;
pub use runner_process_manager::{WindowsRunnerProcessManager, WindowsRunnerProcessManagerFactory};

/// Windows-specific process manager factory
pub struct WindowsProcessManagerFactory;

impl gpmcp_layer_core::ProcessManagerFactory for WindowsProcessManagerFactory {
    fn create_process_manager() -> Box<dyn gpmcp_layer_core::ProcessManager> {
        use gpmcp_layer_core::ProcessManager;
        Box::new(WindowsProcessManager::new())
    }

    fn platform_name() -> &'static str {
        "Windows"
    }
}
