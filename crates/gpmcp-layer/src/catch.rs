use crate::GpmcpError;
use crate::error::GpmcpErrorInner;
use tracing::error;

pub trait Catch<T>: Sync + Send + Sized {
    fn catch(self) -> Result<T, GpmcpError>;
}

impl<T: Sync + Send, E: Into<GpmcpError> + Sync + Send> Catch<T> for Result<T, E> {
    fn catch(self) -> Result<T, GpmcpError> {
        let r = anyhow::Result::from(self);

        match r {
            Ok(r) => Ok(r),
            Err(e) => {
                let e = e.into();
                match e.inner.as_ref() {
                    GpmcpErrorInner::ServiceNotFound => {
                        error!("Service not found error caught");
                    }
                    GpmcpErrorInner::McpOperationFailed(_) => {
                        error!("MCP operation failed error caught: {}", e);
                    }
                    GpmcpErrorInner::JoinError(_) => {
                        error!("Join error caught: {}", e);
                    }
                    GpmcpErrorInner::IoError(_) => {
                        error!("IO error caught: {}", e);
                    }
                    GpmcpErrorInner::StdioInitError(_) => {
                        error!("STDIO initialization error caught: {}", e);
                    }
                    GpmcpErrorInner::SseError(_) => {
                        error!("SSE error caught: {}", e);
                    }
                    GpmcpErrorInner::SseInitError(_) => {
                        error!("SSE initialization error caught: {}", e);
                    }
                }
                Err(e)
            }
        }
    }
}
