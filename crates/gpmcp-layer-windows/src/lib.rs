//! Windows-specific process management implementation

mod windows_process_manager;
mod runner_process_manager;

use gpmcp_layer_core::process::{ProcessManager, ProcessManagerFactory};
pub use windows_process_manager::WindowsProcessManager;
pub use runner_process_manager::{WindowsRunnerProcessManager, WindowsRunnerProcessManagerFactory};

/// Windows-specific process manager factory
pub struct WindowsProcessManagerFactory;

impl ProcessManagerFactory for WindowsProcessManagerFactory {
    fn create_process_manager() -> Box<dyn ProcessManager> {
        Box::new(WindowsProcessManager::new())
    }

    fn platform_name() -> &'static str {
        "Windows"
    }
}
