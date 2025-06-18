//! Unix-specific process management implementation

use gpmcp_core::process::*;

mod unix_process_manager;
mod unix_process_handle;

pub use unix_process_manager::UnixProcessManager;
pub use unix_process_handle::UnixProcessHandle;

/// Unix-specific process manager factory
pub struct UnixProcessManagerFactory;

impl ProcessManagerFactory for UnixProcessManagerFactory {
    fn create_process_manager() -> Box<dyn ProcessManager> {
        Box::new(UnixProcessManager::new())
    }
    
    fn platform_name() -> &'static str {
        "Unix"
    }
}