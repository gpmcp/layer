use crate::config::RunnerConfig;
use crate::process::ProcessHandle;
use crate::{LayerStdErr, LayerStdOut, LayerStdio};
use anyhow::Result;
use async_trait::async_trait;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio_stream::StreamExt;
use tokio_util::bytes::{Buf, BytesMut};
use tokio_util::codec::{Decoder, FramedRead};

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
/// use anyhow::Result;///
///
/// use gpmcp_layer_core::config::RunnerConfig;
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
    /// The type of process handle returned by this process manager
    type Handle: ProcessHandle;

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
    fn new(runner_config: &RunnerConfig) -> Self
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
    async fn start_server(&self, out: LayerStdOut, err: LayerStdErr) -> Result<Self::Handle>;

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
    fn create_process_manager(config: &RunnerConfig) -> Self::Manager;
}

struct Utf8Codec;

impl Decoder for Utf8Codec {
    type Item = String;
    type Error = anyhow::Error;
    fn decode(&mut self, src: &mut BytesMut) -> Result<Option<Self::Item>, Self::Error> {
        if src.is_empty() {
            return Ok(None);
        }

        match str::from_utf8(src) {
            Ok(s) => {
                let out = s.to_owned();
                src.clear();

                if out.is_empty() {
                    return Ok(None);
                }

                Ok(Some(out))
            }
            Err(e) if e.error_len().is_none() => {
                let valid = e.valid_up_to();

                if valid == 0 {
                    return Ok(None);
                }

                let out = str::from_utf8(&src[..valid])?.to_owned();
                src.advance(valid);
                Ok(Some(out))
            }
            Err(e) => Err(std::io::Error::new(std::io::ErrorKind::InvalidData, e).into()),
        }
    }
}
pub async fn stream<A: AsyncReadExt + Unpin + 'static>(
    io: &mut A,
    out: impl Into<LayerStdio>,
) -> tokio::io::Result<()> {
    let mut frames = FramedRead::with_capacity(io, Utf8Codec, 1024);
    stream_frames(&mut frames, out.into()).await
}

async fn stream_frames<R: AsyncReadExt + Unpin>(
    frames: &mut FramedRead<R, Utf8Codec>,
    out: LayerStdio,
) -> tokio::io::Result<()> {
    while let Some(frame) = frames.next().await {
        match frame {
            Ok(text) => {
                let bytes = text.as_bytes();
                out.inner().lock().await.write_all(bytes).await?;
                out.inner().lock().await.flush().await?;
            }
            Err(e) => {
                return Err(tokio::io::Error::new(tokio::io::ErrorKind::InvalidData, e));
            }
        }
    }

    Ok(())
}
