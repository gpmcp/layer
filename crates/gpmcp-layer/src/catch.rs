use gpmcp_layer_core::GpmcpError;
use std::fmt::{Debug, Display};

pub trait Catch<T, E>: Sync + Send + Sized {
    fn catch(self) -> Result<T, E>;
}

impl<
    T,
    E: Display + Debug + Send + Sync + Into<anyhow::Error> + 'static,
    R: Into<Result<T, E>> + Sync + Send,
> Catch<T, E> for R
{
    fn catch(self) -> Result<T, E> {
        let r = anyhow::Result::from(self.into());

        match r {
            Ok(r) => Ok(r),
            Err(e) => {
                let e = e.into();
                if let Some(err) = e.downcast_ref::<GpmcpError>() {
                    match err {
                        GpmcpError::ServiceNotFound => {}
                        GpmcpError::ConnectionFailed(_) => {}
                        GpmcpError::ProcessError(_) => {}
                        GpmcpError::TransportError(_) => {}
                        GpmcpError::ConfigurationError(_) => {}
                        GpmcpError::Timeout(_) => {}
                        GpmcpError::Cancelled => {}
                        GpmcpError::RetryLimitExceeded => {}
                        GpmcpError::ServiceInitializationFailed(_) => {}
                        GpmcpError::ServerNotReady(_) => {}
                        GpmcpError::McpOperationFailed(_) => {}
                        GpmcpError::InvalidState(_) => {}
                        GpmcpError::ResourceNotFound(_) => {}
                        GpmcpError::PermissionDenied(_) => {}
                        GpmcpError::NetworkError(_) => {}
                        GpmcpError::SerializationError(_) => {}
                        GpmcpError::Other(_) => {}
                    }
                }
                Err(e.downcast::<E>().unwrap())
            }
        }
    }
}
