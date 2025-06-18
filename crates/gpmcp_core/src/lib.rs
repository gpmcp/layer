//! GPMCP Core - Platform-independent abstractions and configurations
//! 
//! This crate provides the core traits, configurations, and error types
//! that are shared across platform-specific implementations.

pub mod config;
pub mod error;
pub mod process;

// Re-export commonly used types
pub use config::*;
pub use error::*;
pub use process::*;
