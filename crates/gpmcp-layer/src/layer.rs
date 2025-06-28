use crate::runner::{GpmcpRunnerInner, Initialized, Uninitialized};
use crate::{GpmcpError, RunnerConfig};
use backon::{ExponentialBuilder, Retryable};
use gpmcp_layer_core::RetryConfig;
use std::future::Future;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use tokio::sync::Mutex;

#[derive(Clone)]
pub struct GpmcpLayer<Status> {
    runner_config: RunnerConfig,
    inner: Arc<Mutex<GpmcpRunnerInner<Status>>>,
    retry_config: ExponentialBuilder,
}
impl GpmcpLayer<Uninitialized> {
    pub fn new(runner_config: RunnerConfig) -> Result<Self, GpmcpError> {
        // Validate retry config at construction time
        runner_config
            .retry_config
            .validate()
            .map_err(|e| GpmcpError::ConfigurationError(format!("Invalid retry config: {e}")))?;

        Ok(Self {
            inner: Arc::new(Mutex::new(GpmcpRunnerInner::new(runner_config.clone()))),
            retry_config: Self::create_retry_strategy(&runner_config.retry_config),
            runner_config,
        })
    }
    pub async fn connect(self) -> Result<GpmcpLayer<Initialized>, GpmcpError> {
        let initialized_inner = self.inner.lock().await.connect().await?;
        Ok(GpmcpLayer {
            runner_config: self.runner_config,
            retry_config: self.retry_config,
            inner: Arc::new(Mutex::new(initialized_inner)),
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
impl GpmcpLayer<Initialized> {
    /// Generic retry mechanism for operations that may fail due to connection issues
    /// Uses backon library with GpmcpError.is_retryable() to determine if an error should be retried
    async fn attempt_with_retry<T, F, Fut>(&self, operation: F) -> Result<T, GpmcpError>
    where
        F: Fn() -> Fut + Send + Sync,
        Fut: Future<Output = Result<T, GpmcpError>> + Send,
        T: Send,
    {
        // Create the retry strategy
        let is_retry = AtomicBool::new(false);

        // Create the operation closure that handles connection management
        let operation_with_connection = || async {
            if is_retry.load(std::sync::atomic::Ordering::Relaxed) {
                let new = GpmcpRunnerInner::new(self.runner_config.clone());
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
