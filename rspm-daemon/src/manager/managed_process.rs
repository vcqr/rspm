use chrono::Utc;
use rspm_common::utils::generate_process_id;
use rspm_common::{ProcessConfig, ProcessInfo, ProcessState, ProcessStats, Result, RspmError};
use rspm_common::{RestartPolicy, STABLE_RUN_THRESHOLD_MS};
use std::process::Stdio;
use std::sync::Arc;
use std::time::Instant;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::{Child, Command};
use tracing::{info, warn};

use crate::log_watcher::LogWriter;

/// A managed process instance
pub struct ManagedProcess {
    pub info: ProcessInfo,
    pub config: ProcessConfig,
    pub child: Option<Child>,
    pub restart_policy: RestartPolicy,
    pub start_time: Option<Instant>,
    pub log_writer: Option<Arc<LogWriter>>,
    /// Static server ID if this is a static file server
    pub static_server_id: Option<String>,
}

impl ManagedProcess {
    pub fn new(db_id: Option<i64>, config: ProcessConfig, instance_id: u32) -> Self {
        let id = generate_process_id(&config.name, instance_id);
        let info = ProcessInfo::new(
            db_id,
            id.clone(),
            config.name.clone(),
            instance_id,
            config.clone(),
        );

        let restart_policy = RestartPolicy::new(config.max_restarts);

        Self {
            info,
            config,
            child: None,
            restart_policy,
            start_time: None,
            log_writer: None,
            static_server_id: None,
        }
    }

    /// Build the command, handling Windows batch/cmd scripts
    fn build_command(config: &ProcessConfig) -> Result<Command> {
        #[cfg(windows)]
        {
            // On Windows, wrap command in cmd.exe /c to support .cmd/.bat scripts like npm
            let mut cmd = Command::new("cmd");
            cmd.arg("/C");

            // Build command string: command + args
            let mut cmd_parts = vec![config.command.clone()];
            cmd_parts.extend(config.args.iter().cloned());
            let full_cmd = cmd_parts.join(" ");
            cmd.arg(&full_cmd);

            info!("Windows command: cmd /C {}", full_cmd);

            Ok(cmd)
        }

        #[cfg(not(windows))]
        {
            // Use shell to execute the command
            // Don't use 'exec' so we can properly track the child process
            let mut cmd = Command::new("/bin/sh");
            let mut cmd_parts = vec![config.command.clone()];
            cmd_parts.extend(config.args.iter().cloned());
            let full_cmd = cmd_parts.join(" ");
            cmd.arg("-c").arg(&full_cmd);

            info!("Executing command: /bin/sh -c {}", full_cmd);
            Ok(cmd)
        }
    }

    /// Start the process
    pub async fn start(&mut self, log_writer: Option<Arc<LogWriter>>) -> Result<()> {
        if self.info.is_running() {
            return Err(RspmError::StateError("Process already running".to_string()));
        }

        self.info.state = ProcessState::Starting;
        self.log_writer = log_writer;

        // Build the command
        let mut cmd = Self::build_command(&self.config)?;

        // Set environment variables
        for (key, value) in &self.config.env {
            cmd.env(key, value);
        }

        // Set working directory
        if let Some(cwd) = &self.config.cwd {
            cmd.current_dir(cwd);
        }

        // Configure stdio
        // Use piped stdin to prevent processes from exiting when stdin is closed
        cmd.stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .stdin(Stdio::piped());

        // Kill process on drop
        cmd.kill_on_drop(true);

        // Set process group on Unix for proper signal handling
        #[cfg(unix)]
        {
            cmd.process_group(0); // Create new process group
        }

        // Spawn the process
        let mut child = match cmd.spawn() {
            Ok(c) => c,
            Err(e) => {
                self.info.state = ProcessState::Errored;
                self.info.error_message = Some(e.to_string());
                return Err(RspmError::SpawnFailed(e.to_string()));
            }
        };

        // Get PID
        let pid = child.id();
        self.info.pid = pid;
        self.info.state = ProcessState::Running;
        self.info.started_at = Some(Utc::now());
        self.start_time = Some(Instant::now());
        self.info.error_message = None;
        self.info.exit_code = None;

        // Spawn stdout reader task
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| RspmError::InternalError("Failed to capture stdout".to_string()))?;

        let process_id = self.info.id.clone();
        let log_writer_clone = self.log_writer.clone();
        tokio::spawn(async move {
            let reader = BufReader::new(stdout);
            let mut lines = reader.lines();
            while let Ok(Some(line)) = lines.next_line().await {
                if let Some(ref writer) = log_writer_clone {
                    writer.write_stdout(&process_id, &line).await;
                }
            }
        });

        // Spawn stderr reader task
        let stderr = child
            .stderr
            .take()
            .ok_or_else(|| RspmError::InternalError("Failed to capture stderr".to_string()))?;

        let process_id = self.info.id.clone();
        let log_writer_clone = self.log_writer.clone();
        tokio::spawn(async move {
            let reader = BufReader::new(stderr);
            let mut lines = reader.lines();
            while let Ok(Some(line)) = lines.next_line().await {
                if let Some(ref writer) = log_writer_clone {
                    writer.write_stderr(&process_id, &line).await;
                }
            }
        });

        self.child = Some(child);

        Ok(())
    }

    /// Stop the process
    /// `force` - if true, forcefully kill the process (SIGKILL on Unix, taskkill /F on Windows)
    pub async fn stop(&mut self, force: bool) -> Result<()> {
        if !self.info.is_running() {
            return Ok(());
        }

        self.info.state = ProcessState::Stopping;

        if let Some(ref mut child) = self.child.take() {
            let pid = child.id();

            #[cfg(windows)]
            {
                if force {
                    // On Windows, use taskkill /F /T to force kill the process tree
                    // This is more reliable than tokio's kill() for npm/node processes
                    if let Some(pid) = pid {
                        let kill_result = std::process::Command::new("taskkill")
                            .args(["/F", "/T", "/PID"])
                            .arg(pid.to_string())
                            .output();

                        match kill_result {
                            Ok(output) => {
                                if !output.status.success() {
                                    let stderr = String::from_utf8_lossy(&output.stderr);
                                    warn!("taskkill output: {}", stderr);
                                }
                            }
                            Err(e) => {
                                warn!("taskkill failed: {}", e);
                            }
                        }
                    }
                }
            }

            #[cfg(unix)]
            {
                if force {
                    // Kill the entire process group to ensure child processes are terminated
                    use libc::{SIGKILL, kill};
                    if let Some(pid) = self.info.pid {
                        // Send signal to process group (negative PID)
                        unsafe {
                            kill(-(pid as i32), SIGKILL);
                        }
                    } else {
                        let _ = child.kill().await;
                    }
                } else {
                    // Send SIGTERM to process group
                    use libc::{SIGTERM, kill};
                    if let Some(pid) = self.info.pid {
                        unsafe {
                            kill(-(pid as i32), SIGTERM);
                        }
                    }
                }
            }

            // Wait for process to exit (with timeout)
            match tokio::time::timeout(std::time::Duration::from_secs(5), child.wait()).await {
                Ok(Ok(status)) => {
                    self.info.exit_code = status.code();
                    info!("Process {:?} exited with status: {:?}", pid, status.code());
                }
                Ok(Err(e)) => {
                    warn!("Failed to wait for process {:?}: {}", pid, e);
                }
                Err(_) => {
                    warn!(
                        "Timeout waiting for process {:?} to exit, forcing kill",
                        pid
                    );
                    // Force kill as fallback
                    #[cfg(windows)]
                    {
                        if let Some(pid) = pid {
                            let _ = std::process::Command::new("taskkill")
                                .args(["/F", "/T", "/PID"])
                                .arg(pid.to_string())
                                .output();
                        }
                    }
                    #[cfg(not(windows))]
                    {
                        let _ = child.kill().await;
                    }
                    // Try wait again
                    let _ =
                        tokio::time::timeout(std::time::Duration::from_secs(1), child.wait()).await;
                }
            }
        }

        self.child = None;
        self.info.state = ProcessState::Stopped;
        self.info.pid = None;

        Ok(())
    }

    /// Check if the process is still alive
    pub fn is_alive(&mut self) -> bool {
        // For static servers, check if static_server_id is set
        if self.static_server_id.is_some() {
            // Static server is running if it has an ID
            return true;
        }

        if let Some(ref mut child) = self.child {
            match child.try_wait() {
                Ok(Some(status)) => {
                    // Process has exited
                    self.info.exit_code = status.code();
                    self.info.state = ProcessState::Stopped;
                    // Don't clear pid here, keep it for display
                    // self.info.pid = None;
                    // self.child = None;
                    false
                }
                Ok(None) => true, // Still running
                Err(_) => false,
            }
        } else {
            false
        }
    }

    /// Update process stats
    pub fn update_stats(&mut self, stats: ProcessStats) {
        self.info.stats = stats;
    }

    /// Update uptime
    pub fn update_uptime(&mut self) {
        if let Some(start_time) = self.start_time {
            self.info.uptime_ms = start_time.elapsed().as_millis() as u64;
        }
    }

    /// Check if process has been running stable
    pub fn is_stable(&self) -> bool {
        rspm_common::utils::is_stable(self.info.started_at, STABLE_RUN_THRESHOLD_MS)
    }

    /// Calculate delay before restart
    pub fn calculate_restart_delay(&mut self) -> Option<std::time::Duration> {
        self.restart_policy.calculate_delay()
    }

    /// Reset restart counters
    pub fn reset_restart_counters(&mut self) {
        self.restart_policy.reset();
        self.info.restart_count = 0;
    }
}
