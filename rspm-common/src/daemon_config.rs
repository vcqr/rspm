use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;

/// Default gRPC port
pub const DEFAULT_GRPC_PORT: u16 = 6680;

/// Default host
pub const DEFAULT_HOST: &str = "127.0.0.1";

/// Default config file path (relative to rspm dir)
pub const DEFAULT_CONFIG_FILE: &str = ".env";

/// Daemon configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DaemonConfig {
    /// gRPC server host
    #[serde(default = "default_host")]
    pub host: String,

    /// gRPC server port
    #[serde(default = "default_grpc_port")]
    pub port: u16,

    /// Web dashboard port (defaults to grpc port + 1)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub web_port: Option<u16>,

    /// Authentication token for remote connections
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub token: Option<String>,

    /// Additional environment variables
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub env: HashMap<String, String>,
}

impl Default for DaemonConfig {
    fn default() -> Self {
        Self {
            host: default_host(),
            port: DEFAULT_GRPC_PORT,
            web_port: None,
            token: None,
            env: HashMap::new(),
        }
    }
}

fn default_host() -> String {
    DEFAULT_HOST.to_string()
}

fn default_grpc_port() -> u16 {
    DEFAULT_GRPC_PORT
}

impl DaemonConfig {
    /// Load configuration from file
    /// Supports .env, .yaml, .yml, .json, .toml formats
    pub fn from_file(path: &PathBuf) -> crate::Result<Self> {
        let content = std::fs::read_to_string(path).map_err(|e| {
            crate::RspmError::ConfigError(format!("Failed to read config file: {}", e))
        })?;

        let ext = path
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("")
            .to_lowercase();

        let config = match ext.as_str() {
            "yaml" | "yml" => serde_yaml::from_str(&content).map_err(|e| {
                crate::RspmError::ConfigError(format!("Failed to parse YAML config: {}", e))
            })?,
            "json" => serde_json::from_str(&content).map_err(|e| {
                crate::RspmError::ConfigError(format!("Failed to parse JSON config: {}", e))
            })?,
            "toml" => toml::from_str(&content).map_err(|e| {
                crate::RspmError::ConfigError(format!("Failed to parse TOML config: {}", e))
            })?,
            "env" | "" => {
                // Parse as env file format (key=value pairs)
                Self::from_env_content(&content)?
            }
            _ => {
                // Try to auto-detect format
                if content.trim().starts_with('{') {
                    serde_json::from_str(&content).map_err(|e| {
                        crate::RspmError::ConfigError(format!("Failed to parse config: {}", e))
                    })?
                } else if content.trim().starts_with('[') || content.contains(":") {
                    serde_yaml::from_str(&content).map_err(|e| {
                        crate::RspmError::ConfigError(format!("Failed to parse config: {}", e))
                    })?
                } else {
                    Self::from_env_content(&content)?
                }
            }
        };

        Ok(config)
    }

    /// Parse env file content (key=value pairs)
    fn from_env_content(content: &str) -> crate::Result<Self> {
        let mut config = DaemonConfig::default();

        for line in content.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }

            if let Some((key, value)) = line.split_once('=') {
                let key = key.trim();
                let value = value.trim().trim_matches('"').trim_matches('\'');

                match key.to_lowercase().as_str() {
                    "host" => config.host = value.to_string(),
                    "port" => {
                        config.port = value.parse().map_err(|e| {
                            crate::RspmError::ConfigError(format!("Invalid port: {}", e))
                        })?;
                    }
                    "web_port" => {
                        config.web_port = Some(value.parse().map_err(|e| {
                            crate::RspmError::ConfigError(format!("Invalid web_port: {}", e))
                        })?);
                    }
                    "token" => {
                        if !value.is_empty() {
                            config.token = Some(value.to_string());
                        }
                    }
                    _ => {
                        config.env.insert(key.to_string(), value.to_string());
                    }
                }
            }
        }

        Ok(config)
    }

    /// Get the web dashboard port
    pub fn get_web_port(&self) -> u16 {
        self.web_port.unwrap_or(self.port + 1)
    }

    /// Get the gRPC address (host:port)
    pub fn get_grpc_addr(&self) -> String {
        format!("{}:{}", self.host, self.port)
    }

    /// Get default config file path (~/.rspm/.env)
    pub fn get_default_config_path() -> PathBuf {
        crate::get_rspm_dir().join(DEFAULT_CONFIG_FILE)
    }

    /// Load config from default location or return default config
    pub fn load_default() -> Self {
        let default_path = Self::get_default_config_path();
        if default_path.exists() {
            Self::from_file(&default_path).unwrap_or_default()
        } else {
            Self::default()
        }
    }

    /// Check if token is required (for remote connections)
    pub fn requires_token(&self) -> bool {
        self.host != "127.0.0.1" && self.host != "localhost" && !self.host.is_empty()
    }

    /// Get the authentication token
    pub fn get_token(&self) -> Option<&str> {
        self.token.as_deref()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config() {
        let config = DaemonConfig::default();
        assert_eq!(config.host, "127.0.0.1");
        assert_eq!(config.port, 6680);
        assert_eq!(config.get_web_port(), 6681);
    }

    #[test]
    fn test_from_env_content() {
        let content = r#"
# This is a comment
HOST=0.0.0.0
PORT=60000
WEB_PORT=60001
CUSTOM_VAR=value
"#;
        let config = DaemonConfig::from_env_content(content).unwrap();
        assert_eq!(config.host, "0.0.0.0");
        assert_eq!(config.port, 60000);
        assert_eq!(config.web_port, Some(60001));
        assert_eq!(config.env.get("CUSTOM_VAR"), Some(&"value".to_string()));
    }
}
