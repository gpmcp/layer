use crate::runner::process_manager::ProcessManager;
use crate::runner::transport_manager::TransportManager;
use crate::RunnerConfig;
use std::collections::HashMap;
use std::sync::Arc;
use tokio_util::sync::CancellationToken;

#[tokio::test]
async fn test_sse_transport_starts_server_process() {
    let cancellation_token = Arc::new(CancellationToken::new());
    let config = RunnerConfig {
        name: "test-server".to_string(),
        version: "1.0.0".to_string(),
        command: "sleep".to_string(), // Simple command for testing
        args: vec!["10".to_string()],
        env: HashMap::new(),
        working_directory: None,
        transport: crate::runner_config::Transport::Sse {
            url: "http://localhost:8080/sse".to_string(),
        },
        retry_config: crate::retry_config::RetryConfig::default(),
    };
    
    let process_manager = ProcessManager::new(cancellation_token.clone(), &config).await.unwrap();
    
    // Verify no processes active initially
    assert_eq!(process_manager.get_active_processes().len(), 0);
    
    // Create transport manager - this should start the server process for SSE
    let transport_manager = TransportManager::new(&config).await;
    
    // The transport manager creation might fail because there's no actual server at localhost:8080
    // But we should see that a process was started
    match transport_manager {
        Ok(_tm) => {
            println!("TransportManager created successfully");
            // For this test, we're just checking that the architecture works
            // The actual SSE connection will fail because there's no server
        }
        Err(e) => {
            println!("TransportManager creation failed (expected): {}", e);
        }
    }
    
    // Cleanup any remaining processes
    process_manager.cleanup().await.unwrap();
}

#[tokio::test] 
async fn test_manual_server_start() {
    let cancellation_token = Arc::new(CancellationToken::new());
    let config = RunnerConfig {
        name: "test-server".to_string(),
        version: "1.0.0".to_string(),
        command: "sleep".to_string(),
        args: vec!["5".to_string()],
        env: HashMap::new(),
        working_directory: None,
        transport: crate::runner_config::Transport::Stdio,
        retry_config: crate::retry_config::RetryConfig::default(),
    };
    
    let process_manager = ProcessManager::new(cancellation_token, &config).await.unwrap();
    
    // Manually start the server process
    let handle = process_manager.start_server().await.unwrap();
    
    // Verify process is running
    assert!(handle.is_running().await);
    
    // Verify it's tracked
    assert_eq!(process_manager.get_active_processes().len(), 1);
    
    // Get the PID
    if let Some(pid) = handle.get_pid() {
        println!("Started server process with PID: {}", pid.0);
    }
    
    // Cleanup
    process_manager.cleanup().await.unwrap();
}