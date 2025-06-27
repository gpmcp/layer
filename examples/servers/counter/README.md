# Counter MCP Server

A stateful Model Context Protocol (MCP) server implementation that provides counter functionality with increment,
decrement, and value retrieval operations. This example demonstrates how to build a complete MCP server using the RMCP
library with shared mutable state.

## Features

- **Stateful Operations**: Maintains a counter value that persists across tool calls
- **Multiple Tools**: Provides increment, decrement, and get_value operations
- **Resource Management**: Demonstrates resource listing and reading capabilities
- **Prompt Handling**: Shows how to implement prompt templates with arguments
- **Transport Abstraction**: Supports different transport mechanisms (currently stdio)
- **Thread-Safe State**: Uses `Arc<Mutex<T>>` for safe concurrent access to shared state

## How to Run

### Prerequisites

Ensure you have Rust installed and the project dependencies are available.

### Running the Server

The server uses the `TRANSPORT` environment variable to determine which transport mechanism to use:

**Standard I/O Transport (recommended):**

```bash
TRANSPORT=stdio cargo run
```

**Without transport specified:**

```bash
cargo run
```

*Note: This will exit with an error message asking you to set the TRANSPORT environment variable.*

## Available Tools

### `increment`

### `decrement`

### `get_value`

## Available Resources

### `str:////Users/to/some/path/`

### `memo://insights`

## Available Prompts

### `example_prompt`

- **Description**: A sample prompt that takes a message argument
- **Arguments**:
    - `message` (required): A message to include in the prompt
- **Returns**: A formatted prompt message containing the provided text

## Implementation Details

### Architecture

The server is built using several key components:

- **`Counter` struct**: Main server implementation with shared state
- **Tool Router**: Automatically generated routing for tool methods using `#[tool_router]` macro
- **Server Handler**: Implements the full MCP server protocol
- **Transport Layer**: Abstracts communication mechanism (stdio, SSE, etc.)

## Dependencies

This example uses the following key dependencies:

- **`rmcp`**: Rust MCP implementation with server capabilities
- **`tokio`**: Async runtime for handling concurrent requests
- **`serde_json`**: JSON serialization for MCP protocol messages
- **`anyhow`**: Error handling and propagation
- **`axum`**: Web framework (for HTTP-based transports)
- **`tracing`**: Structured logging and observability
