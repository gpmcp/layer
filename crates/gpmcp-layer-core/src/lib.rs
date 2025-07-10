pub mod config;
pub mod error;
pub mod layer;
pub mod process;
pub mod process_manager_trait;
mod runner;
mod stdio;

pub use rmcp;
pub use runner::{service_coordinator, transport_manager};
pub use stdio::*;
