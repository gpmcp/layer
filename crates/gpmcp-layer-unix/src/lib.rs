mod unix_process_manager;

pub use unix_process_manager::{UnixProcessHandle, UnixProcessManager};

pub struct UnixProcessManagerFactory;

impl gpmcp_layer_core::ProcessManagerFactory for UnixProcessManagerFactory {
    fn create_process_manager() -> Box<dyn gpmcp_layer_core::ProcessManager> {
        use gpmcp_layer_core::ProcessManager;
        Box::new(UnixProcessManager::new())
    }

    fn platform_name() -> &'static str {
        "Unix"
    }
}
