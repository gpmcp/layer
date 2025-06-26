use gpmcp_layer::{GpmcpLayer, RunnerConfig};
use std::path::PathBuf;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let config = RunnerConfig::builder()
        .name("counter")
        .version("0.0.0")
        .command("cargo")
        .args(["run"])
        .env("TRANSPORT", "stdio")
        .working_directory(
            PathBuf::from(".")
                .join("examples")
                .join("servers")
                .join("counter")
                .canonicalize()
                .unwrap(),
        )
        .build()?;

    let layer = GpmcpLayer::new(config)?;
    let tools = layer.list_tools().await?;
    tools.tools.into_iter().for_each(|tool| {
        println!("{}", tool.name);
    });

    Ok(())
}
