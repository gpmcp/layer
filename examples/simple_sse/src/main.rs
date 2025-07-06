use gpmcp_layer::GpmcpCrossLayer;
use gpmcp_layer::config::{RunnerConfig, Transport};
use std::path::PathBuf;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    const PORT: u16 = 8000;
    // Build the RunnerConfig with the desired settings.
    let config = RunnerConfig::builder()
        .name("test-mcp-server") // Name of the tool
        .version("0.0.0") // Version string
        .command("cargo") // Command to run
        .args(["run"]) // Arguments for the command
        .env("TRANSPORT", "sse") // Set TRANSPORT environment variable
        .env("PORT", PORT) // Set PORT for SSE
        .transport(Transport::Sse {
            url: format!("http://0.0.0.0:{PORT}/sse"),
        })
        .working_directory(
            // Set the working directory to the test-mcp-server example server
            PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("..")
                .join("test-mcp-server"),
        )
        .build()?;

    // Create a new GpmcpLayer instance with the config.
    let layer = GpmcpCrossLayer::new(config).connect().await?;

    // List available tools.
    let tools = layer.list_tools().await?;

    // Print the name of each tool.
    tools.tools.into_iter().for_each(|tool| {
        println!("{}", tool.name);
    });

    Ok(())
}
