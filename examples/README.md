# GPMCP Layer Examples

This directory contains examples demonstrating how to use the GPMCP Layer library for building
MCP applications in Rust. These examples showcase different patterns and use cases for both client and server
implementations.

## Project Structure

```
examples/
├── simple_stdio/               # Basic GPMCP Layer client example using stdio
├── simple_sse/                 # Basic GPMCP Layer client example using SSE
└── test-mcp-server             # Test MCP server implementation
```

### Server Examples (`test-mcp-server/`)

A stateful MCP server that provides counter functionality with increment, decrement, and value retrieval operations.
For more details, refer to the [test-mc-server README](test-mcp-server/README.md).

### Simple Example with stdio (`simple_stdio/`)

A basic client example that demonstrates how to use the GPMCP Layer with StdIO to interact with MCP servers.
For more details on the simple client example, refer to the [Simple Client README](simple_stdio/README.md).

### Simple Example with SSE (`simple_sse/`)

A basic client example that demonstrates how to use the GPMCP Layer with SSE to interact with MCP servers.
For more details on the simple client example, refer to the [Simple Client README](simple_sse/README.md).
