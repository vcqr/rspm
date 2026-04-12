use chrono::Utc;
use rspm_common::LogEntry;
use std::fs::{File, OpenOptions};
use std::io::{BufRead, BufReader, Write};
use std::path::PathBuf;
use std::sync::RwLock;
use tokio::sync::broadcast;
use tracing::error;

/// Log writer with rotation support
pub struct LogWriter {
    base_dir: PathBuf,
    stdout_file: RwLock<Option<File>>,
    stderr_file: RwLock<Option<File>>,
    stdout_path: RwLock<Option<PathBuf>>,
    #[allow(dead_code)]
    stderr_path: RwLock<Option<PathBuf>>,
    max_size: RwLock<u64>,
    max_files: RwLock<u32>,
    current_size: RwLock<u64>,
    log_tx: broadcast::Sender<LogEntry>,
}

impl LogWriter {
    pub fn new(base_dir: PathBuf) -> Self {
        let (log_tx, _) = broadcast::channel(1024);

        Self {
            base_dir,
            stdout_file: RwLock::new(None),
            stderr_file: RwLock::new(None),
            stdout_path: RwLock::new(None),
            stderr_path: RwLock::new(None),
            max_size: RwLock::new(10 * 1024 * 1024), // 10MB default
            max_files: RwLock::new(5),
            current_size: RwLock::new(0),
            log_tx,
        }
    }

    /// Create a new log writer for a specific process instance
    pub fn create_instance_writer(
        &self,
        process_name: &str,
        instance_id: u32,
        custom_stdout: Option<&str>,
        custom_stderr: Option<&str>,
        max_size: u64,
        max_files: u32,
    ) -> Self {
        let mut log_dir = self.base_dir.clone();
        log_dir.push(process_name);

        let stdout_filename = if instance_id == 0 {
            "stdout.log".to_string()
        } else {
            format!("stdout-{}.log", instance_id)
        };

        let stderr_filename = if instance_id == 0 {
            "stderr.log".to_string()
        } else {
            format!("stderr-{}.log", instance_id)
        };

        let stdout_path = if let Some(custom) = custom_stdout {
            PathBuf::from(custom)
        } else {
            log_dir.join(&stdout_filename)
        };

        let stderr_path = if let Some(custom) = custom_stderr {
            PathBuf::from(custom)
        } else {
            log_dir.join(&stderr_filename)
        };

        // Ensure directories exist
        if let Some(parent) = stdout_path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        if let Some(parent) = stderr_path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }

        let stdout_file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&stdout_path)
            .ok();

        let stderr_file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&stderr_path)
            .ok();

        let (log_tx, _) = broadcast::channel(1024);

        Self {
            base_dir: self.base_dir.clone(),
            stdout_file: RwLock::new(stdout_file),
            stderr_file: RwLock::new(stderr_file),
            stdout_path: RwLock::new(Some(stdout_path)),
            stderr_path: RwLock::new(Some(stderr_path)),
            max_size: RwLock::new(max_size),
            max_files: RwLock::new(max_files),
            current_size: RwLock::new(0),
            log_tx,
        }
    }

    /// Write to stdout log
    pub async fn write_stdout(&self, process_id: &str, line: &str) {
        let entry = LogEntry::new(process_id.to_string(), line.to_string(), false);

        // Broadcast to subscribers
        let _ = self.log_tx.send(entry.clone());

        // Write to file
        if let Ok(mut file_guard) = self.stdout_file.write()
            && let Some(ref mut file) = *file_guard
        {
            let timestamp = entry.timestamp.format("%Y-%m-%d %H:%M:%S%.3f");
            let log_line = format!("[{}] {}\n", timestamp, line);

            if let Err(e) = file.write_all(log_line.as_bytes()) {
                error!("Failed to write stdout log: {}", e);
            }

            // Check for rotation
            if let Ok(mut current_size) = self.current_size.write() {
                *current_size += log_line.len() as u64;
                if *current_size > *self.max_size.read().unwrap() {
                    drop(file_guard);
                    drop(current_size);
                    let _ = self.rotate_stdout();
                }
            }
        }
    }

    /// Write to stderr log
    pub async fn write_stderr(&self, process_id: &str, line: &str) {
        let entry = LogEntry::new(process_id.to_string(), line.to_string(), true);

        // Broadcast to subscribers
        let _ = self.log_tx.send(entry.clone());

        // Write to file
        if let Ok(mut file_guard) = self.stderr_file.write()
            && let Some(ref mut file) = *file_guard
        {
            let timestamp = entry.timestamp.format("%Y-%m-%d %H:%M:%S%.3f");
            let log_line = format!("[{}] {}\n", timestamp, line);

            if let Err(e) = file.write_all(log_line.as_bytes()) {
                error!("Failed to write stderr log: {}", e);
            }
        }
    }

    /// Rotate stdout log
    fn rotate_stdout(&self) -> std::io::Result<()> {
        let path_guard = self.stdout_path.read().unwrap();
        let path = path_guard.as_ref().ok_or_else(|| {
            std::io::Error::new(std::io::ErrorKind::NotFound, "No stdout path set")
        })?;

        let max_files = *self.max_files.read().unwrap();

        // Delete oldest file
        let oldest = format!("{}.{}", path.display(), max_files);
        let _ = std::fs::remove_file(&oldest);

        // Rotate existing files
        for i in (1..=max_files).rev() {
            let old_path = format!("{}.{}", path.display(), i);
            let new_path = format!("{}.{}", path.display(), i + 1);
            if std::path::Path::new(&old_path).exists() {
                std::fs::rename(&old_path, &new_path)?;
            }
        }

        // Move current to .1
        let new_path = format!("{}.1", path.display());
        std::fs::rename(path, &new_path)?;

        // Create new file
        let new_file = OpenOptions::new().create(true).append(true).open(path)?;

        drop(path_guard);

        let mut file_guard = self.stdout_file.write().unwrap();
        *file_guard = Some(new_file);

        let mut current_size = self.current_size.write().unwrap();
        *current_size = 0;

        Ok(())
    }

    /// Subscribe to log entries
    pub fn subscribe(&self) -> broadcast::Receiver<LogEntry> {
        self.log_tx.subscribe()
    }

    /// Flush logs
    pub fn flush(&self) -> std::io::Result<()> {
        if let Ok(mut file_guard) = self.stdout_file.write()
            && let Some(ref mut file) = *file_guard
        {
            file.flush()?;
        }
        if let Ok(mut file_guard) = self.stderr_file.write()
            && let Some(ref mut file) = *file_guard
        {
            file.flush()?;
        }
        Ok(())
    }

    /// Read last N lines from stdout log file
    pub fn read_stdout_history(&self, lines: usize) -> std::io::Result<Vec<LogEntry>> {
        let path_guard = self.stdout_path.read().unwrap();
        let path = match path_guard.as_ref() {
            Some(p) => p.clone(),
            None => return Ok(Vec::new()),
        };
        drop(path_guard);

        self.read_log_file(&path, lines, false)
    }

    /// Read last N lines from stderr log file
    pub fn read_stderr_history(&self, lines: usize) -> std::io::Result<Vec<LogEntry>> {
        let path_guard = self.stderr_path.read().unwrap();
        let path = match path_guard.as_ref() {
            Some(p) => p.clone(),
            None => return Ok(Vec::new()),
        };
        drop(path_guard);

        self.read_log_file(&path, lines, true)
    }

    /// Read last N lines from a log file
    fn read_log_file(
        &self,
        path: &PathBuf,
        lines: usize,
        is_error: bool,
    ) -> std::io::Result<Vec<LogEntry>> {
        if !path.exists() {
            return Ok(Vec::new());
        }

        let file = File::open(path)?;
        let reader = BufReader::new(file);
        let all_lines: Vec<String> = reader.lines().filter_map(|l| l.ok()).collect();

        // Get last N lines
        let start_idx = all_lines.len().saturating_sub(lines);
        let recent_lines: Vec<String> = all_lines.into_iter().skip(start_idx).collect();

        // Parse lines into LogEntry
        let entries: Vec<LogEntry> = recent_lines
            .into_iter()
            .enumerate()
            .map(|(idx, line)| {
                // Try to parse timestamp from format [YYYY-MM-DD HH:MM:SS.mmm] message
                let timestamp = if line.starts_with('[') {
                    if let Some(end_idx) = line.find("] ") {
                        let ts_str = &line[1..end_idx];
                        chrono::NaiveDateTime::parse_from_str(ts_str, "%Y-%m-%d %H:%M:%S%.3f")
                            .map(|dt| dt.and_utc())
                            .unwrap_or_else(|_| Utc::now())
                    } else {
                        Utc::now()
                    }
                } else {
                    Utc::now()
                };

                let message = if line.starts_with('[') && line.contains("] ") {
                    line.split_once("] ")
                        .map(|x| x.1)
                        .unwrap_or(&line)
                        .to_string()
                } else {
                    line
                };

                LogEntry {
                    process_id: format!("history-{}", idx),
                    timestamp,
                    message,
                    is_error,
                }
            })
            .collect();

        Ok(entries)
    }

    /// Get stdout log file path
    pub fn get_stdout_path(&self) -> Option<PathBuf> {
        self.stdout_path.read().unwrap().clone()
    }

    /// Get stderr log file path
    pub fn get_stderr_path(&self) -> Option<PathBuf> {
        self.stderr_path.read().unwrap().clone()
    }
}
