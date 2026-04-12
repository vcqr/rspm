use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::time::{Duration, Instant};

/// Process state
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum ProcessState {
    #[default]
    Stopped,
    Starting,
    Running,
    Stopping,
    Errored,
}

impl std::fmt::Display for ProcessState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ProcessState::Stopped => write!(f, "stopped"),
            ProcessState::Starting => write!(f, "starting"),
            ProcessState::Running => write!(f, "running"),
            ProcessState::Stopping => write!(f, "stopping"),
            ProcessState::Errored => write!(f, "errored"),
        }
    }
}

/// Restart policy with exponential backoff
#[derive(Debug, Clone)]
pub struct RestartPolicy {
    /// Maximum number of restarts within the window
    pub max_restarts: u32,
    /// Current restart count
    pub restart_count: u32,
    /// Time window for restart counting (in seconds)
    pub restart_window_secs: u64,
    /// Base delay for exponential backoff (in milliseconds)
    pub base_delay_ms: u64,
    /// Maximum delay for exponential backoff (in milliseconds)
    pub max_delay_ms: u64,
    /// Last restart time
    pub last_restart: Option<Instant>,
    /// Restart timestamps within the window
    pub restart_times: Vec<Instant>,
}

impl Default for RestartPolicy {
    fn default() -> Self {
        Self {
            max_restarts: 15,
            restart_count: 0,
            restart_window_secs: 60,
            base_delay_ms: 1000,
            max_delay_ms: 60000,
            last_restart: None,
            restart_times: Vec::new(),
        }
    }
}

impl RestartPolicy {
    pub fn new(max_restarts: u32) -> Self {
        Self {
            max_restarts,
            ..Default::default()
        }
    }

    /// Calculate the delay before next restart using exponential backoff
    /// Returns None if max restarts exceeded
    pub fn calculate_delay(&mut self) -> Option<Duration> {
        let now = Instant::now();

        // Clean up old restart times outside the window
        let window_ago = now - Duration::from_secs(self.restart_window_secs);
        self.restart_times.retain(|&t| t > window_ago);

        // Check if we've exceeded max restarts in the window
        if self.restart_times.len() >= self.max_restarts as usize {
            return None;
        }

        // Calculate exponential backoff delay
        let delay_ms = if self.restart_times.is_empty() {
            0
        } else {
            let count = self.restart_times.len() as u32;
            let delay = self.base_delay_ms * (2u64.pow(count - 1));
            delay.min(self.max_delay_ms)
        };

        // Record this restart attempt
        self.restart_times.push(now);
        self.restart_count += 1;
        self.last_restart = Some(now);

        Some(Duration::from_millis(delay_ms))
    }

    /// Reset restart counters (called on successful stable run)
    pub fn reset(&mut self) {
        self.restart_count = 0;
        self.restart_times.clear();
        self.last_restart = None;
    }

    /// Check if max restarts exceeded
    pub fn is_exceeded(&self) -> bool {
        let now = Instant::now();
        let window_ago = now - Duration::from_secs(self.restart_window_secs);
        let recent_restarts = self
            .restart_times
            .iter()
            .filter(|&&t| t > window_ago)
            .count();
        recent_restarts >= self.max_restarts as usize
    }
}

/// Process statistics
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ProcessStats {
    /// CPU usage percentage (0-100)
    pub cpu_percent: f64,
    /// Memory usage in bytes
    pub memory_bytes: u64,
    /// Number of file descriptors
    pub fd_count: u32,
}

/// Process runtime information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProcessInfo {
    /// Database auto-increment ID
    pub db_id: Option<i64>,
    /// Unique process ID (display ID)
    pub id: String,
    /// Process name
    pub name: String,
    /// Current state
    pub state: ProcessState,
    /// System process ID (if running)
    pub pid: Option<u32>,
    /// Instance number for clustered processes
    pub instance_id: u32,
    /// Process configuration
    pub config: crate::config::ProcessConfig,
    /// Uptime in milliseconds
    pub uptime_ms: u64,
    /// Number of restarts
    pub restart_count: u32,
    /// Current stats
    pub stats: ProcessStats,
    /// Creation timestamp
    pub created_at: DateTime<Utc>,
    /// Last start timestamp
    pub started_at: Option<DateTime<Utc>>,
    /// Exit code (if stopped)
    pub exit_code: Option<i32>,
    /// Error message (if errored)
    pub error_message: Option<String>,
}

impl ProcessInfo {
    pub fn new(
        db_id: Option<i64>,
        id: String,
        name: String,
        instance_id: u32,
        config: crate::config::ProcessConfig,
    ) -> Self {
        Self {
            db_id,
            id,
            name,
            state: ProcessState::Stopped,
            pid: None,
            instance_id,
            config,
            uptime_ms: 0,
            restart_count: 0,
            stats: ProcessStats::default(),
            created_at: Utc::now(),
            started_at: None,
            exit_code: None,
            error_message: None,
        }
    }

    /// Check if the process is running
    pub fn is_running(&self) -> bool {
        matches!(self.state, ProcessState::Running | ProcessState::Starting)
    }
}

/// Log entry
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LogEntry {
    /// Process ID
    pub process_id: String,
    /// Timestamp
    pub timestamp: DateTime<Utc>,
    /// Log message
    pub message: String,
    /// Is error output
    pub is_error: bool,
}

impl LogEntry {
    pub fn new(process_id: String, message: String, is_error: bool) -> Self {
        Self {
            process_id,
            timestamp: Utc::now(),
            message,
            is_error,
        }
    }
}
