mod inner;
// Removed: mod process_manager; - replaced by trait-based implementations
mod process_manager_trait;
mod service_coordinator;
mod transport_manager;

// Process management traits and platform-specific implementations
// Removed: mod platform_factory; - moved to main gpmcp-layer crate
// Removed: mod platform_runner_factory; - moved to main gpmcp-layer crate

pub use inner::{GpmcpRunnerInner, Initialized, Uninitialized};
pub use process_manager_trait::{RunnerProcessManager, RunnerProcessManagerFactory};
// Platform factory moved to main gpmcp-layer crate
// pub use platform_runner_factory::{PlatformRunnerProcessManagerFactory, create_runner_process_manager};
