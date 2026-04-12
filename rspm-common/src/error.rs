use thiserror::Error;

#[derive(Error, Debug)]
pub enum RspmError {
    #[error("Process not found: {0}")]
    ProcessNotFound(String),

    #[error("Process already exists: {0}")]
    ProcessAlreadyExists(String),

    #[error("Failed to start process: {0}")]
    StartFailed(String),

    #[error("Failed to stop process: {0}")]
    StopFailed(String),

    #[error("Failed to spawn process: {0}")]
    SpawnFailed(String),

    #[error("Invalid configuration: {0}")]
    InvalidConfig(String),

    #[error("Daemon not running")]
    DaemonNotRunning,

    #[error("Daemon already running")]
    DaemonAlreadyRunning,

    #[error("IO error: {0}")]
    IoError(#[from] std::io::Error),

    #[error("Serialization error: {0}")]
    SerializationError(#[from] serde_json::Error),

    #[error("gRPC error: {0}")]
    GrpcError(String),

    #[error("Process state error: {0}")]
    StateError(String),

    #[error("Log error: {0}")]
    LogError(String),

    #[error("Monitor error: {0}")]
    MonitorError(String),

    #[error("Config error: {0}")]
    ConfigError(String),

    #[error("Config parse error: {0}")]
    ConfigParseError(String),

    #[error("Unsupported config format: {0}")]
    UnsupportedConfigFormat(String),

    #[error("Internal error: {0}")]
    InternalError(String),

    #[error("Database error: {0}")]
    DatabaseError(String),

    #[error("Scheduler error: {0}")]
    SchedulerError(String),

    #[error("Not found: {0}")]
    NotFound(String),
}

pub type Result<T> = std::result::Result<T, RspmError>;
