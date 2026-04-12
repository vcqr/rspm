use serde::{Deserialize, Serialize};
use std::path::Path;

use crate::ProcessConfig;

/// Supported configuration file formats
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConfigFormat {
    Json,
    Yaml,
    Toml,
}

impl ConfigFormat {
    /// Detect format from file extension
    pub fn from_path(path: &Path) -> Option<Self> {
        let ext = path.extension()?.to_str()?.to_lowercase();
        match ext.as_str() {
            "json" => Some(ConfigFormat::Json),
            "yaml" | "yml" => Some(ConfigFormat::Yaml),
            "toml" => Some(ConfigFormat::Toml),
            _ => None,
        }
    }
}

/// Multi-process configuration file structure
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConfigFile {
    /// List of process configurations
    #[serde(default)]
    pub processes: Vec<ProcessConfig>,
}

impl ConfigFile {
    /// Create a new empty config file
    pub fn new() -> Self {
        Self {
            processes: Vec::new(),
        }
    }

    /// Create from a single process config
    pub fn single(config: ProcessConfig) -> Self {
        Self {
            processes: vec![config],
        }
    }

    /// Add a process configuration
    pub fn add(&mut self, config: ProcessConfig) {
        self.processes.push(config);
    }

    /// Check if this is a single-process config
    pub fn is_single(&self) -> bool {
        self.processes.len() == 1
    }

    /// Get the single process if this is a single-process config
    pub fn as_single(&self) -> Option<&ProcessConfig> {
        if self.is_single() {
            self.processes.first()
        } else {
            None
        }
    }

    /// Get all process configurations
    pub fn into_processes(self) -> Vec<ProcessConfig> {
        self.processes
    }
}

impl Default for ConfigFile {
    fn default() -> Self {
        Self::new()
    }
}
