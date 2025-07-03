use crate::error::GpmcpErrorInner;
use crate::GpmcpError;

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
                    GpmcpErrorInner::ServiceNotFound => {}
                    GpmcpErrorInner::McpOperationFailed(_) => {}
                    GpmcpErrorInner::JoinError(_) => {}
                    GpmcpErrorInner::IoError(_) => {}
                    GpmcpErrorInner::StdioInitError(_) => {}
                    GpmcpErrorInner::SseError(_) => {}
                    GpmcpErrorInner::SseInitError(_) => {}
                }
                Err(e)
            }
        }
    }
}
