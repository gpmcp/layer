pub mod unix;
pub mod windows;
pub mod factory;

// Re-export platform-specific implementations
pub use factory::{create_platform_process_manager, create_platform_termination_strategy};

#[cfg(unix)]
pub use unix::*;

#[cfg(windows)]
pub use windows::*;