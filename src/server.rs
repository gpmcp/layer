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
use std::convert::Infallible;
use std::net::SocketAddr;
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context as TaskContext, Poll};
use tokio::net::TcpListener;
use tokio::process::{Child, Command};
use tokio::sync::RwLock;
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
            Runner::Stdio { command_runner: _ } => {
                todo!("Stdio runner not implemented yet")
            }
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

        tracing::info!(
            "Started command process: {} with args: {:?}",
            command_runner.command,
            command_runner.args
        );

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
