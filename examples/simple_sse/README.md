# Simple_sse GPMCP Layer Client Example

A basic client example demonstrating how to use the GPMCP Layer library to interact with MCP using SSE transport.

## Purpose

This example serves as an introduction to the GPMCP Layer library, demonstrating:

- How to configure a `RunnerConfig` for connecting to MCP servers
- Discovering available tools from a connected server
- Basic error handling patterns

## What It Does

The simple example:

1. **Configures a connection** to the counter server using `RunnerConfig`
2. **Creates a GpmcpLayer instance** with the specified configuration
3. **Lists available tools** from the connected server
4. **Prints tool names** to demonstrate successful communication
