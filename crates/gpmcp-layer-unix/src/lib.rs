mod unix_process_manager;

pub use unix_process_manager::{UnixProcessHandle, UnixProcessManager};

pub struct UnixProcessManagerFactory;

impl UnixProcessManagerFactory {
    pub fn create_process_manager() -> UnixProcessManager {
        UnixProcessManager::new()
    }

    pub fn platform_name() -> &'static str {
        "Unix"
    }
}
