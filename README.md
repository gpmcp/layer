# GPMCP Layer

A robust, cross-platform Rust library for managing Model Context Protocol (MCP) server processes with advanced retry
logic, health monitoring, and platform-specific optimizations.

## Overview 

GPMCP Layer provides a high-level abstraction for spawning, managing, and communicating with MCP servers. It handles
process lifecycle management, automatic reconnection with configurable retry strategies, and platform-specific
optimizations for both Unix and Windows systems.

## Features

- **Cross-Platform Support**: Native implementations for Unix (Linux, macOS) and Windows
- **Robust Process Management**: Advanced process lifecycle management with graceful termination
- **Configurable Retry Logic**: Exponential backoff, jitter, and customizable retry strategies
- **Health Monitoring**: Built-in health checks and automatic recovery
- **Transport Flexibility**: Support for stdio and SSE (Server-Sent Events) transports
- **Resource Management**: Automatic cleanup and resource management
- **Async/Await Support**: Fully asynchronous API built on Tokio

## Architecture

The project is organized as a Rust workspace with the following crates:

```
├── gpmcp-layer-core/     # Platform-independent traits and configurations
├── gpmcp-layer/          # Main library with high-level API
├── gpmcp-layer-unix/     # Unix-specific process management
└── gpmcp-layer-windows/  # Windows-specific process management
```

## Quick Start

Add this to your `Cargo.toml`:

```toml
[dependencies]
gpmcp-layer = { git = "https://github.com/gpmcp/layer/" }
tokio = { version = "1.0", features = ["full"] }
```

### Basic Usage

```rust
use gpmcp_layer::{GpmcpLayer, RunnerConfig, Transport};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Configure the MCP server
    let config = RunnerConfig::builder()
        .name("my-mcp-server")
        .version("1.0.0")
        .command("python3")
        .args(["server.py", "--port", "8080"])
        .transport(Transport::Sse {
            url: "http://localhost:8080/sse".to_string(),
        })
        .build()?;

    // Create and start the layer
    let layer = GpmcpLayer::new(config)?;

    // List available tools
    let tools = layer.list_tools().await?;
    println!("Available tools: {:?}", tools);

    // Call a tool
    let request = CallToolRequestParam {
        name: "example_tool".to_string(),
        arguments: serde_json::json!({"input": "test"}),
    };
    let result = layer.call_tool(request).await?;
    println!("Tool result: {:?}", result);

    // Cleanup
    layer.cancel().await?;
    Ok(())
}
```


## Examples

Coming soon! Will be available in `examples/` directory for practical use cases and advanced configurations.

## API Reference

### Core Methods

- `list_tools()` - Get available tools from the MCP server
- `call_tool(request)` - Execute a tool with parameters
- `list_prompts()` - Get available prompts
- `get_prompt(request)` - Retrieve a specific prompt
- `list_resources()` - Get available resources
- `read_resource(request)` - Read a specific resource
- `is_healthy()` - Check server health with retries
- `is_healthy_quick()` - Quick health check without retries
- `peer_info()` - Get server information
- `cancel()` - Gracefully shutdown the layer

## Building

### Standard Build

```bash
cargo build --workspace
```

### Platform-Specific Builds

For CI environments or when you only need specific platform support:

```bash
# Unix only (Linux, macOS)
cargo build --workspace --exclude gpmcp-layer-windows

# Windows only
cargo build --workspace --exclude gpmcp-layer-unix
```

## Testing

```bash
# Run all tests
cargo test --workspace

# Platform-specific testing
cargo test --workspace --exclude gpmcp-layer-windows  # Unix
cargo test --workspace --exclude gpmcp-layer-unix     # Windows

# Integration tests
cargo test --test integration_tests
```

## Platform Support

| Platform | Status | Process Manager | Notes                          |
|----------|--------|-----------------|--------------------------------|
| Linux    | ✅ Full | Unix            | Native process groups, signals |
| macOS    | ✅ Full | Unix            | Native process groups, signals |
| Windows  | ✅ Full | Windows         | Job objects, process trees     |

## Error Handling

The library uses `anyhow::Result` for error handling and provides detailed error context:

```rust
async fn call(layer: &GpmcpLayer, request: CallToolRequest) -> anyhow::Result<()> {
    match layer.call_tool(request).await {
        Ok(result) => println!("Success: {:?}", result),
        Err(e) => {
            eprintln!("Error: {}", e);
            // Error chain provides detailed context
            for cause in e.chain() {
                eprintln!("  Caused by: {}", cause);
            }
        }
    }
}
```

## Logging

Enable logging to see detailed operation information:

```rust
use tracing_subscriber;
fn main() {
    tracing_subscriber::fmt()
        .with_env_filter("gpmcp_layer=debug")
        .init();
}
```

## Performance Considerations

- Process spawning is optimized for each platform
- Retry strategies use exponential backoff to avoid overwhelming servers
- Health checks are lightweight and non-blocking
- Resource cleanup is automatic and thorough

## Security

- Process isolation using platform-specific mechanisms
- Secure environment variable handling
- Proper cleanup of sensitive data
- No hardcoded credentials or secrets
