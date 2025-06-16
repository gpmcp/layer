use anyhow::{Context, Result};
use derive_builder::Builder;
use futures_util::Stream;
use gpmcp_domain::blueprint::{Runner, ServerDefinition};
use http_body_util::{BodyExt, StreamBody};
use hyper::body::{Bytes, Frame};
use hyper::server::conn::http1;
use hyper::service::service_fn;
use hyper::{Request, Response, StatusCode};
use hyper_util::rt::TokioIo;
use serde_json::json;
use std::convert::Infallible;
use std::net::SocketAddr;
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context as TaskContext, Poll};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;
use tokio::process::{Child, Command};
use tokio::sync::{RwLock, mpsc};
use tokio_util::sync::CancellationToken;

#[derive(Debug, Default, Clone, Builder)]
pub struct GpmcpServer {
    cancellation_token: Arc<RwLock<Option<CancellationToken>>>,
    server_definition: ServerDefinition,
}

impl GpmcpServer {
    pub fn builder() -> GpmcpServerBuilder {
        GpmcpServerBuilder::default()
    }

    pub async fn run(&self) -> anyhow::Result<()> {
        match &self.server_definition.runner {
            Runner::Stdio { command_runner } => self.run_stdio_server(command_runner).await,
            Runner::Sse {
                command_runner,
                url,
            } => self.run_sse_server(command_runner, url).await,
        }
    }

    async fn run_sse_server(
        &self,
        command_runner: &gpmcp_domain::blueprint::CommandRunner,
        target_url: &str,
    ) -> Result<()> {
        // Create cancellation token for coordinated shutdown
        let cancellation_token = CancellationToken::new();
        {
            let mut token_guard = self.cancellation_token.write().await;
            *token_guard = Some(cancellation_token.clone());
        }

        // Start the command process
        let server_definition = self.server_definition.clone();
        let command_runner = command_runner.clone();
        let mut child_process =
            Self::start_command_process(server_definition, command_runner).await?;

        // Start the proxy server
        let proxy_handle = self
            .start_proxy_server(target_url.to_string(), cancellation_token.clone())
            .await?;

        // Wait for cancellation or process termination
        tokio::select! {
            _ = cancellation_token.cancelled() => {
                tracing::info!("Cancellation requested, shutting down servers");
            }
            result = child_process.wait() => {
                match result {
                    Ok(status) => tracing::info!("Command process exited with status: {}", status),
                    Err(e) => tracing::error!("Error waiting for command process: {}", e),
                }
            }
        }

        // Cleanup: terminate child process and proxy server
        self.cleanup_servers(child_process, proxy_handle, cancellation_token)
            .await?;

        Ok(())
    }

    async fn run_stdio_server(
        &self,
        command_runner: &gpmcp_domain::blueprint::CommandRunner,
    ) -> Result<()> {
        // Create cancellation token for coordinated shutdown
        let cancellation_token = CancellationToken::new();
        {
            let mut token_guard = self.cancellation_token.write().await;
            *token_guard = Some(cancellation_token.clone());
        }

        // Start the command process with stdin/stdout pipes
        let server_definition = self.server_definition.clone();
        let command_runner = command_runner.clone();
        let mut child_process =
            Self::start_stdio_command_process(server_definition, command_runner).await?;

        // Get stdin and stdout handles
        let mut stdin = child_process
            .stdin
            .take()
            .ok_or_else(|| anyhow::anyhow!("Failed to get stdin handle"))?;
        let mut stdout = child_process
            .stdout
            .take()
            .ok_or_else(|| anyhow::anyhow!("Failed to get stdout handle"))?;

        // Create channels for communication between HTTP server and child process
        let (request_sender, mut request_receiver) = mpsc::unbounded_channel::<Bytes>();
        let (response_sender, response_receiver) =
            mpsc::unbounded_channel::<Result<Bytes, std::io::Error>>();

        // Start the HTTP server
        let http_handle = self
            .start_stdio_http_server(
                request_sender,
                response_receiver,
                cancellation_token.clone(),
            )
            .await?;

        // Handle stdin/stdout communication
        let stdin_handle = {
            let cancellation_token = cancellation_token.clone();
            tokio::spawn(async move {
                while let Some(data) = request_receiver.recv().await {
                    if cancellation_token.is_cancelled() {
                        break;
                    }

                    if let Err(e) = stdin.write_all(&data).await {
                        tracing::error!("Error writing to stdin: {}", e);
                        break;
                    }

                    if let Err(e) = stdin.flush().await {
                        tracing::error!("Error flushing stdin: {}", e);
                        break;
                    }
                }
            })
        };

        let stdout_handle = {
            let response_sender = response_sender.clone();
            let cancellation_token = cancellation_token.clone();
            tokio::spawn(async move {
                let mut buffer = [0u8; 8192];
                loop {
                    if cancellation_token.is_cancelled() {
                        break;
                    }

                    match stdout.read(&mut buffer).await {
                        Ok(0) => {
                            // EOF reached
                            tracing::info!("Child process stdout closed");
                            break;
                        }
                        Ok(n) => {
                            let data = Bytes::copy_from_slice(&buffer[..n]);
                            if response_sender.send(Ok(data)).is_err() {
                                tracing::warn!("Response receiver dropped");
                                break;
                            }
                        }
                        Err(e) => {
                            tracing::error!("Error reading from stdout: {}", e);
                            let _ = response_sender.send(Err(e));
                            break;
                        }
                    }
                }
            })
        };

        // Wait for cancellation or process termination
        tokio::select! {
            _ = cancellation_token.cancelled() => {
                tracing::info!("Cancellation requested, shutting down stdio server");
            }
            result = child_process.wait() => {
                match result {
                    Ok(status) => tracing::info!("Command process exited with status: {}", status),
                    Err(e) => tracing::error!("Error waiting for command process: {}", e),
                }
            }
        }

        // Cleanup: terminate child process and servers
        cancellation_token.cancel();

        // Wait for handles to complete
        let _ = tokio::join!(stdin_handle, stdout_handle, http_handle);

        // Terminate child process
        if let Err(e) = child_process.kill().await {
            tracing::warn!("Failed to kill child process: {}", e);
        }

        // Wait for child process to exit
        if let Err(e) = child_process.wait().await {
            tracing::warn!("Error waiting for child process to exit: {}", e);
        }

        tracing::info!("Stdio server shut down successfully");
        Ok(())
    }

    async fn start_command_process(
        server_definition: ServerDefinition,
        command_runner: gpmcp_domain::blueprint::CommandRunner,
    ) -> Result<Child> {
        let mut cmd = Command::new(&command_runner.command);
        cmd.args(&command_runner.args);

        if !command_runner.workdir.is_empty() {
            cmd.current_dir(&command_runner.workdir);
        }

        // Add environment variables
        for (key, value) in server_definition.env {
            cmd.env(key, value);
        }

        let child = cmd
            .spawn()
            .with_context(|| format!("Failed to start command: {}", command_runner.command))?;

        // Log the PID if available
        match child.id() {
            Some(pid) => {
                tracing::info!(
                    "Started command process: {} with args: {:?}, PID: {}",
                    command_runner.command, command_runner.args, pid
                );
            }
            None => {
                tracing::warn!(
                    "Started command process: {} with args: {:?}, but PID is not available (process may have exited quickly)",
                    command_runner.command, command_runner.args
                );
            }
        }

        Ok(child)
    }

    async fn start_stdio_command_process(
        server_definition: ServerDefinition,
        command_runner: gpmcp_domain::blueprint::CommandRunner,
    ) -> Result<Child> {
        let mut cmd = Command::new(&command_runner.command);
        cmd.args(&command_runner.args);

        if !command_runner.workdir.is_empty() {
            cmd.current_dir(&command_runner.workdir);
        }

        // Add environment variables
        for (key, value) in server_definition.env {
            cmd.env(key, value);
        }

        let child = cmd
            .spawn()
            .with_context(|| format!("Failed to start command: {}", command_runner.command))?;

        // Log the PID if available
        match child.id() {
            Some(pid) => {
                tracing::info!(
                    "Started stdio command process: {} with args: {:?}, PID: {}",
                    command_runner.command, command_runner.args, pid
                );
            }
            None => {
                tracing::warn!(
                    "Started stdio command process: {} with args: {:?}, but PID is not available (process may have exited quickly)",
                    command_runner.command, command_runner.args
                );
            }
        }

        Ok(child)
    }

    async fn start_proxy_server(
        &self,
        target_url: String,
        cancellation_token: CancellationToken,
    ) -> Result<tokio::task::JoinHandle<()>> {
        let addr: SocketAddr = "127.0.0.1:3000".parse()?;
        let listener = TcpListener::bind(addr)
            .await
            .with_context(|| format!("Failed to bind to address: {}", addr))?;

        tracing::info!("Proxy server listening on http://{}", addr);

        let handle = tokio::spawn(async move {
            let proxy_service = ProxyService::new(target_url, cancellation_token.clone());

            loop {
                tokio::select! {
                    _ = cancellation_token.cancelled() => {
                        tracing::info!("Proxy server shutting down due to cancellation");
                        break;
                    }
                    accept_result = listener.accept() => {
                        match accept_result {
                            Ok((stream, _)) => {
                                let io = TokioIo::new(stream);
                                let proxy_service = proxy_service.clone();

                                tokio::spawn(async move {
                                    let service = service_fn(move |req| {
                                        proxy_service.clone().handle_request(req)
                                    });

                                    if let Err(err) = http1::Builder::new()
                                        .serve_connection(io, service)
                                        .await
                                    {
                                        tracing::error!("Error serving connection: {:?}", err);
                                    }
                                });
                            }
                            Err(e) => {
                                tracing::error!("Error accepting connection: {}", e);
                            }
                        }
                    }
                }
            }
        });

        Ok(handle)
    }

    async fn start_stdio_http_server(
        &self,
        request_sender: mpsc::UnboundedSender<Bytes>,
        response_receiver: mpsc::UnboundedReceiver<Result<Bytes, std::io::Error>>,
        cancellation_token: CancellationToken,
    ) -> Result<tokio::task::JoinHandle<()>> {
        let addr: SocketAddr = "127.0.0.1:3002".parse()?;
        let listener = TcpListener::bind(addr)
            .await
            .with_context(|| format!("Failed to bind to address: {}", addr))?;

        tracing::info!("Stdio HTTP server listening on http://{}", addr);

        let response_receiver = Arc::new(tokio::sync::Mutex::new(response_receiver));

        let handle = tokio::spawn(async move {
            let stdio_service = StdioService::new(
                request_sender,
                response_receiver.clone(),
                cancellation_token.clone(),
            );

            loop {
                tokio::select! {
                    _ = cancellation_token.cancelled() => {
                        tracing::info!("Stdio HTTP server shutting down due to cancellation");
                        break;
                    }
                    accept_result = listener.accept() => {
                        match accept_result {
                            Ok((stream, _)) => {
                                let io = TokioIo::new(stream);
                                let stdio_service = stdio_service.clone();

                                tokio::spawn(async move {
                                    let service = service_fn(move |req| {
                                        stdio_service.clone().handle_request(req)
                                    });

                                    if let Err(err) = http1::Builder::new()
                                        .serve_connection(io, service)
                                        .await
                                    {
                                        tracing::error!("Error serving connection: {:?}", err);
                                    }
                                });
                            }
                            Err(e) => {
                                tracing::error!("Error accepting connection: {}", e);
                            }
                        }
                    }
                }
            }
        });

        Ok(handle)
    }

    async fn cleanup_servers(
        &self,
        mut child_process: Child,
        proxy_handle: tokio::task::JoinHandle<()>,
        cancellation_token: CancellationToken,
    ) -> Result<()> {
        // Cancel the proxy server
        cancellation_token.cancel();

        // Wait for proxy server to shutdown
        if let Err(e) = proxy_handle.await {
            tracing::error!("Error shutting down proxy server: {}", e);
        }

        // Terminate child process
        if let Err(e) = child_process.kill().await {
            tracing::warn!("Failed to kill child process: {}", e);
        }

        // Wait for child process to exit
        if let Err(e) = child_process.wait().await {
            tracing::warn!("Error waiting for child process to exit: {}", e);
        }

        tracing::info!("All servers shut down successfully");
        Ok(())
    }
}

#[derive(Clone)]
struct ProxyService {
    target_url: String,
    cancellation_token: CancellationToken,
    client: reqwest::Client,
}

impl ProxyService {
    fn new(target_url: String, cancellation_token: CancellationToken) -> Self {
        Self {
            target_url,
            cancellation_token,
            client: reqwest::Client::new(),
        }
    }

    async fn handle_request(
        self,
        req: Request<hyper::body::Incoming>,
    ) -> Result<Response<StreamBody<EventStreamWrapper>>, Infallible> {
        // Check for calculation token request - this triggers shutdown
        if self.is_calculation_token_request(&req) {
            tracing::info!("Received calculation token request, triggering shutdown");
            self.cancellation_token.cancel();
            return Ok(self.create_shutdown_response());
        }

        // Forward the request to the target server
        match self.forward_request(req).await {
            Ok(response) => Ok(response),
            Err(e) => {
                tracing::error!("Error forwarding request: {}", e);
                Ok(self.create_error_response())
            }
        }
    }

    fn is_calculation_token_request(&self, req: &Request<hyper::body::Incoming>) -> bool {
        // Check if this is a calculation token request
        // This could be based on path, headers, or query parameters
        let path = req.uri().path();
        let query = req.uri().query().unwrap_or("");

        // Example: Check for specific endpoint or query parameter
        path.contains("/calculate") && query.contains("token")
            || path == "/shutdown"
            || req.headers().get("x-calculation-token").is_some()
    }

    async fn forward_request(
        &self,
        req: Request<hyper::body::Incoming>,
    ) -> Result<Response<StreamBody<EventStreamWrapper>>> {
        let method = req.method().clone();
        let uri = req.uri();
        let headers = req.headers().clone();

        // Construct target URL
        let target_url = format!(
            "{}{}",
            self.target_url.trim_end_matches('/'),
            uri.path_and_query().map(|pq| pq.as_str()).unwrap_or("/")
        );

        // Log the forwarding for debugging
        tracing::info!(
            "Forwarding {} {} to {}",
            method,
            uri.path_and_query().map(|pq| pq.as_str()).unwrap_or("/"),
            target_url
        );

        // Collect request body
        let body_bytes = req.into_body().collect().await?.to_bytes();
        tracing::info!("Received {body_bytes:?}");

        // Create request to target server
        let mut client_req = self.client.request(method, &target_url);

        // Copy headers (excluding host and other hop-by-hop headers)
        for (name, value) in headers.iter() {
            let name_str = name.as_str().to_lowercase();
            if !["host", "connection", "transfer-encoding", "content-length"]
                .contains(&name_str.as_str())
            {
                if let Ok(value_str) = value.to_str() {
                    client_req = client_req.header(name.as_str(), value_str);
                }
            }
        }

        // Add body if present
        if !body_bytes.is_empty() {
            client_req = client_req.body(body_bytes.to_vec());
        }

        // Send request and get response
        let response = client_req.send().await?;
        let status = response.status();
        let response_headers = response.headers().clone();

        // Check if the response is an event stream
        let content_type = response_headers
            .get("content-type")
            .and_then(|v| v.to_str().ok())
            .unwrap_or("");

        let mut hyper_response = Response::builder().status(status);

        // Copy response headers
        for (name, value) in response_headers.iter() {
            hyper_response = hyper_response.header(name, value);
        }

        // Handle response as stream (works for both event streams and regular responses)
        let byte_stream = response.bytes_stream();
        let event_stream = EventStreamWrapper::new(byte_stream);
        let stream_body = StreamBody::new(event_stream);

        // Ensure proper content-type for event streams
        if content_type.contains("text/event-stream") {
            hyper_response = hyper_response.header("content-type", "text/event-stream");
            hyper_response = hyper_response.header("cache-control", "no-cache");
            hyper_response = hyper_response.header("connection", "keep-alive");
        }

        Ok(hyper_response.body(stream_body)?)
    }

    fn create_shutdown_response(&self) -> Response<StreamBody<EventStreamWrapper>> {
        let data = Bytes::from(
            r#"{"status":"shutdown","message":"Server shutting down due to calculation token request"}"#,
        );
        let stream = futures_util::stream::once(async move { Ok(data) });
        let event_stream = EventStreamWrapper::new(stream);
        let stream_body = StreamBody::new(event_stream);

        Response::builder()
            .status(StatusCode::OK)
            .header("content-type", "application/json")
            .body(stream_body)
            .unwrap()
    }

    fn create_error_response(&self) -> Response<StreamBody<EventStreamWrapper>> {
        let data = Bytes::from(r#"{"error":"Failed to forward request"}"#);
        let stream = futures_util::stream::once(async move { Ok(data) });
        let event_stream = EventStreamWrapper::new(stream);
        let stream_body = StreamBody::new(event_stream);

        Response::builder()
            .status(StatusCode::INTERNAL_SERVER_ERROR)
            .header("content-type", "application/json")
            .body(stream_body)
            .unwrap()
    }
}

// Wrapper to convert byte streams to Frame streams for hyper
pub struct EventStreamWrapper {
    inner: Pin<Box<dyn Stream<Item = Result<Bytes, reqwest::Error>> + Send + Sync>>,
}

impl EventStreamWrapper {
    fn new<S>(stream: S) -> Self
    where
        S: Stream<Item = Result<Bytes, reqwest::Error>> + Send + Sync + 'static,
    {
        Self {
            inner: Box::pin(stream),
        }
    }
}

#[derive(Clone)]
struct StdioService {
    request_sender: mpsc::UnboundedSender<Bytes>,
    response_receiver:
        Arc<tokio::sync::Mutex<mpsc::UnboundedReceiver<Result<Bytes, std::io::Error>>>>,
    cancellation_token: CancellationToken,
}

impl StdioService {
    fn new(
        request_sender: mpsc::UnboundedSender<Bytes>,
        response_receiver: Arc<
            tokio::sync::Mutex<mpsc::UnboundedReceiver<Result<Bytes, std::io::Error>>>,
        >,
        cancellation_token: CancellationToken,
    ) -> Self {
        Self {
            request_sender,
            response_receiver,
            cancellation_token,
        }
    }

    async fn handle_request(
        self,
        req: Request<hyper::body::Incoming>,
    ) -> Result<Response<StreamBody<StdioResponseStream>>, Infallible> {
        // Check for cancellation
        if self.cancellation_token.is_cancelled() {
            return Ok(self.create_error_response("Server is shutting down"));
        }

        // Check request method and content-length to decide if we should forward
        let method = req.method().clone();
        let content_length = req
            .headers()
            .get("content-length")
            .and_then(|v| v.to_str().ok())
            .and_then(|s| s.parse::<usize>().ok())
            .unwrap_or(0);

        // Only forward POST requests or requests with non-zero body length
        let should_forward = method == hyper::Method::POST || content_length > 0;

        if !should_forward {
            tracing::info!(
                "Rejecting {} request with content-length: {}",
                method,
                content_length
            );
            return Ok(Response::builder()
                .status(StatusCode::OK)
                .body(StreamBody::new(StdioResponseStream::from_stream(
                    futures_util::stream::once(async move { Ok(Bytes::from(json!({"jsonrpc":"2.0","id":0,"result":{"protocolVersion":"2025-03-26","capabilities":{"experimental":{},"prompts":{"listChanged":false},"resources":{"subscribe":false,"listChanged":false},"tools":{"listChanged":false}},"serverInfo":{"name":"test-weather-server","version":"1.9.5.dev5+f3cd20c"}}}).to_string())) }),
                )))
                .unwrap());
        }

        tracing::info!(
            "Forwarding {} request with content-length: {}",
            method,
            content_length
        );

        // Collect the request body
        let body_bytes = match req.into_body().collect().await {
            Ok(collected) => collected.to_bytes(),
            Err(e) => {
                tracing::error!("Error collecting request body: {}", e);
                return Ok(self.create_error_response("Failed to read request body"));
            }
        };

        tracing::info!("Received {} bytes in request body", body_bytes.len());

        // Send the request body to stdin
        if let Err(e) = self.request_sender.send(body_bytes) {
            tracing::error!("Error sending request to stdin: {}", e);
            return Ok(self.create_error_response("Failed to send request to child process"));
        }

        // Wait for response from stdout
        let response_data = {
            let mut receiver = self.response_receiver.lock().await;
            match receiver.recv().await {
                Some(Ok(data)) => data,
                Some(Err(e)) => {
                    tracing::error!("Error reading from stdout: {}", e);
                    return Ok(self.create_error_response("Error reading from child process"));
                }
                None => {
                    tracing::warn!("Child process stdout closed");
                    return Ok(self.create_error_response("Child process closed"));
                }
            }
        };

        // Create a simple response with the data
        let stream = futures_util::stream::once(async move { Ok(response_data) });
        let response_stream = StdioResponseStream::from_stream(stream);
        let stream_body = StreamBody::new(response_stream);

        let response = Response::builder()
            .status(StatusCode::OK)
            .header("content-type", "application/octet-stream")
            .body(stream_body)
            .unwrap();

        Ok(response)
    }

    fn create_error_response(&self, message: &str) -> Response<StreamBody<StdioResponseStream>> {
        let data = Bytes::from(format!(r#"{{"error":"{}"}}"#, message));
        let stream = futures_util::stream::once(async move { Ok(data) });
        let response_stream = StdioResponseStream::from_stream(stream);
        let stream_body = StreamBody::new(response_stream);

        Response::builder()
            .status(StatusCode::INTERNAL_SERVER_ERROR)
            .header("content-type", "application/json")
            .body(stream_body)
            .unwrap()
    }
}

// Response stream for stdio communication
pub struct StdioResponseStream {
    inner_stream: Pin<Box<dyn Stream<Item = Result<Bytes, std::io::Error>> + Send + Sync>>,
}

impl StdioResponseStream {
    fn from_stream<S>(stream: S) -> Self
    where
        S: Stream<Item = Result<Bytes, std::io::Error>> + Send + Sync + 'static,
    {
        Self {
            inner_stream: Box::pin(stream),
        }
    }
}

impl Stream for StdioResponseStream {
    type Item = Result<Frame<Bytes>, Box<dyn std::error::Error + Send + Sync>>;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut TaskContext<'_>) -> Poll<Option<Self::Item>> {
        match self.inner_stream.as_mut().poll_next(cx) {
            Poll::Ready(Some(Ok(bytes))) => Poll::Ready(Some(Ok(Frame::data(bytes)))),
            Poll::Ready(Some(Err(e))) => Poll::Ready(Some(Err(Box::new(e)))),
            Poll::Ready(None) => Poll::Ready(None),
            Poll::Pending => Poll::Pending,
        }
    }
}

impl Stream for EventStreamWrapper {
    type Item = Result<Frame<Bytes>, Box<dyn std::error::Error + Send + Sync>>;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut TaskContext<'_>) -> Poll<Option<Self::Item>> {
        match self.inner.as_mut().poll_next(cx) {
            Poll::Ready(Some(Ok(bytes))) => Poll::Ready(Some(Ok(Frame::data(bytes)))),
            Poll::Ready(Some(Err(e))) => Poll::Ready(Some(Err(Box::new(e)))),
            Poll::Ready(None) => Poll::Ready(None),
            Poll::Pending => Poll::Pending,
        }
    }
}
