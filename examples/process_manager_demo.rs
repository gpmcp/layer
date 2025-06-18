use layer::runner::ProcessManager;
use layer::{RunnerConfig, Transport};
use std::collections::HashMap;
use std::sync::Arc;
use tokio_util::sync::CancellationToken;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Set up logging
    tracing_subscriber::fmt::init();
    
    // Create a simple server configuration
    let config = RunnerConfig {
        name: "example-server".to_string(),
        version: "1.0.0".to_string(),
        command: "python3".to_string(),
        args: vec!["-c".to_string(), "import time; print('Server started!'); time.sleep(30)".to_string()],
        env: HashMap::new(),
        working_directory: None,
        transport: Transport::Stdio,
        retry_config: layer::RetryConfig::default(),
    };
    
    println!("Creating ProcessManager...");
    let cancellation_token = Arc::new(CancellationToken::new());
    let process_manager = ProcessManager::new(cancellation_token.clone(), &config).await?;
    
    println!("Starting server process...");
    let handle = process_manager.start_server().await?;
    
    if let Some(pid) = handle.get_pid() {
        println!("✅ Server process started successfully with PID: {}", pid.0);
        println!("📊 Active processes: {:?}", process_manager.get_active_processes());
        
        // Wait a bit to let the server run
        println!("⏳ Server running for 5 seconds...");
        tokio::time::sleep(tokio::time::Duration::from_secs(5)).await;
        
        // Check if still running
        if handle.is_running().await {
            println!("✅ Server process is still running");
        } else {
            println!("❌ Server process stopped");
        }
    }
    
    println!("🧹 Cleaning up...");
    process_manager.cleanup().await?;
    println!("✅ Cleanup completed");
    
    Ok(())
}