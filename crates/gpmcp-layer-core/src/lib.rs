//! GPMCP Core - Platform-independent abstractions and configurations
//! 
//! This crate provides the core traits, configurations, and error types
//! that are shared across platform-specific implementations.

mod config;
mod error;
mod process;

pub use config::*;
pub use error::*;
pub use process::*;
