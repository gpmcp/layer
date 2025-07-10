use crate::config::{RetryConfig, RunnerConfig};
use crate::error::GpmcpError;
use crate::process_manager_trait::RunnerProcessManager;
use crate::runner::inner::GpmcpRunnerInner;
use crate::{LayerStdErr, LayerStdOut};
use backon::{ExponentialBuilder, Retryable};
use std::future::Future;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use tokio::sync::Mutex;

pub struct Initialized;

pub struct Uninitialized;

#[derive(Clone)]
pub struct GpmcpLayer<Status, Manager> {
    runner_config: RunnerConfig,
    process_manager: Arc<Manager>,
    inner: Arc<Mutex<GpmcpRunnerInner<Status, Manager>>>,
    out: LayerStdOut,
    err: LayerStdErr,
    retry_config: ExponentialBuilder,
}

impl<Manager: RunnerProcessManager> GpmcpLayer<Uninitialized, Manager> {
    pub fn new(runner_config: RunnerConfig, process_manager: Arc<Manager>) -> Self {
        let out = LayerStdOut::new(Box::new(tokio::io::stdout()));
        let err = LayerStdErr::new(Box::new(tokio::io::stderr()));
        Self::new_with_buffers(runner_config, process_manager, out, err)
    }
}

impl<Manager: RunnerProcessManager> GpmcpLayer<Uninitialized, Manager> {
    pub fn new_with_buffers(
        runner_config: RunnerConfig,
        process_manager: Arc<Manager>,
        out: LayerStdOut,
        err: LayerStdErr,
    ) -> Self {
        Self {
            inner: Arc::new(Mutex::new(GpmcpRunnerInner::new(
                runner_config.clone(),
                process_manager.clone(),
                out.clone(),
                err.clone(),
            ))),
            out,
            err,
            retry_config: Self::create_retry_strategy(&runner_config.retry_config),
            runner_config,
            process_manager,
        }
    }

    pub async fn connect(self) -> Result<GpmcpLayer<Initialized, Manager>, GpmcpError> {
        let initialized_inner = self.inner.lock().await.connect().await?;
        Ok(GpmcpLayer {
            runner_config: self.runner_config,
            retry_config: self.retry_config,
            inner: Arc::new(Mutex::new(initialized_inner)),
            out: self.out.clone(),
            err: self.err.clone(),
            process_manager: self.process_manager,
        })
    }
    /// Creates a configured retry strategy based on the current retry configuration
    fn create_retry_strategy(retry_config: &RetryConfig) -> ExponentialBuilder {
        let mut retry_builder = ExponentialBuilder::default()
            .with_min_delay(std::time::Duration::from_millis(retry_config.min_delay_ms))
            .with_max_delay(std::time::Duration::from_millis(retry_config.max_delay_ms))
            .with_max_times(retry_config.max_attempts as usize);

        if retry_config.jitter_factor {
            retry_builder = retry_builder.with_jitter();
        }

        retry_builder
    }
}
impl<Manager: RunnerProcessManager> GpmcpLayer<Initialized, Manager> {
    /// Generic retry mechanism for operations that may fail due to connection issues
    /// Uses backon library with GpmcpError.is_retryable() to determine if an error should be retried
    async fn attempt_with_retry<T, F, Fut>(&self, operation: F) -> Result<T, GpmcpError>
    where
        F: Fn() -> Fut + Send + Sync,
        Fut: Future<Output = Result<T, GpmcpError>> + Send,
        T: Send,
    {
        let is_retry = AtomicBool::new(false);

        // Create the operation closure that handles connection management
        let operation_with_connection = || async {
            if is_retry.load(std::sync::atomic::Ordering::Relaxed) {
                let new = GpmcpRunnerInner::new(
                    self.runner_config.clone(),
                    self.process_manager.clone(),
                    self.out.clone(),
                    self.err.clone(),
                );
                *self.inner.lock().await = new.connect().await?;
            }
            operation().await
        };

        // Use backon with custom retry condition that respects is_retryable
        operation_with_connection
            .retry(self.retry_config)
            .when(|e: &GpmcpError| e.is_retryable())
            .notify(|_, _| is_retry.store(true, std::sync::atomic::Ordering::Relaxed))
            .await
    }

    pub async fn cancel(self) -> Result<(), GpmcpError> {
        self.inner.lock().await.cancel().await
    }

    pub async fn list_tools(&self) -> Result<rmcp::model::ListToolsResult, GpmcpError> {
        self.attempt_with_retry(|| async { self.inner.lock().await.list_tools().await })
            .await
    }

    pub async fn call_tool(
        &self,
        request: rmcp::model::CallToolRequestParam,
    ) -> Result<rmcp::model::CallToolResult, GpmcpError> {
        self.attempt_with_retry(|| async {
            self.inner.lock().await.call_tool(request.clone()).await
        })
        .await
    }

    pub async fn list_prompts(&self) -> Result<rmcp::model::ListPromptsResult, GpmcpError> {
        self.attempt_with_retry(|| async { self.inner.lock().await.list_prompts().await })
            .await
    }

    pub async fn list_resources(&self) -> Result<rmcp::model::ListResourcesResult, GpmcpError> {
        self.attempt_with_retry(|| async { self.inner.lock().await.list_resources().await })
            .await
    }

    pub async fn get_prompt(
        &self,
        request: rmcp::model::GetPromptRequestParam,
    ) -> Result<rmcp::model::GetPromptResult, GpmcpError> {
        self.attempt_with_retry(|| async {
            self.inner.lock().await.get_prompt(request.clone()).await
        })
        .await
    }

    pub async fn read_resource(
        &self,
        request: rmcp::model::ReadResourceRequestParam,
    ) -> Result<rmcp::model::ReadResourceResult, GpmcpError> {
        self.attempt_with_retry(|| async {
            self.inner.lock().await.read_resource(request.clone()).await
        })
        .await
    }

    /// Get server information - synchronous method, no retry needed
    /// Returns None if not connected, Some(ServerInfo) if available
    pub async fn peer_info(&self) -> Option<rmcp::model::ServerInfo> {
        self.inner.lock().await.peer_info().await
    }

    /// Health check method - uses list_tools as the litmus test
    /// This is the primary way to check if the server is running and healthy
    pub async fn is_healthy(&self) -> bool {
        // Use list_tools as health check with retry for robustness
        async {
            self.inner.lock().await.list_tools().await?;
            Ok::<_, GpmcpError>(())
        }
        .await
        .is_ok()
    }
}
