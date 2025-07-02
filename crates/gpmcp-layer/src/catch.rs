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

                Err(e)
            }
        }
    }
}
