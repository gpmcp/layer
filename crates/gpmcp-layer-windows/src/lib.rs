//! Windows-specific process management implementation

mod windows_process_manager;

pub use windows_process_manager::WindowsProcessManager;

/// Windows-specific process manager factory
pub struct WindowsProcessManagerFactory;

impl WindowsProcessManagerFactory {
    pub fn create_process_manager() -> WindowsProcessManager {
        WindowsProcessManager::new()
    }

    pub fn platform_name() -> &'static str {
        "Windows"
    }
}
