//! Application state for the Worktree admin panel

use crate::{error::Result, AdminError, AdminConfig};
use std::sync::Arc;
use tokio::sync::RwLock;
use uuid::Uuid;

/// Shared application state for the admin panel
#[derive(Clone)]
pub struct AdminState {
    /// Admin panel configuration
    pub config: Arc<AdminConfig>,
    /// Server connection manager
    pub server_connection: Arc<RwLock<ServerConnection>>,
    /// Metrics collector
    pub metrics: Arc<RwLock<Metrics>>,
}

impl AdminState {
    /// Create a new admin state with the given configuration
    pub fn new(config: AdminConfig) -> Self {
        Self {
            config: Arc::new(config),
            server_connection: Arc::new(RwLock::new(ServerConnection::new())),
            metrics: Arc::new(RwLock::new(Metrics::new())),
        }
    }

    /// Get the server endpoint URL
    pub fn server_endpoint(&self) -> &str {
        &self.config.server_endpoint
    }

    /// Check if authentication is enabled
    pub fn auth_enabled(&self) -> bool {
        self.config.auth_enabled
    }

    /// Validate API key if authentication is enabled
    pub fn validate_api_key(&self, key: &str) -> Result<()> {
        if !self.auth_enabled() {
            return Ok(());
        }

        match &self.config.api_key {
            Some(valid_key) if valid_key == key => Ok(()),
            Some(_) => Err(AdminError::Authentication("Invalid API key".to_string())),
            None => Err(AdminError::Config(
                "API key not configured but auth is enabled".to_string(),
            )),
        }
    }

    /// Increment request counter
    pub async fn increment_requests(&self) {
        let mut metrics = self.metrics.write().await;
        metrics.total_requests += 1;
    }

    /// Increment error counter
    pub async fn increment_errors(&self) {
        let mut metrics = self.metrics.write().await;
        metrics.total_errors += 1;
    }

    /// Get current metrics snapshot
    pub async fn get_metrics(&self) -> Metrics {
        self.metrics.read().await.clone()
    }
}

/// Server connection state
#[derive(Debug, Clone)]
pub struct ServerConnection {
    /// Connection status
    pub connected: bool,
    /// Server instance ID (if connected)
    pub server_id: Option<Uuid>,
    /// Last successful connection timestamp
    pub last_connected: Option<chrono::DateTime<chrono::Utc>>,
    /// Number of connection attempts
    pub connection_attempts: u64,
    /// Number of failed connections
    pub failed_connections: u64,
}

impl ServerConnection {
    /// Create a new server connection state
    pub fn new() -> Self {
        Self {
            connected: false,
            server_id: None,
            last_connected: None,
            connection_attempts: 0,
            failed_connections: 0,
        }
    }

    /// Mark connection as successful
    pub fn mark_connected(&mut self, server_id: Uuid) {
        self.connected = true;
        self.server_id = Some(server_id);
        self.last_connected = Some(chrono::Utc::now());
        self.connection_attempts += 1;
    }

    /// Mark connection as failed
    pub fn mark_failed(&mut self) {
        self.connected = false;
        self.server_id = None;
        self.connection_attempts += 1;
        self.failed_connections += 1;
    }

    /// Mark connection as disconnected
    pub fn mark_disconnected(&mut self) {
        self.connected = false;
        self.server_id = None;
    }

    /// Get connection uptime in seconds
    pub fn uptime_seconds(&self) -> Option<u64> {
        self.last_connected.map(|connected_at| {
            let now = chrono::Utc::now();
            (now - connected_at).num_seconds() as u64
        })
    }
}

impl Default for ServerConnection {
    fn default() -> Self {
        Self::new()
    }
}

/// Metrics for the admin panel
#[derive(Debug, Clone)]
pub struct Metrics {
    /// Total number of requests received
    pub total_requests: u64,
    /// Total number of errors encountered
    pub total_errors: u64,
    /// Timestamp when metrics collection started
    pub started_at: chrono::DateTime<chrono::Utc>,
}

impl Metrics {
    /// Create a new metrics collector
    pub fn new() -> Self {
        Self {
            total_requests: 0,
            total_errors: 0,
            started_at: chrono::Utc::now(),
        }
    }

    /// Get uptime in seconds
    pub fn uptime_seconds(&self) -> u64 {
        let now = chrono::Utc::now();
        (now - self.started_at).num_seconds() as u64
    }

    /// Calculate error rate (errors / requests)
    pub fn error_rate(&self) -> f64 {
        if self.total_requests == 0 {
            0.0
        } else {
            self.total_errors as f64 / self.total_requests as f64
        }
    }
}

impl Default for Metrics {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_server_connection_new() {
        let conn = ServerConnection::new();
        assert!(!conn.connected);
        assert!(conn.server_id.is_none());
        assert_eq!(conn.connection_attempts, 0);
    }

    #[test]
    fn test_server_connection_mark_connected() {
        let mut conn = ServerConnection::new();
        let server_id = Uuid::new_v4();

        conn.mark_connected(server_id);

        assert!(conn.connected);
        assert_eq!(conn.server_id, Some(server_id));
        assert!(conn.last_connected.is_some());
        assert_eq!(conn.connection_attempts, 1);
    }

    #[test]
    fn test_server_connection_mark_failed() {
        let mut conn = ServerConnection::new();

        conn.mark_failed();

        assert!(!conn.connected);
        assert!(conn.server_id.is_none());
        assert_eq!(conn.connection_attempts, 1);
        assert_eq!(conn.failed_connections, 1);
    }

    #[test]
    fn test_metrics_new() {
        let metrics = Metrics::new();
        assert_eq!(metrics.total_requests, 0);
        assert_eq!(metrics.total_errors, 0);
        assert_eq!(metrics.error_rate(), 0.0);
    }

    #[test]
    fn test_metrics_error_rate() {
        let mut metrics = Metrics::new();
        metrics.total_requests = 100;
        metrics.total_errors = 5;

        assert_eq!(metrics.error_rate(), 0.05);
    }

    #[tokio::test]
    async fn test_admin_state_increment_requests() {
        let config = AdminConfig::default();
        let state = AdminState::new(config);

        state.increment_requests().await;
        state.increment_requests().await;

        let metrics = state.get_metrics().await;
        assert_eq!(metrics.total_requests, 2);
    }

    #[tokio::test]
    async fn test_admin_state_validate_api_key() {
        let mut config = AdminConfig::default();
        config.auth_enabled = true;
        config.api_key = Some("secret123".to_string());

        let state = AdminState::new(config);

        assert!(state.validate_api_key("secret123").is_ok());
        assert!(state.validate_api_key("wrong").is_err());
    }

    #[tokio::test]
    async fn test_admin_state_no_auth() {
        let config = AdminConfig::default(); // auth_enabled = false
        let state = AdminState::new(config);

        assert!(state.validate_api_key("anything").is_ok());
    }
}
