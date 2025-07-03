use derive_more::From;
use rmcp::service::ClientInitializeError;
use rmcp::transport::sse_client::SseTransportError;
use thiserror::Error;

/// Core error types for GPMCP operations
#[derive(Debug, Error)]
#[error("{inner}")]
pub struct GpmcpError {
    pub inner: Box<GpmcpErrorInner>,
}

impl GpmcpError {
    pub fn service_not_found() -> Self {
        GpmcpError {
            inner: Box::new(GpmcpErrorInner::ServiceNotFound),
        }
    }
}

impl<T: Into<GpmcpErrorInner>> From<T> for GpmcpError {
    fn from(value: T) -> Self {
        GpmcpError {
            inner: Box::new(value.into()),
        }
    }
}

#[derive(Error, Debug, From)]
pub enum GpmcpErrorInner {
    #[error("Service not found or not initialized")]
    ServiceNotFound,

    #[error("MCP operation failed: {0}")]
    McpOperationFailed(rmcp::ServiceError),

    #[error("Unable to wait for operation completion: {0}")]
    JoinError(tokio::task::JoinError),

    #[error("IO error occurred: {0}")]
    IoError(std::io::Error),

    #[error("Stdio client initialization error: {0}")]
    StdioInitError(ClientInitializeError<std::io::Error>),

    #[error("SSE transport error: {0}")]
    SseError(SseTransportError<reqwest::Error>),

    #[error("SSE client initialization error: {0}")]
    SseInitError(ClientInitializeError<SseTransportError<reqwest::Error>>),
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
