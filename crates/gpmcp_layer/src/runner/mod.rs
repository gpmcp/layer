mod inner;
mod process_manager;
mod process_runner;
mod service_coordinator;
mod transport_manager;

// Process management traits and platform-specific implementations
mod platform_factory;
mod process_traits;

#[cfg(unix)]
mod unix_process_manager;

#[cfg(windows)]
mod windows_process_manager;

// Integration tests
#[cfg(test)]
mod integration_tests;

#[cfg(test)]
mod sse_test;

// Public exports
pub use process_manager::ProcessManager;
pub use process_runner::*;
pub use process_traits::{ProcessHandle, ProcessId, ProcessInfo, ProcessStatus, TerminationResult};
