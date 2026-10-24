use crate::error::ServerError;

/// Placeholder gRPC server for the Worktree daemon API.
///
/// In the future this will host a full gRPC service definition (generated from
/// `.proto` files) that exposes tree, snapshot, branch, sync, and
/// administration RPCs. For now it provides the structural skeleton that the
/// rest of the server can reference.
pub struct GrpcServer {
    /// The address the server will bind to (e.g. `"127.0.0.1:9876"`).
    addr: String,

    /// Whether the server is currently listening.
    running: bool,
}

impl GrpcServer {
    /// Create a new `GrpcServer` that will listen on the given address.
    pub fn new(addr: impl Into<String>) -> Self {
        Self {
            addr: addr.into(),
            running: false,
        }
    }

    /// Return the address this server is configured to bind to.
    pub fn addr(&self) -> &str {
        &self.addr
    }

    /// Return whether the server is currently running.
    pub fn is_running(&self) -> bool {
        self.running
    }

    /// Start the gRPC server, binding to the configured address and serving
    /// incoming requests.
    ///
    /// This method will run until the server is shut down or an unrecoverable
    /// error occurs.
    ///
    /// # Arguments
    ///
    /// * `addr` — Override the bind address. Pass the same value as
    ///   [`Self::addr`] to use the default, or a different address to rebind.
    ///
    /// # Errors
    ///
    /// Returns [`ServerError::Api`] if the server fails to bind or encounters
    /// a fatal runtime error.
    pub async fn start(&mut self, addr: &str) -> Result<(), ServerError> {
        tracing::info!(addr = %addr, "Starting gRPC server");
        self.addr = addr.to_string();
        self.running = true;

        todo!("bind gRPC service to addr and serve incoming RPCs (tonic or similar)")
    }

    /// Gracefully shut down the gRPC server, finishing in-flight requests
    /// before stopping.
    ///
    /// # Errors
    ///
    /// Returns [`ServerError::Api`] if the shutdown process fails.
    pub async fn stop(&mut self) -> Result<(), ServerError> {
        if !self.running {
            return Err(ServerError::Api("gRPC server is not running".into()));
        }

        tracing::info!(addr = %self.addr, "Stopping gRPC server");
        self.running = false;

        todo!("signal the gRPC runtime to perform graceful shutdown")
    }
}

impl Default for GrpcServer {
    fn default() -> Self {
        Self::new("127.0.0.1:9876")
    }
}
