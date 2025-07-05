mod runner_process_manager;
mod unix_process_manager;

use gpmcp_layer_core::process::{ProcessManager, ProcessManagerFactory};
pub use runner_process_manager::{UnixRunnerProcessManager, UnixRunnerProcessManagerFactory};
pub use unix_process_manager::{UnixProcessHandle, UnixProcessManager};

pub struct UnixProcessManagerFactory;

impl ProcessManagerFactory for UnixProcessManagerFactory {
    fn create_process_manager() -> Box<dyn ProcessManager> {
        Box::new(UnixProcessManager::new())
    }

    fn platform_name() -> &'static str {
        "Unix"
    }
}
