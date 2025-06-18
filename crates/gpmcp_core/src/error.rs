use thiserror::Error;

/// Core error types for GPMCP operations
#[derive(Error, Debug)]
pub enum GpmcpError {
    #[error("Service not found or not initialized")]
    ServiceNotFound,

    #[error("Connection failed: {0}")]
    ConnectionFailed(String),

    #[error("Process management error: {0}")]
    ProcessError(String),

    #[error("Transport error: {0}")]
    TransportError(String),

    #[error("Configuration error: {0}")]
    ConfigurationError(String),

    #[error("Timeout occurred: {0}")]
    Timeout(String),

    #[error("Operation cancelled")]
    Cancelled,

    #[error("Retry limit exceeded")]
    RetryLimitExceeded,

    #[error("Other error: {0}")]
    Other(#[from] anyhow::Error),
}

impl GpmcpError {
    /// Check if this error is retryable
    pub fn is_retryable(&self) -> bool {
        matches!(
            self,
            GpmcpError::ConnectionFailed(_)
                | GpmcpError::TransportError(_)
                | GpmcpError::Timeout(_)
        )
    }

    /// Check if this error indicates a permanent failure
    pub fn is_permanent(&self) -> bool {
        matches!(
            self,
            GpmcpError::ConfigurationError(_) | GpmcpError::Cancelled
        )
    }
}