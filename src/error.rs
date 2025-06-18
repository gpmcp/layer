#[derive(Debug, thiserror::Error)]
pub enum GpmcpError {
    #[error("Process manager not initialized")]
    ProcessManagerNotFound,
    #[error("Service coordinator not initialized")]
    ServiceNotFound,
    #[error("Unexpected error: {0}")]
    Other(anyhow::Error),
}
