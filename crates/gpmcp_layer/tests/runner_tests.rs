use gpmcp_core::{RunnerConfig, Transport};
use gpmcp_layer::GpmcpLayer;

/// Test creating a stdio runner with basic echo command
#[tokio::test]
async fn test_stdio_runner_creation_with_echo() {
    let _ = tracing_subscriber::fmt()
        .with_file(true)
        .with_thread_ids(false)
        .with_target(false)
        .with_line_number(true)
        .try_init();

    let server_definition = RunnerConfig::builder()
        .name("test_stdio_runner")
        .version("0.1")
        .command("uv")
        .args(["run", "main.py"])
        .working_directory("/home/ssdd/PycharmProjects/mcp-image/")
        .transport(Transport::Sse {
            url: "http://localhost:8000/sse".to_string(),
        })
        .build()
        .unwrap();

    // Test that the runner can be created (even if it fails to work as MCP server)
    let result = GpmcpLayer::new(server_definition).unwrap();
    println!("{:?}", result.list_tools().await.unwrap());
    result.cancel().await.unwrap();
}
