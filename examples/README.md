# GPMCP Layer Examples

This directory contains examples demonstrating how to use the GPMCP Layer library for building Model Context Protocol (
MCP) applications in Rust. These examples showcase different patterns and use cases for both client and server
implementations.

## Quick Start

### To run any example
 Navigate to its directory and use:

- For example: for the simple client example, you can run:

```bash
cd examples/simple
cargo run
```

This will:

1. Configure a connection to the counter server
2. List available tools
3. Print the tool names

### To run any server example
 Navigate to the server example directory and run: 

- For stdio transport:
```bash
TRANSPORT=stdio cargo run
```


## Example Structure

### Server Examples (`servers/`)

Server implementations demonstrating different MCP server patterns.

#### Counter Server (`servers/counter/`)

A stateful MCP server that provides counter functionality with increment, decrement, and value retrieval operations.
For more details, refer to the [Counter Server README](servers/counter/README.md).

### Simple Example (`simple/`)

A basic client example that demonstrates how to use the GPMCP Layer to interact with MCP servers.
For more details on the simple client example, refer to the [Simple Client README](simple/README.md).
