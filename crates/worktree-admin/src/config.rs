//! Configuration for the Worktree Admin Panel UI

use serde::{Deserialize, Serialize};
use std::path::Path;
use thiserror::Error;

/// Configuration error types
#[derive(Debug, Error)]
pub enum ConfigError {
    /// IO error while reading config file
    #[error("Failed to read config file: {0}")]
    Io(#[from] std::io::Error),

    /// Failed to parse TOML config
    #[error("Failed to parse config: {0}")]
    Parse(#[from] toml::de::Error),

    /// Invalid configuration value
    #[error("Invalid configuration: {0}")]
    Invalid(String),
}

/// Admin panel configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AdminConfig {
    /// Server configuration
    #[serde(default)]
    pub server: ServerConfig,

    /// UI configuration
    #[serde(default)]
    pub ui: UiConfig,

    /// Security and authentication configuration
    #[serde(default)]
    pub security: SecurityConfig,

    /// Logging configuration
    #[serde(default)]
    pub logging: LoggingConfig,
}

impl Default for AdminConfig {
    fn default() -> Self {
        Self {
            server: ServerConfig::default(),
            ui: UiConfig::default(),
            security: SecurityConfig::default(),
            logging: LoggingConfig::default(),
        }
    }
}

impl AdminConfig {
    /// Load configuration from a TOML file
    pub fn from_file(path: impl AsRef<Path>) -> Result<Self, ConfigError> {
        let contents = std::fs::read_to_string(path)?;
        let config: AdminConfig = toml::from_str(&contents)?;
        config.validate()?;
        Ok(config)
    }

    /// Load configuration from a TOML string
    pub fn from_str(s: &str) -> Result<Self, ConfigError> {
        let config: AdminConfig = toml::from_str(s)?;
        config.validate()?;
        Ok(config)
    }

    /// Save configuration to a TOML file
    pub fn to_file(&self, path: impl AsRef<Path>) -> Result<(), ConfigError> {
        let contents = toml::to_string_pretty(self)
            .map_err(|e| ConfigError::Invalid(e.to_string()))?;
        std::fs::write(path, contents)?;
        Ok(())
    }

    /// Validate the configuration
    pub fn validate(&self) -> Result<(), ConfigError> {
        // Validate port number
        if self.server.port == 0 {
            return Err(ConfigError::Invalid("Port must be greater than 0".to_string()));
        }

        // Validate that if auth is enabled, we have an API key
        if self.security.auth_enabled && self.security.api_key.is_none() {
            return Err(ConfigError::Invalid(
                "API key must be set when authentication is enabled".to_string(),
            ));
        }

        Ok(())
    }

    /// Get the full bind address
    pub fn bind_address(&self) -> String {
        format!("{}:{}", self.server.host, self.server.port)
    }
}

/// Server configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServerConfig {
    /// Host address to bind to
    #[serde(default = "default_host")]
    pub host: String,

    /// Port to listen on
    #[serde(default = "default_port")]
    pub port: u16,

    /// Number of worker threads (0 = auto-detect)
    #[serde(default)]
    pub worker_threads: usize,

    /// Maximum number of concurrent connections
    #[serde(default = "default_max_connections")]
    pub max_connections: usize,

    /// Request timeout in seconds
    #[serde(default = "default_request_timeout")]
    pub request_timeout_secs: u64,
}

impl Default for ServerConfig {
    fn default() -> Self {
        Self {
            host: default_host(),
            port: default_port(),
            worker_threads: 0,
            max_connections: default_max_connections(),
            request_timeout_secs: default_request_timeout(),
        }
    }
}

/// UI configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UiConfig {
    /// Application title
    #[serde(default = "default_title")]
    pub title: String,

    /// Theme (light, dark, auto)
    #[serde(default = "default_theme")]
    pub theme: String,

    /// Number of items per page in lists
    #[serde(default = "default_page_size")]
    pub page_size: usize,

    /// Enable real-time updates
    #[serde(default = "default_true")]
    pub realtime_updates: bool,

    /// Auto-refresh interval in seconds (0 = disabled)
    #[serde(default = "default_refresh_interval")]
    pub refresh_interval_secs: u64,
}

impl Default for UiConfig {
    fn default() -> Self {
        Self {
            title: default_title(),
            theme: default_theme(),
            page_size: default_page_size(),
            realtime_updates: true,
            refresh_interval_secs: default_refresh_interval(),
        }
    }
}

/// Security configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SecurityConfig {
    /// Enable authentication
    #[serde(default)]
    pub auth_enabled: bool,

    /// API key for authentication (if enabled)
    pub api_key: Option<String>,

    /// Enable CORS
    #[serde(default = "default_true")]
    pub cors_enabled: bool,

    /// Allowed CORS origins (empty = allow all)
    #[serde(default)]
    pub cors_origins: Vec<String>,

    /// Enable TLS/HTTPS
    #[serde(default)]
    pub tls_enabled: bool,

    /// Path to TLS certificate file
    pub tls_cert_path: Option<String>,

    /// Path to TLS private key file
    pub tls_key_path: Option<String>,
}

impl Default for SecurityConfig {
    fn default() -> Self {
        Self {
            auth_enabled: false,
            api_key: None,
            cors_enabled: true,
            cors_origins: Vec::new(),
            tls_enabled: false,
            tls_cert_path: None,
            tls_key_path: None,
        }
    }
}

/// Logging configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LoggingConfig {
    /// Log level (trace, debug, info, warn, error)
    #[serde(default = "default_log_level")]
    pub level: String,

    /// Log format (json, pretty)
    #[serde(default = "default_log_format")]
    pub format: String,

    /// Enable file logging
    #[serde(default)]
    pub file_enabled: bool,

    /// Log file path
    pub file_path: Option<String>,

    /// Maximum log file size in MB
    #[serde(default = "default_max_file_size")]
    pub max_file_size_mb: u64,
}

impl Default for LoggingConfig {
    fn default() -> Self {
        Self {
            level: default_log_level(),
            format: default_log_format(),
            file_enabled: false,
            file_path: None,
            max_file_size_mb: default_max_file_size(),
        }
    }
}

// Default value functions
fn default_host() -> String {
    "127.0.0.1".to_string()
}

fn default_port() -> u16 {
    3000
}

fn default_max_connections() -> usize {
    1000
}

fn default_request_timeout() -> u64 {
    30
}

fn default_title() -> String {
    "Worktree Admin".to_string()
}

fn default_theme() -> String {
    "auto".to_string()
}

fn default_page_size() -> usize {
    20
}

fn default_refresh_interval() -> u64 {
    30
}

fn default_true() -> bool {
    true
}

fn default_log_level() -> String {
    "info".to_string()
}

fn default_log_format() -> String {
    "pretty".to_string()
}

fn default_max_file_size() -> u64 {
    100
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config() {
        let config = AdminConfig::default();
        assert_eq!(config.server.host, "127.0.0.1");
        assert_eq!(config.server.port, 3000);
        assert!(!config.security.auth_enabled);
        assert_eq!(config.ui.title, "Worktree Admin");
    }

    #[test]
    fn test_config_from_toml() {
        let toml = r#"
            [server]
            host = "0.0.0.0"
            port = 8080

            [ui]
            title = "My Admin Panel"
            theme = "dark"

            [security]
            auth_enabled = false

            [logging]
            level = "debug"
        "#;

        let config = AdminConfig::from_str(toml).unwrap();
        assert_eq!(config.server.host, "0.0.0.0");
        assert_eq!(config.server.port, 8080);
        assert_eq!(config.ui.title, "My Admin Panel");
        assert_eq!(config.ui.theme, "dark");
        assert_eq!(config.logging.level, "debug");
    }

    #[test]
    fn test_config_validation_port() {
        let mut config = AdminConfig::default();
        config.server.port = 0;
        assert!(config.validate().is_err());
    }

    #[test]
    fn test_config_validation_auth() {
        let mut config = AdminConfig::default();
        config.security.auth_enabled = true;
        config.security.api_key = None;
        assert!(config.validate().is_err());

        config.security.api_key = Some("test_key".to_string());
        assert!(config.validate().is_ok());
    }

    #[test]
    fn test_bind_address() {
        let config = AdminConfig::default();
        assert_eq!(config.bind_address(), "127.0.0.1:3000");
    }
}
