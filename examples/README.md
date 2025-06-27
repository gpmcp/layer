# GPMCP Layer Examples

This directory contains examples demonstrating how to use the GPMCP Layer library for building
MCP applications in Rust. These examples showcase different patterns and use cases for both client and server
implementations.

## Project Structure

```
examples/
├── simple/                     # Basic GPMCP Layer client example
└── servers/                    # MCP server implementations
    └── counter/                # Counter server example
```

### Server Examples (`servers/`)

Server implementations demonstrating different MCP server patterns.

#### Counter Server (`servers/counter/`)

A stateful MCP server that provides counter functionality with increment, decrement, and value retrieval operations.
For more details, refer to the [Counter Server README](servers/counter/README.md).

### Simple Example (`simple/`)

A basic client example that demonstrates how to use the GPMCP Layer to interact with MCP servers.
For more details on the simple client example, refer to the [Simple Client README](simple/README.md).
