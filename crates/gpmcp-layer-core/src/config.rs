use derive_builder::Builder;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;

/// Configuration for retry logic used throughout the application
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct RetryConfig {
    /// Minimum delay between retry attempts (in milliseconds)
    #[serde(default = "default_min_delay_ms")]
    pub min_delay_ms: u64,

    /// Maximum delay between retry attempts (in milliseconds)
    #[serde(default = "default_max_delay_ms")]
    pub max_delay_ms: u64,

    /// Maximum number of retry attempts (0 means no retries, just one attempt)
    #[serde(default = "default_max_attempts")]
    pub max_attempts: u32,

    /// Whether to use exponential backoff (true) or fixed delay (false)
    #[serde(default = "default_use_exponential_backoff")]
    pub use_exponential_backoff: bool,

    /// Jitter factor for randomizing delays (0.0 to 1.0)
    /// 0.0 = no jitter, 1.0 = up to 100% jitter
    #[serde(default = "default_jitter_factor")]
    pub jitter_factor: f64,

    /// Whether to retry on connection failures
    #[serde(default = "default_retry_on_connection_failure")]
    pub retry_on_connection_failure: bool,

    /// Whether to retry on operation failures (like list_tools, call_tool)
    #[serde(default = "default_retry_on_operation_failure")]
    pub retry_on_operation_failure: bool,
}

impl Default for RetryConfig {
    fn default() -> Self {
        Self {
            min_delay_ms: default_min_delay_ms(),
            max_delay_ms: default_max_delay_ms(),
            max_attempts: default_max_attempts(),
            use_exponential_backoff: default_use_exponential_backoff(),
            jitter_factor: default_jitter_factor(),
            retry_on_connection_failure: default_retry_on_connection_failure(),
            retry_on_operation_failure: default_retry_on_operation_failure(),
        }
    }
}

impl RetryConfig {
    /// Create a new RetryConfig with sensible defaults
    pub fn new() -> Self {
        Self::default()
    }

    /// Create a RetryConfig with aggressive retry settings
    pub fn aggressive() -> Self {
        Self {
            min_delay_ms: 50,
            max_delay_ms: 10_000, // 10 seconds
            max_attempts: 5,
            use_exponential_backoff: true,
            jitter_factor: 0.1,
            retry_on_connection_failure: true,
            retry_on_operation_failure: true,
        }
    }

    /// Create a RetryConfig with conservative retry settings
    pub fn conservative() -> Self {
        Self {
            min_delay_ms: 500,
            max_delay_ms: 2_000, // 2 seconds
            max_attempts: 2,
            use_exponential_backoff: false,
            jitter_factor: 0.0,
            retry_on_connection_failure: true,
            retry_on_operation_failure: false,
        }
    }

    /// Create a RetryConfig with no retries (fail fast)
    pub fn no_retry() -> Self {
        Self {
            min_delay_ms: 0,
            max_delay_ms: 0,
            max_attempts: 1, // One attempt, no retries
            use_exponential_backoff: false,
            jitter_factor: 0.0,
            retry_on_connection_failure: false,
            retry_on_operation_failure: false,
        }
    }

    /// Validate the configuration and return errors if invalid
    pub fn validate(&self) -> anyhow::Result<()> {
        if self.min_delay_ms > self.max_delay_ms {
            return Err(anyhow::anyhow!(
                "min_delay_ms cannot be greater than max_delay_ms"
            ));
        }

        if self.jitter_factor < 0.0 || self.jitter_factor > 1.0 {
            return Err(anyhow::anyhow!("jitter_factor must be between 0.0 and 1.0"));
        }

        if self.max_attempts > 10 {
            return Err(anyhow::anyhow!(
                "max_attempts should not exceed 10 to avoid excessive retries"
            ));
        }

        if self.max_delay_ms > 60_000 {
            return Err(anyhow::anyhow!("max_delay_ms should not exceed 60 seconds"));
        }

        Ok(())
    }

    /// Get the minimum delay as Duration
    pub fn min_delay(&self) -> std::time::Duration {
        std::time::Duration::from_millis(self.min_delay_ms)
    }

    /// Get the maximum delay as Duration
    pub fn max_delay(&self) -> std::time::Duration {
        std::time::Duration::from_millis(self.max_delay_ms)
    }

    /// Check if retries are enabled (more than 1 attempt)
    pub fn retries_enabled(&self) -> bool {
        self.max_attempts > 1
            && (self.retry_on_connection_failure || self.retry_on_operation_failure)
    }
}

/// Transport configuration
#[derive(Default, Debug, Clone, PartialEq)]
pub enum Transport {
    #[default]
    Stdio,
    Sse {
        url: String,
    },
}

/// Main runner configuration
#[derive(Default, Debug, Clone, PartialEq, Builder)]
#[builder(setter(into, strip_option))]
pub struct RunnerConfig {
    pub name: String,
    pub version: String,
    pub command: String,
    #[builder(default)]
    #[builder(setter(custom))]
    pub args: Vec<String>,
    #[builder(default)]
    #[builder(setter(custom))]
    pub env: HashMap<String, String>,
    #[builder(default)]
    pub transport: Transport,
    pub working_directory: Option<PathBuf>,
    #[builder(default)]
    pub retry_config: RetryConfig,
}

impl RunnerConfig {
    pub fn builder() -> RunnerConfigBuilder {
        RunnerConfigBuilder::default()
    }
}

impl RunnerConfigBuilder {
    pub fn args<S: ToString, I: IntoIterator<Item = S>>(&mut self, iter: I) -> &mut Self {
        let args: Vec<String> = iter.into_iter().map(|s| s.to_string()).collect();
        self.args = Some(args);
        self
    }
    pub fn env<T: ToString>(&mut self, key: T, value: T) -> &mut Self {
        let map = self.env.get_or_insert_with(HashMap::new);
        map.insert(key.to_string(), value.to_string());

        self
    }

    pub fn env_multi<T: ToString, I: IntoIterator<Item = (T, T)>>(&mut self, iter: I) -> &mut Self {
        let env = self.env.get_or_insert_with(HashMap::new);
        for (key, value) in iter {
            env.insert(key.to_string(), value.to_string());
        }
        self
    }
}

// Default value functions for serde
fn default_min_delay_ms() -> u64 {
    100
}
fn default_max_delay_ms() -> u64 {
    5_000
}
fn default_max_attempts() -> u32 {
    3
}
fn default_use_exponential_backoff() -> bool {
    true
}
fn default_jitter_factor() -> f64 {
    0.1
}
fn default_retry_on_connection_failure() -> bool {
    true
}
fn default_retry_on_operation_failure() -> bool {
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config() {
        let config = RetryConfig::default();
        assert!(config.validate().is_ok());
        assert!(config.retries_enabled());
    }

    #[test]
    fn test_aggressive_config() {
        let config = RetryConfig::aggressive();
        assert!(config.validate().is_ok());
        assert_eq!(config.max_attempts, 5);
        assert_eq!(config.max_delay_ms, 10_000);
    }

    #[test]
    fn test_conservative_config() {
        let config = RetryConfig::conservative();
        assert!(config.validate().is_ok());
        assert_eq!(config.max_attempts, 2);
        assert!(!config.use_exponential_backoff);
    }

    #[test]
    fn test_no_retry_config() {
        let config = RetryConfig::no_retry();
        assert!(config.validate().is_ok());
        assert!(!config.retries_enabled());
    }

    #[test]
    fn test_invalid_config() {
        let mut config = RetryConfig {
            min_delay_ms: 1000,
            max_delay_ms: 500,
            ..Default::default()
        };
        assert!(config.validate().is_err());

        config.min_delay_ms = 100;
        config.max_delay_ms = 1000;
        config.jitter_factor = 1.5;
        assert!(config.validate().is_err());
    }

    #[test]
    fn test_serialization() {
        let config = RetryConfig::aggressive();
        let json = serde_json::to_string(&config).unwrap();
        let deserialized: RetryConfig = serde_json::from_str(&json).unwrap();
        assert_eq!(config.max_attempts, deserialized.max_attempts);
    }
}
#[test]
fn test_no_retry_behavior() {
    let config = RetryConfig::no_retry();
    assert_eq!(config.max_attempts, 1);
    assert!(!config.retries_enabled()); // No retries, but still 1 attempt
    assert!(!config.retry_on_connection_failure);
    assert!(!config.retry_on_operation_failure);
}

#[test]
fn test_retry_attempts_logic() {
    // Test that max_attempts represents total attempts
    let config = RetryConfig {
        max_attempts: 3,
        retry_on_connection_failure: true,
        ..Default::default()
    };
    assert!(config.retries_enabled()); // 3 attempts = 1 initial + 2 retries

    let config = RetryConfig {
        max_attempts: 1,
        retry_on_connection_failure: true,
        ..Default::default()
    };
    assert!(!config.retries_enabled()); // 1 attempt = no retries
}
