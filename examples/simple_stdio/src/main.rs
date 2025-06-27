use gpmcp_layer::{GpmcpLayer, RunnerConfig};
use std::path::PathBuf;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Build the RunnerConfig with the desired settings.
    let config = RunnerConfig::builder()
        .name("test-mcp-server") // Name of the tool
        .version("0.0.0") // Version string
        .command("cargo") // Command to run
        .args(["run"]) // Arguments for the command
        .env("TRANSPORT", "stdio") // Set environment variable
        .working_directory(
            // Set the working directory to the test-mcp-server example server
            PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("..")
                .join("test-mcp-server"),
        )
        .build()?;

    // Create a new GpmcpLayer instance with the config.
    let layer = GpmcpLayer::new(config)?;

    // List available tools.
    let tools = layer.list_tools().await?;

    // Print the name of each tool.
    tools.tools.into_iter().for_each(|tool| {
        println!("{}", tool.name);
    });

    Ok(())
}
