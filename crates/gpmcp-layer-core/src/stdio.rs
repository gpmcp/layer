use std::sync::Arc;
use tokio::io::{AsyncWrite, AsyncWriteExt};
use tokio::sync::Mutex;

#[derive(Clone, derive_more::From)]
pub enum LayerStdio {
    Out(LayerStdOut),
    Err(LayerStdErr),
}

impl LayerStdio {
    pub fn inner(&self) -> Arc<Mutex<Box<dyn AsyncWrite + Unpin + Sync + Send>>> {
        match self {
            LayerStdio::Out(out) => out.inner(),
            LayerStdio::Err(err) => err.inner(),
        }
    }
}

pub struct LayerStdOut(Arc<Mutex<Box<dyn AsyncWrite + Unpin + Sync + Send>>>);

impl Clone for LayerStdOut {
    fn clone(&self) -> Self {
        LayerStdOut(self.0.clone())
    }
}
impl LayerStdOut {
    pub fn new(t: Box<dyn AsyncWrite + Unpin + Sync + Send>) -> LayerStdOut {
        LayerStdOut(Arc::new(Mutex::new(t)))
    }
    pub fn inner(&self) -> Arc<Mutex<Box<dyn AsyncWrite + Unpin + Sync + Send>>> {
        self.0.clone()
    }

    pub async fn print(&self, message: &str) {
        let mut lock = self.0.lock().await;
        let _ = lock.write_all(message.as_bytes()).await;
    }
}

pub struct LayerStdErr(Arc<Mutex<Box<dyn AsyncWrite + Unpin + Sync + Send>>>);

impl Clone for LayerStdErr {
    fn clone(&self) -> Self {
        LayerStdErr(self.0.clone())
    }
}

impl LayerStdErr {
    pub fn new(t: Box<dyn AsyncWrite + Unpin + Sync + Send>) -> LayerStdErr {
        LayerStdErr(Arc::new(Mutex::new(t)))
    }
    pub fn inner(&self) -> Arc<Mutex<Box<dyn AsyncWrite + Unpin + Sync + Send>>> {
        self.0.clone()
    }
}
