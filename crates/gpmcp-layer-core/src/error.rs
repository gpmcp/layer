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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_error_creation() {
        let error = GpmcpError::ProcessError("test command".to_string());
        assert!(!error.is_retryable()); // ProcessError is not retryable

        let error = GpmcpError::ConfigurationError("invalid config".to_string());
        assert!(!error.is_retryable());
    }

    #[test]
    fn test_error_display() {
        let error = GpmcpError::ProcessError("test".to_string());
        let display = format!("{error}");
        assert!(display.contains("Process management error"));

        let error = GpmcpError::TransportError("connection failed".to_string());
        let display = format!("{error}");
        assert!(display.contains("Transport error"));
    }

    #[test]
    fn test_error_categorization() {
        // Retryable errors
        assert!(GpmcpError::ConnectionFailed("test".to_string()).is_retryable());
        assert!(GpmcpError::TransportError("test".to_string()).is_retryable());
        assert!(GpmcpError::Timeout("test".to_string()).is_retryable());

        // Non-retryable errors
        assert!(!GpmcpError::ConfigurationError("test".to_string()).is_retryable());
        assert!(!GpmcpError::ProcessError("test".to_string()).is_retryable());
        assert!(!GpmcpError::ServiceNotFound.is_retryable());
    }

    #[test]
    fn test_error_debug_format() {
        let error = GpmcpError::ProcessError("test command".to_string());
        let debug_str = format!("{error:?}");
        assert!(debug_str.contains("ProcessError"));
        assert!(debug_str.contains("test command"));
    }
}
