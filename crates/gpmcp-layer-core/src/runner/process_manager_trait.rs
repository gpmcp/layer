use anyhow::Result;
use async_trait::async_trait;
use std::collections::HashMap;

use crate::{ProcessHandle, ProcessId, RunnerConfig};

/// High-level process manager trait for platform-independent process orchestration
/// 
/// This trait defines the interface for managing processes at a high level, including
/// configuration-aware process spawning, active process tracking, and coordinated cleanup.
/// It is designed to be implemented by platform-specific crates that compose the low-level
/// ProcessManager trait for actual process operations.
/// 
/// # Design Philosophy
/// 
/// This trait focuses on orchestration and configuration integration rather than low-level
/// process operations. It provides:
/// - Configuration-driven server startup using RunnerConfig
/// - Active process tracking and lifecycle management
/// - Coordinated cleanup of all managed processes
/// - Platform-independent interface for high-level operations
/// 
/// # Implementation Notes
/// 
/// Implementations should:
/// - Internally use the low-level ProcessManager trait for actual process operations
/// - Maintain active process tracking with appropriate synchronization
/// - Handle RunnerConfig integration for server startup operations
/// - Provide emergency cleanup in Drop implementations
/// - Use platform-appropriate synchronization primitives
/// 
/// # Example Usage
/// 
/// ```rust,no_run
/// use gpmcp_layer_core::{RunnerProcessManager, RunnerConfig};
/// use anyhow::Result;
/// 
/// async fn example_usage() -> Result<()> {
///     let config = RunnerConfig::builder()
///         .name("my-server")
///         .command("node")
///         .args(["server.js"])
///         .build()?;
///     
///     // Platform-specific manager would be created here
///     // let manager = SomePlatformProcessManager::new(&config).await?;
///     // let server_handle = manager.start_server().await?;
///     
///     // Do work with server...
///     
///     // manager.cleanup().await?;
///     Ok(())
/// }
/// ```
#[async_trait]
pub trait RunnerProcessManager: Send + Sync {
    /// Create a new process manager instance with the given configuration
    /// 
    /// The configuration will be used for server startup operations and should be
    /// stored internally for use by `start_server()`.
    /// 
    /// # Arguments
    /// 
    /// * `runner_config` - Configuration for process management and server startup
    /// 
    /// # Returns
    /// 
    /// Returns a new instance of the process manager or an error if initialization fails.
    async fn new(runner_config: &RunnerConfig) -> Result<Self>
    where
        Self: Sized;

    /// Start the server process using the configuration provided during construction
    /// 
    /// This method uses the RunnerConfig provided to `new()` to start a server process
    /// with the configured command, arguments, environment variables, and working directory.
    /// The started process will be automatically tracked for cleanup.
    /// 
    /// # Returns
    /// 
    /// Returns a handle to the started server process or an error if startup fails.
    /// 
    /// # Example
    /// 
    /// ```rust,no_run
    /// // let server_handle = manager.start_server().await?;
    /// // println!("Server started with PID: {:?}", server_handle.get_pid());
    /// ```
    async fn start_server(&self) -> Result<Box<dyn ProcessHandle>>;

    /// Spawn a new process with the given parameters and track it for cleanup
    /// 
    /// This method spawns a process with the specified command and arguments, optionally
    /// setting a working directory and environment variables. The spawned process will
    /// be automatically tracked and included in cleanup operations.
    /// 
    /// # Arguments
    /// 
    /// * `command` - The command to execute
    /// * `args` - Command-line arguments for the process
    /// * `working_dir` - Optional working directory for the process
    /// * `env` - Optional environment variables for the process
    /// 
    /// # Returns
    /// 
    /// Returns a handle to the spawned process or an error if spawning fails.
    /// 
    /// # Example
    /// 
    /// ```rust,no_run
    /// // let handle = manager.spawn_process(
    /// //     "node",
    /// //     &["script.js".to_string()],
    /// //     Some("/path/to/working/dir"),
    /// //     Some(&env_vars)
    /// // ).await?;
    /// ```
    async fn spawn_process(
        &self,
        command: &str,
        args: &[String],
        working_dir: Option<&str>,
        env: Option<&HashMap<String, String>>,
    ) -> Result<Box<dyn ProcessHandle>>;

    /// Cleanup all tracked processes and release resources
    /// 
    /// This method performs coordinated cleanup of all processes managed by this instance.
    /// It should:
    /// - Terminate all tracked processes using appropriate strategies
    /// - Clear internal process tracking state
    /// - Release any platform-specific resources
    /// - Delegate to the underlying ProcessManager for platform-specific cleanup
    /// 
    /// # Returns
    /// 
    /// Returns Ok(()) if cleanup succeeds, or an error if cleanup fails.
    /// 
    /// # Implementation Notes
    /// 
    /// Implementations should use the underlying ProcessManager's termination strategies,
    /// typically starting with graceful termination and escalating to force termination
    /// if necessary. Process trees should be terminated to ensure complete cleanup.
    /// 
    /// # Example
    /// 
    /// ```rust,no_run
    /// // Cleanup all managed processes before shutdown
    /// // manager.cleanup().await?;
    /// ```
    async fn cleanup(&self) -> Result<()>;

    /// Get the number of currently tracked active processes
    /// 
    /// This method returns the count of processes currently being tracked by this
    /// process manager instance. This can be useful for monitoring and debugging.
    /// 
    /// # Returns
    /// 
    /// Returns the number of active tracked processes.
    fn active_process_count(&self) -> usize;

    /// Get information about all currently tracked processes
    /// 
    /// This method returns a snapshot of all currently tracked processes, including
    /// their process IDs and the commands that started them. This is useful for
    /// monitoring and debugging purposes.
    /// 
    /// # Returns
    /// 
    /// Returns a vector of tuples containing (ProcessId, command_name) for each
    /// tracked process.
    fn get_tracked_processes(&self) -> Vec<(ProcessId, String)>;
}

/// Factory trait for creating platform-specific RunnerProcessManager implementations
/// 
/// This trait provides a platform-independent way to create RunnerProcessManager
/// instances. Platform-specific crates should implement this trait to provide
/// their own process manager implementations.
/// 
/// # Example Implementation
/// 
/// ```rust,no_run
/// // pub struct UnixRunnerProcessManagerFactory;
/// // 
/// // impl RunnerProcessManagerFactory for UnixRunnerProcessManagerFactory {
/// //     type Manager = UnixRunnerProcessManager;
/// //     
/// //     async fn create_process_manager(config: &RunnerConfig) -> Result<Self::Manager> {
/// //         UnixRunnerProcessManager::new(config).await
/// //     }
/// //     
/// //     fn platform_name() -> &'static str {
/// //         "unix"
/// //     }
/// // }
/// ```
#[async_trait]
pub trait RunnerProcessManagerFactory {
    /// The type of process manager this factory creates
    type Manager: RunnerProcessManager;

    /// Create a new process manager instance for the current platform
    /// 
    /// # Arguments
    /// 
    /// * `config` - Configuration for the process manager
    /// 
    /// # Returns
    /// 
    /// Returns a new process manager instance or an error if creation fails.
    async fn create_process_manager(config: &RunnerConfig) -> Result<Self::Manager>;

    /// Get the platform name for logging and debugging
    /// 
    /// # Returns
    /// 
    /// Returns a static string identifying the platform (e.g., "windows", "unix").
    fn platform_name() -> &'static str;
}