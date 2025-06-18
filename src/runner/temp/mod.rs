pub mod platforms;
mod runner_factory;
mod simple_trait_manager;
mod trait_based_runner;
mod trait_process_manager;
pub mod traits;
pub use runner_factory::{
    GpmcpRunnerFactory, GpmcpRunnerWrapper, McpServiceWrapper, RunnerImplementation,
};
pub use simple_trait_manager::SimpleTraitProcessManager;
pub use trait_based_runner::{TraitBasedGpmcpRunner, TraitBasedMcpService};
pub use trait_process_manager::TraitBasedProcessManager;
