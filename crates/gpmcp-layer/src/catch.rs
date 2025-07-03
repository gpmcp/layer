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
                        error!(error=%e, "MCP operation failed error caught");
                    }
                    GpmcpErrorInner::JoinError(_) => {
                        error!(error=%e, "Join error caught");
                    }
                    GpmcpErrorInner::IoError(_) => {
                        error!(error=%e, "IO error caught");
                    }
                    GpmcpErrorInner::StdioInitError(_) => {
                        error!(error=%e, "STDIO initialization error caught");
                    }
                    GpmcpErrorInner::SseError(_) => {
                        error!(error=%e, "SSE error caught");
                    }
                    GpmcpErrorInner::SseInitError(_) => {
                        error!(error=%e, "SSE initialization error caught");
                    }
                }
                Err(e)
            }
        }
    }
}
