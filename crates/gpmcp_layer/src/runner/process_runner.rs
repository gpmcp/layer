use crate::RunnerConfig;
use crate::runner::inner::GpmcpRunnerInner;
use anyhow::{Context, Result};
use backon::{BackoffBuilder, ExponentialBackoff, ExponentialBuilder, Retryable};
use std::future::Future;
use std::sync::Arc;
use tokio::sync::Mutex;

pub struct GpmcpLayer {
    runner_config: RunnerConfig,
    inner: Arc<Mutex<Option<GpmcpRunnerInner>>>,
}

impl GpmcpLayer {
    pub fn new(runner_config: RunnerConfig) -> anyhow::Result<Self> {
        // Validate retry config at construction time
        runner_config.retry_config.validate()?;

        Ok(Self {
            runner_config,
            inner: Default::default(),
        })
    }

    /// Create a new ExponentialBackoff strategy based on the current retry config
    /// This is called each time we need to retry, but the creation is lightweight
    fn create_retry_strategy(&self) -> Option<ExponentialBackoff> {
        let retry_config = &self.runner_config.retry_config;

        if !retry_config.retries_enabled() {
            return None;
        }

        let strategy = if retry_config.use_exponential_backoff {
            ExponentialBuilder::default()
                .with_min_delay(retry_config.min_delay())
                .with_max_delay(retry_config.max_delay())
                .with_max_times(retry_config.max_attempts as usize)
                .build()
        } else {
            // For fixed delay, use min_delay as the fixed interval
            ExponentialBuilder::default()
                .with_min_delay(retry_config.min_delay())
                .with_max_delay(retry_config.min_delay()) // Same as min for fixed delay
                .with_max_times(retry_config.max_attempts as usize)
                .build()
        };

        Some(strategy)
    }

    /// Generic retry mechanism for operations that may fail due to connection issues
    /// Creates retry strategy on-demand based on stored configuration
    async fn attempt_with_retry<T, F, Fut>(&self, operation: F) -> Result<T>
    where
        F: Fn() -> Fut + Send + Sync,
        Fut: Future<Output = Result<T>> + Send,
        T: Send,
    {
        let retry_config = &self.runner_config.retry_config;

        // If retries are disabled, just try once
        let Some(retry_strategy) = self.create_retry_strategy() else {
            if retry_config.retry_on_connection_failure {
                self.ensure_connected_internal().await?;
            }
            return operation().await;
        };

        (|| async {
            // Try to ensure connection before each attempt
            if retry_config.retry_on_connection_failure {
                self.ensure_connected_internal().await?;
            }

            // Attempt the operation
            operation().await.inspect_err(|_e| {
                // Clear connection on failure if operation retries are enabled
                if retry_config.retry_on_operation_failure {
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
        })
        .retry(retry_strategy)
        .await
    }

    /// Internal connection management - ensures connection without retry logic
    /// This is used by attempt_with_retry to avoid circular dependencies
    async fn ensure_connected_internal(&self) -> Result<()> {
        let mut lock = self.inner.lock().await;

        if lock.is_none() {
            // Create new inner instance and connect
            let inner = GpmcpRunnerInner::new(self.runner_config.clone());
            inner
                .connect()
                .await
                .context("Failed to connect to service")?;
            *lock = Some(inner);
        }

        Ok(())
    }

    pub async fn cancel(self) -> Result<()> {
        let mut lock = self.inner.lock().await;
        if let Some(inner) = lock.take() {
            inner
                .cancel()
                .await
                .context("Failed to cancel GpmcpRunner")?;
        }
        Ok(())
    }

    pub async fn list_tools(&self) -> Result<rmcp::model::ListToolsResult> {
        self.attempt_with_retry(|| async {
            let lock = self.inner.lock().await;
            let inner = lock
                .as_ref()
                .ok_or_else(|| anyhow::anyhow!("Connection not established"))?;
            let result = inner
                .list_tools()
                .await
                .map_err(|e| anyhow::anyhow!("list_tools failed: {}", e))?;
            Ok(result)
        })
        .await
    }

    pub async fn call_tool(
        &self,
        request: rmcp::model::CallToolRequestParam,
    ) -> Result<rmcp::model::CallToolResult> {
        self.attempt_with_retry(|| async {
            let lock = self.inner.lock().await;
            let inner = lock
                .as_ref()
                .ok_or_else(|| anyhow::anyhow!("Connection not established"))?;
            let result = inner
                .call_tool(request.clone())
                .await
                .map_err(|e| anyhow::anyhow!("call_tool failed: {}", e))?;
            Ok(result)
        })
        .await
    }

    pub async fn list_prompts(&self) -> Result<rmcp::model::ListPromptsResult> {
        self.attempt_with_retry(|| async {
            let lock = self.inner.lock().await;
            let inner = lock
                .as_ref()
                .ok_or_else(|| anyhow::anyhow!("Connection not established"))?;
            let result = inner
                .list_prompts()
                .await
                .map_err(|e| anyhow::anyhow!("list_prompts failed: {}", e))?;
            Ok(result)
        })
        .await
    }

    pub async fn list_resources(&self) -> Result<rmcp::model::ListResourcesResult> {
        self.attempt_with_retry(|| async {
            let lock = self.inner.lock().await;
            let inner = lock
                .as_ref()
                .ok_or_else(|| anyhow::anyhow!("Connection not established"))?;
            let result = inner
                .list_resources()
                .await
                .map_err(|e| anyhow::anyhow!("list_resources failed: {}", e))?;
            Ok(result)
        })
        .await
    }

    pub async fn get_prompt(
        &self,
        request: rmcp::model::GetPromptRequestParam,
    ) -> Result<rmcp::model::GetPromptResult> {
        self.attempt_with_retry(|| async {
            let lock = self.inner.lock().await;
            let inner = lock
                .as_ref()
                .ok_or_else(|| anyhow::anyhow!("Connection not established"))?;
            let result = inner
                .get_prompt(request.clone())
                .await
                .map_err(|e| anyhow::anyhow!("get_prompt failed: {}", e))?;
            Ok(result)
        })
        .await
    }

    pub async fn read_resource(
        &self,
        request: rmcp::model::ReadResourceRequestParam,
    ) -> Result<rmcp::model::ReadResourceResult> {
        self.attempt_with_retry(|| async {
            let lock = self.inner.lock().await;
            let inner = lock
                .as_ref()
                .ok_or_else(|| anyhow::anyhow!("Connection not established"))?;
            let result = inner
                .read_resource(request.clone())
                .await
                .map_err(|e| anyhow::anyhow!("read_resource failed: {}", e))?;
            Ok(result)
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
        self.attempt_with_retry(|| async {
            let lock = self.inner.lock().await;
            let inner = lock
                .as_ref()
                .ok_or_else(|| anyhow::anyhow!("Connection not established"))?;
            inner
                .list_tools()
                .await
                .map_err(|e| anyhow::anyhow!("Health check failed: {}", e))?;
            Ok(())
        })
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
