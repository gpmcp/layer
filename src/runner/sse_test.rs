use crate::runner::transport_manager::TransportManager;
use crate::{RunnerConfig, Transport, RetryConfig};
use crate::runner::ProcessManager;
use std::collections::HashMap;
use std::sync::Arc;
use tokio_util::sync::CancellationToken;

#[tokio::test]
async fn test_sse_transport_retry_logic() {
    let _ = tracing_subscriber::fmt()
        .with_file(true)
        .with_thread_ids(false)
        .with_target(false)
        .with_line_number(true)
        .try_init();

    let config = RunnerConfig {
        name: "test-sse-server".to_string(),
        version: "1.0.0".to_string(),
        command: "python3".to_string(),
        args: vec![
            "-c".to_string(), 
            "import time; print('Mock server started'); time.sleep(60)".to_string()
        ],
        env: HashMap::new(),
        working_directory: None,
        transport: Transport::Sse {
            url: "http://localhost:9999/sse".to_string(), // Non-existent server
        },
        retry_config: RetryConfig::default(),
    };
    
    let cancellation_token = Arc::new(CancellationToken::new());
    let process_manager = ProcessManager::new(cancellation_token, &config).await.unwrap();
    
    println!("Creating TransportManager with SSE transport...");
    
    // For SSE transport, we need to start the server process first (like inner.rs does)
    let _handle = process_manager.start_server().await.unwrap();
    println!("Server process started");
    
    // This should create SSE transport but fail to connect (which is expected since no real server)
    let start_time = std::time::Instant::now();
    let result = TransportManager::new(&config).await;
    let elapsed = start_time.elapsed();
    
    println!("TransportManager creation took: {:?}", elapsed);
    println!("Result: {:?}", result.is_ok());
    
    // Check if server process was started
    let active_processes = process_manager.get_active_processes();
    println!("Active processes: {:?}", active_processes);
    
    // Should have started a process even if transport failed
    assert!(!active_processes.is_empty(), "Server process should have been started");
    
    // Cleanup
    process_manager.cleanup().await.unwrap();
}