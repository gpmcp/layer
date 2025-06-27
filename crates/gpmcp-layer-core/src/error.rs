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

    #[error("Service initialization failed: {0}")]
    ServiceInitializationFailed(String),

    #[error("Server not ready: {0}")]
    ServerNotReady(String),

    #[error("MCP operation failed: {0}")]
    McpOperationFailed(String),

    #[error("Invalid state: {0}")]
    InvalidState(String),

    #[error("Resource not found: {0}")]
    ResourceNotFound(String),

    #[error("Permission denied: {0}")]
    PermissionDenied(String),

    #[error("Network error: {0}")]
    NetworkError(String),

    #[error("Serialization error: {0}")]
    SerializationError(String),

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
                | GpmcpError::ServerNotReady(_)
                | GpmcpError::NetworkError(_)
                | GpmcpError::ServiceInitializationFailed(_)
        )
    }

    /// Check if this error indicates a permanent failure
    pub fn is_permanent(&self) -> bool {
        matches!(
            self,
            GpmcpError::ConfigurationError(_) 
                | GpmcpError::Cancelled
                | GpmcpError::PermissionDenied(_)
                | GpmcpError::ResourceNotFound(_)
                | GpmcpError::InvalidState(_)
        )
    }

    /// Create a connection failed error
    pub fn connection_failed(msg: impl Into<String>) -> Self {
        Self::ConnectionFailed(msg.into())
    }

    /// Create a transport error
    pub fn transport_error(msg: impl Into<String>) -> Self {
        Self::TransportError(msg.into())
    }

    /// Create a process error
    pub fn process_error(msg: impl Into<String>) -> Self {
        Self::ProcessError(msg.into())
    }

    /// Create a timeout error
    pub fn timeout(msg: impl Into<String>) -> Self {
        Self::Timeout(msg.into())
    }

    /// Create a server not ready error
    pub fn server_not_ready(msg: impl Into<String>) -> Self {
        Self::ServerNotReady(msg.into())
    }

    /// Create an MCP operation failed error
    pub fn mcp_operation_failed(msg: impl Into<String>) -> Self {
        Self::McpOperationFailed(msg.into())
    }

    /// Create a service initialization failed error
    pub fn service_initialization_failed(msg: impl Into<String>) -> Self {
        Self::ServiceInitializationFailed(msg.into())
    }

    /// Create a network error
    pub fn network_error(msg: impl Into<String>) -> Self {
        Self::NetworkError(msg.into())
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
        let display = format!("{}", error);
        assert!(display.contains("Process management error"));

        let error = GpmcpError::TransportError("connection failed".to_string());
        let display = format!("{}", error);
        assert!(display.contains("Transport error"));
    }

    #[test]
    fn test_error_categorization() {
        // Retryable errors
        assert!(GpmcpError::ConnectionFailed("test".to_string()).is_retryable());
        assert!(GpmcpError::TransportError("test".to_string()).is_retryable());
        assert!(GpmcpError::Timeout("test".to_string()).is_retryable());
        assert!(GpmcpError::ServerNotReady("test".to_string()).is_retryable());
        assert!(GpmcpError::NetworkError("test".to_string()).is_retryable());
        assert!(GpmcpError::ServiceInitializationFailed("test".to_string()).is_retryable());

        // Non-retryable errors
        assert!(!GpmcpError::ConfigurationError("test".to_string()).is_retryable());
        assert!(!GpmcpError::ProcessError("test".to_string()).is_retryable());
        assert!(!GpmcpError::ServiceNotFound.is_retryable());
        assert!(!GpmcpError::PermissionDenied("test".to_string()).is_retryable());
        assert!(!GpmcpError::ResourceNotFound("test".to_string()).is_retryable());

        // Permanent errors
        assert!(GpmcpError::ConfigurationError("test".to_string()).is_permanent());
        assert!(GpmcpError::Cancelled.is_permanent());
        assert!(GpmcpError::PermissionDenied("test".to_string()).is_permanent());
        assert!(GpmcpError::ResourceNotFound("test".to_string()).is_permanent());
        assert!(GpmcpError::InvalidState("test".to_string()).is_permanent());

        // Non-permanent errors
        assert!(!GpmcpError::ConnectionFailed("test".to_string()).is_permanent());
        assert!(!GpmcpError::TransportError("test".to_string()).is_permanent());
    }

    #[test]
    fn test_error_debug_format() {
        let error = GpmcpError::ProcessError("test command".to_string());
        let debug_str = format!("{:?}", error);
        assert!(debug_str.contains("ProcessError"));
        assert!(debug_str.contains("test command"));
    }

    #[test]
    fn test_convenience_methods() {
        let error = GpmcpError::connection_failed("test connection");
        assert!(matches!(error, GpmcpError::ConnectionFailed(_)));
        assert!(error.is_retryable());

        let error = GpmcpError::transport_error("test transport");
        assert!(matches!(error, GpmcpError::TransportError(_)));
        assert!(error.is_retryable());

        let error = GpmcpError::server_not_ready("test server");
        assert!(matches!(error, GpmcpError::ServerNotReady(_)));
        assert!(error.is_retryable());

        let error = GpmcpError::mcp_operation_failed("test operation");
        assert!(matches!(error, GpmcpError::McpOperationFailed(_)));
        assert!(!error.is_retryable());
    }
}
