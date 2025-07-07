//! Windows-specific process management implementation

mod runner_process_manager;
mod windows_process_manager;

use gpmcp_layer_core::process::{ProcessManager, ProcessManagerFactory};
pub use runner_process_manager::{WindowsRunnerProcessManager, WindowsRunnerProcessManagerFactory};
pub use windows_process_manager::WindowsProcessManager;

/// Windows-specific process manager factory
pub struct WindowsProcessManagerFactory;

impl ProcessManagerFactory for WindowsProcessManagerFactory {
    type Manager = WindowsProcessManager;

    fn create_process_manager() -> Self::Manager {
        WindowsProcessManager::new()
    }

}
