//! Windows-specific process management implementation

use gpmcp_core::process::*;

mod windows_process_manager;

pub use windows_process_manager::WindowsProcessManager;

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
