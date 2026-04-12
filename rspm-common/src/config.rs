use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Server type for the process
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
#[derive(Default)]
pub enum ServerType {
    /// Normal process
    #[default]
    Process,
    /// Static file server
    StaticServer,
}

/// Process configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProcessConfig {
    /// Unique name for the process
    pub name: String,
    /// Command to execute
    pub command: String,
    /// Command line arguments
    #[serde(default)]
    pub args: Vec<String>,
    /// Environment variables
    #[serde(default)]
    pub env: HashMap<String, String>,
    /// Working directory
    #[serde(default)]
    pub cwd: Option<String>,
    /// Number of instances for load balancing
    #[serde(default = "default_instances")]
    pub instances: u32,
    /// Auto restart on crash
    #[serde(default = "default_autorestart")]
    pub autorestart: bool,
    /// Max restart attempts before marking as errored
    #[serde(default = "default_max_restarts")]
    pub max_restarts: u32,
    /// Memory limit in MB, 0 means unlimited
    #[serde(default)]
    pub max_memory_mb: u32,
    /// Watch for file changes (dev mode)
    #[serde(default)]
    pub watch: bool,
    /// Paths to watch for file changes
    #[serde(default)]
    pub watch_paths: Vec<String>,
    /// Log file path
    #[serde(default)]
    pub log_file: Option<String>,
    /// Error log file path
    #[serde(default)]
    pub error_file: Option<String>,
    /// Max log file size in bytes
    #[serde(default = "default_log_max_size")]
    pub log_max_size: u64,
    /// Max number of rotated log files
    #[serde(default = "default_log_max_files")]
    pub log_max_files: u32,
    /// Server type (normal process or static file server)
    #[serde(default)]
    pub server_type: ServerType,
}

fn default_instances() -> u32 {
    1
}
fn default_autorestart() -> bool {
    true
}
fn default_max_restarts() -> u32 {
    15
}
fn default_log_max_size() -> u64 {
    10 * 1024 * 1024
} // 10MB
fn default_log_max_files() -> u32 {
    5
}

impl Default for ProcessConfig {
    fn default() -> Self {
        Self {
            name: String::new(),
            command: String::new(),
            args: Vec::new(),
            env: HashMap::new(),
            cwd: None,
            instances: default_instances(),
            autorestart: default_autorestart(),
            max_restarts: default_max_restarts(),
            max_memory_mb: 0,
            watch: false,
            watch_paths: Vec::new(),
            log_file: None,
            error_file: None,
            log_max_size: default_log_max_size(),
            log_max_files: default_log_max_files(),
            server_type: ServerType::default(),
        }
    }
}

impl ProcessConfig {
    /// Create a new process config with the given name and command
    pub fn new(name: impl Into<String>, command: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            command: command.into(),
            ..Default::default()
        }
    }

    /// Set command arguments
    pub fn args(mut self, args: Vec<String>) -> Self {
        self.args = args;
        self
    }

    /// Set environment variables
    pub fn env(mut self, env: HashMap<String, String>) -> Self {
        self.env = env;
        self
    }

    /// Set working directory
    pub fn cwd(mut self, cwd: impl Into<String>) -> Self {
        self.cwd = Some(cwd.into());
        self
    }

    /// Set number of instances
    pub fn instances(mut self, instances: u32) -> Self {
        self.instances = instances.max(1);
        self
    }

    /// Set auto restart
    pub fn autorestart(mut self, autorestart: bool) -> Self {
        self.autorestart = autorestart;
        self
    }

    /// Set max restarts
    pub fn max_restarts(mut self, max_restarts: u32) -> Self {
        self.max_restarts = max_restarts;
        self
    }

    /// Set memory limit in MB
    pub fn max_memory_mb(mut self, max_memory_mb: u32) -> Self {
        self.max_memory_mb = max_memory_mb;
        self
    }

    /// Set watch mode
    pub fn watch(mut self, watch: bool) -> Self {
        self.watch = watch;
        self
    }

    /// Set watch paths
    pub fn watch_paths(mut self, paths: Vec<String>) -> Self {
        self.watch_paths = paths;
        self
    }

    /// Set log file path
    pub fn log_file(mut self, path: impl Into<String>) -> Self {
        self.log_file = Some(path.into());
        self
    }

    /// Set error log file path
    pub fn error_file(mut self, path: impl Into<String>) -> Self {
        self.error_file = Some(path.into());
        self
    }
}
