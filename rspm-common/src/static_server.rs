use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// Static file server configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StaticServerConfig {
    pub id: String,
    pub name: String,
    pub host: String,
    pub port: i32,
    pub directory: String,
}

/// Static file server information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StaticServerInfo {
    pub id: String,
    pub name: String,
    pub host: String,
    pub port: i32,
    pub directory: String,
    pub running: bool,
    pub started_at: Option<DateTime<Utc>>,
}

impl StaticServerInfo {
    pub fn url(&self) -> String {
        format!("http://{}:{}", self.host, self.port)
    }
}
