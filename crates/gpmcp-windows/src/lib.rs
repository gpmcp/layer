//! Windows-specific process management implementation

mod windows_process_manager;

pub use windows_process_manager::WindowsProcessManager;

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
