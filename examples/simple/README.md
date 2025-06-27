# Simple GPMCP Layer Client Example

A basic client example demonstrating how to use the GPMCP Layer library to interact with Model Context Protocol (MCP) servers. This example shows the fundamental patterns for configuring, connecting to, and communicating with MCP servers using the GPMCP Layer abstraction.

## Purpose

This example serves as an introduction to the GPMCP Layer library, demonstrating:

- How to configure a `RunnerConfig` for connecting to MCP servers
- Setting up transport mechanisms (stdio in this case)
- Discovering available tools from a connected server
- Basic error handling patterns

## What It Does

The simple example:

1. **Configures a connection** to the counter server using `RunnerConfig`
2. **Creates a GpmcpLayer instance** with the specified configuration
3. **Lists available tools** from the connected server
4. **Prints tool names** to demonstrate successful communication


## How to Run

### Prerequisites

- Rust toolchain installed
- The counter server example available (located at `../servers/counter/`)

### Running the Example

```bash
cd examples/simple
cargo run
```

### Expected Output

When run successfully, you should see output similar to:

```
increment
decrement
get_value
```

These are the tool names provided by the counter server.


## Dependencies

The example uses these key dependencies:

- **`gpmcp-layer`**: Main GPMCP Layer library
- **`tokio`**: Async runtime
- **`anyhow`**: Error handling
- **`tracing-subscriber`**: Logging (optional)
