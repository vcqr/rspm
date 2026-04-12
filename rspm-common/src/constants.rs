use std::path::PathBuf;

/// Get the base rspm directory: ~/.rspm
pub fn get_rspm_dir() -> PathBuf {
    dirs::home_dir()
        .map(|home| home.join(".rspm"))
        .unwrap_or_else(|| PathBuf::from("/tmp/rspm"))
}

/// Get the socket directory: ~/.rspm/sock
pub fn get_socket_dir() -> PathBuf {
    get_rspm_dir().join("sock")
}

/// Get the PID directory: ~/.rspm/pid
pub fn get_pid_dir() -> PathBuf {
    get_rspm_dir().join("pid")
}

/// Get the logs directory: ~/.rspm/logs
pub fn get_logs_dir() -> PathBuf {
    get_rspm_dir().join("logs")
}

/// Get the config directory: ~/.rspm/config
pub fn get_config_dir() -> PathBuf {
    get_rspm_dir().join("config")
}

/// Get the database directory: ~/.rspm/db
pub fn get_db_dir() -> PathBuf {
    get_rspm_dir().join("db")
}

/// Get the default socket path
#[cfg(unix)]
pub fn get_socket_path() -> PathBuf {
    get_socket_dir().join("rspm.sock")
}

#[cfg(windows)]
pub fn get_socket_path() -> PathBuf {
    // Windows uses TCP, get the actual address from config
    let config = crate::DaemonConfig::load_default();
    PathBuf::from(config.get_grpc_addr())
}

/// Get the default PID file path
pub fn get_pid_file() -> PathBuf {
    get_pid_dir().join("rspm.pid")
}

/// Get the default database path
pub fn get_db_path() -> PathBuf {
    get_db_dir().join("rspm.db")
}

// Legacy constants for backward compatibility (deprecated)
#[deprecated(since = "0.1.0", note = "Use get_socket_path() instead")]
#[cfg(unix)]
pub const DEFAULT_SOCKET_PATH: &str = "/tmp/rspm.sock";

#[deprecated(since = "0.1.0", note = "Use get_socket_path() instead")]
#[cfg(windows)]
pub const DEFAULT_SOCKET_PATH: &str = {
    // Use the default port from DaemonConfig
    "127.0.0.1:6680"
};

#[deprecated(since = "0.1.0", note = "Use get_pid_file() instead")]
#[cfg(unix)]
pub const DEFAULT_PID_FILE: &str = "/tmp/rspm.pid";

#[deprecated(since = "0.1.0", note = "Use get_pid_file() instead")]
#[cfg(windows)]
pub const DEFAULT_PID_FILE: &str = "rspm.pid";

#[deprecated(since = "0.1.0", note = "Use get_logs_dir() instead")]
#[cfg(unix)]
pub const DEFAULT_LOG_DIR: &str = "/tmp/rspm/logs";

#[deprecated(since = "0.1.0", note = "Use get_logs_dir() instead")]
#[cfg(windows)]
pub const DEFAULT_LOG_DIR: &str = "rspm\\logs";

#[deprecated(since = "0.1.0", note = "Use get_config_dir() instead")]
#[cfg(unix)]
pub const DEFAULT_CONFIG_DIR: &str = "/tmp/rspm/config";

#[deprecated(since = "0.1.0", note = "Use get_config_dir() instead")]
#[cfg(windows)]
pub const DEFAULT_CONFIG_DIR: &str = "rspm\\config";

/// Daemon version
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// Default monitoring interval in milliseconds
pub const DEFAULT_MONITOR_INTERVAL_MS: u64 = 1000;

/// Default log flush interval in milliseconds
pub const DEFAULT_LOG_FLUSH_INTERVAL_MS: u64 = 100;

/// Minimum time before a process is considered "stable" (no immediate restart)
pub const STABLE_RUN_THRESHOLD_MS: u64 = 60_000; // 1 minute

/// Print the RSPM banner with version
pub fn print_banner() {
    println!(
        r#"
   _____   _____   ____   __  __
  |  __ \ / ____| |  _ \ |  \/  |
  | |__) | (___   | |_) || \  / |
  |  _  / \___ \  |  __/ | |\/| |
  | | \ \ ____) | | |    | |  | |
  |_|  \_\_____/  |_|    |_|  |_|
                🦀 rspm v{}
"#,
        VERSION
    );
}
