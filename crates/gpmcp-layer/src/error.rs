use derive_more::From;
use thiserror::Error;

/// Core error types for GPMCP operations
#[derive(Error, Debug, From)]
pub enum GpmcpError {
    #[error("Service not found or not initialized")]
    ServiceNotFound,

    #[error("MCP operation failed: {0}")]
    McpOperationFailed(rmcp::ServiceError),
}

impl GpmcpError {
    /// Check if this error is retryable
    pub fn is_retryable(&self) -> bool {
        todo!()
    }

    /// Check if this error indicates a permanent failure
    pub fn is_permanent(&self) -> bool {
        todo!()
    }
}
