//! Trait abstractions for platform-independent runner components
//! 
//! This module provides trait-based abstractions for:
//! - Process lifecycle management (creation, monitoring, termination)
//! - Process termination strategies (graceful, tree cleanup, force kill)
//! - Transport management (stdio, SSE, custom transports)
//! - Service coordination (MCP service management)

pub mod process_lifecycle;
pub mod termination;
pub mod transport;
pub mod service;

// Re-export commonly used traits and types
pub use process_lifecycle::{
    ProcessCreator, ProcessMonitor, ProcessManager, ProcessHandle, ProcessId, ProcessInfo, ProcessStatus, TokioProcessHandle
};
pub use termination::{
    ProcessGroupTerminator, ProcessTreeTerminator, ProcessKiller, TerminationStrategy, TerminationResult, DefaultTerminationStrategy
};
pub use transport::{
    TransportCreator, TransportManager, TransportHandle, TransportType, TransportVariant, 
    DefaultTransportCreator, DefaultTransportManager, StdioTransportHandle, SseTransportHandle
};
pub use service::{ServiceManager, ServiceCoordinator};