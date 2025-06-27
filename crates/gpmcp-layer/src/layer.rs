use crate::runner::GpmcpRunnerInner;
use crate::{GpmcpError, RunnerConfig};
use backon::{BackoffBuilder, ExponentialBuilder, Retryable};
use std::future::Future;
use std::sync::Arc;
use tokio::sync::Mutex;

pub struct GpmcpLayer {
    runner_config: RunnerConfig,
    inner: Arc<Mutex<Option<GpmcpRunnerInner>>>,
}

impl GpmcpLayer {
    pub fn new(runner_config: RunnerConfig) -> Result<Self, GpmcpError> {
        // Validate retry config at construction time
        runner_config
            .retry_config
            .validate()
            .map_err(|e| GpmcpError::ConfigurationError(format!("Invalid retry config: {e}")))?;

        Ok(Self {
            runner_config,
            inner: Default::default(),
        })
    }

    /// Creates a configured retry strategy based on the current retry configuration
    fn create_retry_strategy(&self) -> backon::ExponentialBackoff {
        let retry_config = &self.runner_config.retry_config;

        let mut retry_builder = ExponentialBuilder::default()
            .with_min_delay(std::time::Duration::from_millis(retry_config.min_delay_ms))
            .with_max_delay(std::time::Duration::from_millis(retry_config.max_delay_ms))
            .with_max_times(retry_config.max_attempts as usize);

        if retry_config.jitter_factor {
            retry_builder = retry_builder.with_jitter();
        }

        retry_builder.build()
    }

    /// Generic retry mechanism for operations that may fail due to connection issues
    /// Uses backon library with GpmcpError.is_retryable() to determine if an error should be retried
    async fn attempt_with_retry<T, F, Fut>(&self, operation: F) -> Result<T, GpmcpError>
    where
        F: Fn() -> Fut + Send + Sync,
        Fut: Future<Output = Result<T, GpmcpError>> + Send,
        T: Send,
    {
        let retry_config = &self.runner_config.retry_config;

        // If retries are disabled, try once
        if !retry_config.retries_enabled() {
            self.ensure_connected_internal().await?;
            return operation().await;
        }

        // Create the retry strategy
        let retry_strategy = self.create_retry_strategy();

        // Create the operation closure that handles connection management
        let operation_with_connection = || async {
            // Try to ensure connection before each attempt
            self.ensure_connected_internal().await?;

            // Attempt the operation
            operation().await.inspect_err(|e| {
                // Only restart process if the error is retryable and operation retries are enabled
                if e.is_retryable() && retry_config.retry_on_operation_failure {
                    tokio::spawn({
                        let inner = self.inner.clone();
                        async move {
                            let mut lock = inner.lock().await;
                            if let Some(inner) = lock.as_mut() {
                                inner.restart_process().await.ok();
                            }
                        }
                    });
                }
            })
        };

        // Use backon with custom retry condition that respects is_retryable
        operation_with_connection
            .retry(retry_strategy)
            .when(|e: &GpmcpError| e.is_retryable())
            .await
    }

    /// Internal connection management - ensures connection without retry logic
    /// This is used by attempt_with_retry to avoid circular dependencies
    async fn ensure_connected_internal(&self) -> Result<(), GpmcpError> {
        if !self.is_healthy().await {
            // Create new inner instance and connect
            let inner = GpmcpRunnerInner::new(self.runner_config.clone());
            inner.connect().await?;
            *self.inner.lock().await = Some(inner);
        }

        Ok(())
    }

    pub async fn cancel(self) -> Result<(), GpmcpError> {
        let mut lock = self.inner.lock().await;
        if let Some(inner) = lock.take() {
            inner.cancel().await?;
        }
        Ok(())
    }

    pub async fn list_tools(&self) -> Result<rmcp::model::ListToolsResult, GpmcpError> {
        self.attempt_with_retry(|| async {
            let lock = self.inner.lock().await;
            let inner = lock.as_ref().ok_or_else(|| {
                GpmcpError::InvalidState("Connection not established".to_string())
            })?;
            inner.list_tools().await
        })
        .await
    }

    pub async fn call_tool(
        &self,
        request: rmcp::model::CallToolRequestParam,
    ) -> Result<rmcp::model::CallToolResult, GpmcpError> {
        self.attempt_with_retry(|| async {
            let lock = self.inner.lock().await;
            let inner = lock.as_ref().ok_or_else(|| {
                GpmcpError::InvalidState("Connection not established".to_string())
            })?;
            inner.call_tool(request.clone()).await
        })
        .await
    }

    pub async fn list_prompts(&self) -> Result<rmcp::model::ListPromptsResult, GpmcpError> {
        self.attempt_with_retry(|| async {
            let lock = self.inner.lock().await;
            let inner = lock.as_ref().ok_or_else(|| {
                GpmcpError::InvalidState("Connection not established".to_string())
            })?;
            inner.list_prompts().await
        })
        .await
    }

    pub async fn list_resources(&self) -> Result<rmcp::model::ListResourcesResult, GpmcpError> {
        self.attempt_with_retry(|| async {
            let lock = self.inner.lock().await;
            let inner = lock.as_ref().ok_or_else(|| {
                GpmcpError::InvalidState("Connection not established".to_string())
            })?;
            inner.list_resources().await
        })
        .await
    }

    pub async fn get_prompt(
        &self,
        request: rmcp::model::GetPromptRequestParam,
    ) -> Result<rmcp::model::GetPromptResult, GpmcpError> {
        self.attempt_with_retry(|| async {
            let lock = self.inner.lock().await;
            let inner = lock.as_ref().ok_or_else(|| {
                GpmcpError::InvalidState("Connection not established".to_string())
            })?;
            inner.get_prompt(request.clone()).await
        })
        .await
    }

    pub async fn read_resource(
        &self,
        request: rmcp::model::ReadResourceRequestParam,
    ) -> Result<rmcp::model::ReadResourceResult, GpmcpError> {
        self.attempt_with_retry(|| async {
            let lock = self.inner.lock().await;
            let inner = lock.as_ref().ok_or_else(|| {
                GpmcpError::InvalidState("Connection not established".to_string())
            })?;
            inner.read_resource(request.clone()).await
        })
        .await
    }

    /// Get server information - synchronous method, no retry needed
    /// Returns None if not connected, Some(ServerInfo) if available
    pub async fn peer_info(&self) -> Option<rmcp::model::ServerInfo> {
        let lock = self.inner.lock().await;
        if let Some(inner) = lock.as_ref() {
            inner.peer_info().await
        } else {
            None
        }
    }

    /// Health check method - uses list_tools as the litmus test
    /// This is the primary way to check if the server is running and healthy
    pub async fn is_healthy(&self) -> bool {
        // Use list_tools as health check with retry for robustness
        async {
            let lock = self.inner.lock().await;
            let inner = lock.as_ref().ok_or_else(|| {
                GpmcpError::InvalidState("Connection not established".to_string())
            })?;
            inner.list_tools().await?;
            Ok::<_, GpmcpError>(())
        }
        .await
        .is_ok()
    }

    /// Quick health check without retries - for immediate feedback
    /// Returns true only if the connection exists and list_tools succeeds immediately
    pub async fn is_healthy_quick(&self) -> bool {
        let lock = match self.inner.try_lock() {
            Ok(lock) => lock,
            Err(_) => return false, // Can't get lock immediately
        };

        if let Some(inner) = lock.as_ref() {
            inner.list_tools().await.is_ok()
        } else {
            false
        }
    }
}
