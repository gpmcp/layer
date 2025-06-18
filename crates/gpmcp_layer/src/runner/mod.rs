mod inner;
mod process_manager;
mod service_coordinator;
mod transport_manager;

// Process management traits and platform-specific implementations
mod platform_factory;

pub use inner::GpmcpRunnerInner;
