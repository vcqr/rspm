use rspm_common::{
    ProcessConfig, ProcessInfo, ProcessState, ProcessStats, Result, RspmError, ServerType,
};
use rspm_common::{ScheduleConfig, ScheduleExecution, ScheduleInfo, ScheduleStatus};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::{RwLock, broadcast};
use tokio::time::sleep;
use tracing::{error, info, warn};

use crate::log_watcher::LogWriter;
use crate::manager::managed_process::ManagedProcess;
use crate::manager::state_store::StateStore;
use crate::monitor::Monitor;
use crate::scheduler::ScheduleManager;
use crate::static_server::StaticServerManager;

/// Event sent when a process status changes
#[derive(Debug, Clone)]
pub enum ProcessEvent {
    Started {
        id: String,
        name: String,
    },
    Stopped {
        id: String,
        name: String,
        exit_code: Option<i32>,
    },
    Crashed {
        id: String,
        name: String,
        exit_code: Option<i32>,
    },
    Restarting {
        id: String,
        name: String,
        delay_ms: u64,
    },
    Errored {
        id: String,
        name: String,
        message: String,
    },
    StatsUpdated {
        id: String,
        stats: ProcessStats,
    },
}

/// Main process manager
pub struct ProcessManager {
    processes: Arc<RwLock<HashMap<String, ManagedProcess>>>,
    state_store: Arc<StateStore>,
    log_writer: Arc<LogWriter>,
    monitor: Arc<Monitor>,
    event_tx: broadcast::Sender<ProcessEvent>,
    start_time: Instant,
    schedule_manager: Arc<RwLock<Option<ScheduleManager>>>,
    static_server_manager: Arc<RwLock<StaticServerManager>>,
    shutdown_tx: tokio::sync::watch::Sender<bool>,
}

impl ProcessManager {
    pub async fn new(base_dir: PathBuf, db_path: &std::path::Path) -> Result<Self> {
        let log_dir = base_dir.join("logs");
        let state_store = Arc::new(StateStore::new(db_path).await?);
        let log_writer = Arc::new(LogWriter::new(log_dir));
        let monitor = Arc::new(Monitor::new());
        let (event_tx, _) = broadcast::channel(256);
        let static_server_manager = Arc::new(RwLock::new(StaticServerManager::new()));
        let (shutdown_tx, _) = tokio::sync::watch::channel(false);

        Ok(Self {
            processes: Arc::new(RwLock::new(HashMap::new())),
            state_store,
            log_writer,
            monitor,
            event_tx,
            start_time: Instant::now(),
            schedule_manager: Arc::new(RwLock::new(None)),
            static_server_manager,
            shutdown_tx,
        })
    }

    /// Initialize schedule manager
    pub async fn init_scheduler(&self) -> Result<()> {
        let schedule_manager = ScheduleManager::new(
            self.state_store.clone(),
            self.processes.clone(),
            self.log_writer.clone(),
        )
        .await
        .map_err(|e| RspmError::SchedulerError(e.to_string()))?;

        // Load and schedule all active schedules
        let schedules = self.state_store.get_all_schedules().await?;
        let schedule_count = schedules.len();
        for info in &schedules {
            if info.config.enabled
                && matches!(info.status, ScheduleStatus::Active)
                && let Err(e) = schedule_manager.schedule_job(info).await
            {
                warn!("Failed to schedule job '{}': {}", info.config.name, e);
            }
        }

        // Start the scheduler
        schedule_manager
            .start()
            .await
            .map_err(|e| RspmError::SchedulerError(e.to_string()))?;

        *self.schedule_manager.write().await = Some(schedule_manager);
        info!(
            "Schedule manager initialized with {} schedules",
            schedule_count
        );

        Ok(())
    }

    /// Subscribe to process events
    pub fn subscribe(&self) -> broadcast::Receiver<ProcessEvent> {
        self.event_tx.subscribe()
    }

    /// Initialize the manager (load saved state, start monitor, restore processes)
    pub async fn init(&self) -> Result<()> {
        let saved_processes = self.state_store.load().await?;

        // Restore processes from database
        for (name, config) in saved_processes {
            info!("Restoring process '{}' from database", name);

            // Create instances based on config
            let instances = config.instances.max(1);

            for instance_id in 0..instances {
                let instance_config = ProcessConfig {
                    instances: 1,
                    ..config.clone()
                };

                // Get or create database ID
                let db_id = self.state_store.get_id_by_name(&name).await?.unwrap_or(0); // Should not happen for saved processes

                // Create log writer for this instance
                let instance_log_writer = Some(Arc::new(self.log_writer.create_instance_writer(
                    &config.name,
                    instance_id,
                    config.log_file.as_deref(),
                    config.error_file.as_deref(),
                    config.log_max_size,
                    config.log_max_files,
                )));

                let mut managed_process =
                    ManagedProcess::new(Some(db_id), instance_config, instance_id);
                let display_id = if instances > 1 {
                    format!("{}-{}", db_id, instance_id)
                } else {
                    db_id.to_string()
                };
                managed_process.info.id = display_id.clone();

                // Start the process
                match managed_process.start(instance_log_writer).await {
                    Ok(_) => {
                        info!("Restored process '{}' (ID: {})", name, display_id);
                        let mut processes = self.processes.write().await;
                        processes.insert(display_id.clone(), managed_process);

                        let _ = self.event_tx.send(ProcessEvent::Started {
                            id: display_id,
                            name: name.clone(),
                        });
                    }
                    Err(e) => {
                        warn!("Failed to restore process '{}': {}", name, e);
                    }
                }
            }
        }

        self.start_event_loop();
        Ok(())
    }

    /// Start a new process
    pub async fn start_process(&self, config: ProcessConfig) -> Result<String> {
        use rspm_common::ServerType;

        let instances = config.instances.max(1);

        // Check if process with this name already exists and is running (only for single instance)
        // For multi-instance, we allow multiple processes with the same name
        if instances == 1 {
            let processes = self.processes.read().await;
            for (_, proc) in processes.iter() {
                if proc.config.name == config.name && proc.info.is_running() {
                    return Err(RspmError::ProcessAlreadyExists(config.name.clone()));
                }
            }
        }

        let mut created_ids = Vec::new();

        for instance_id in 0..instances {
            // Create a config for this instance
            let instance_config = ProcessConfig {
                instances: 1, // Each instance is independent
                ..config.clone()
            };

            // Save process configuration - use insert for multi-instance to get unique db_id
            let db_id = if instances > 1 {
                self.state_store.insert_process(&instance_config).await?
            } else {
                self.state_store.save_process(&instance_config).await?
            };

            // Use database ID as the display ID
            let display_id = db_id.to_string();

            // Create log writer for this instance
            let instance_log_writer = Some(Arc::new(self.log_writer.create_instance_writer(
                &config.name,
                instance_id,
                config.log_file.as_deref(),
                config.error_file.as_deref(),
                config.log_max_size,
                config.log_max_files,
            )));

            let mut managed_process =
                ManagedProcess::new(Some(db_id), instance_config.clone(), instance_id);
            managed_process.info.id = display_id.clone();

            // Check if this is a static server
            if instance_config.server_type == ServerType::StaticServer {
                // Extract parameters from args: [host, port, directory]
                let args = &instance_config.args;
                if args.len() >= 3 {
                    let host = args[0].clone();
                    let port: i32 = args[1].parse().unwrap_or(8080);
                    let directory = args[2].clone();

                    // Start static server using StaticServerManager
                    let static_manager = self.static_server_manager.write().await;
                    match static_manager
                        .start_server(instance_config.name.clone(), host, port, directory)
                        .await
                    {
                        Ok(static_id) => {
                            managed_process.static_server_id = Some(static_id);
                            managed_process.info.state = rspm_common::ProcessState::Running;
                            managed_process.info.started_at = Some(chrono::Utc::now());
                            info!(
                                "Started static file server '{}' with ID: {}",
                                instance_config.name, display_id
                            );
                        }
                        Err(e) => {
                            managed_process.info.state = rspm_common::ProcessState::Errored;
                            managed_process.info.error_message = Some(e.to_string());
                            return Err(RspmError::StartFailed(e.to_string()));
                        }
                    }
                } else {
                    return Err(RspmError::InvalidConfig(
                        "Static server requires 3 arguments: host, port, directory".to_string(),
                    ));
                }
            } else {
                // Normal process - start using ManagedProcess
                managed_process.start(instance_log_writer).await?;
            }

            let id_clone = display_id.clone();
            let name_clone = config.name.clone();
            {
                let mut processes = self.processes.write().await;
                processes.insert(display_id.clone(), managed_process);
            }

            let _ = self.event_tx.send(ProcessEvent::Started {
                id: id_clone,
                name: name_clone,
            });

            created_ids.push(display_id);
        }

        // Return the first instance ID
        Ok(created_ids.into_iter().next().unwrap())
    }

    /// Stop a process (supports both ID and name)
    pub async fn stop_process(&self, id: &str, force: bool) -> Result<()> {
        let mut processes = self.processes.write().await;

        // First try to find by exact ID
        // If not found, try to find by name
        let process_id = if processes.contains_key(id) {
            id.to_string()
        } else {
            // Find by name
            let matching: Vec<String> = processes
                .iter()
                .filter(|(_, p)| p.info.name == id)
                .map(|(k, _)| k.clone())
                .collect();

            if matching.is_empty() {
                return Err(RspmError::ProcessNotFound(id.to_string()));
            } else if matching.len() > 1 {
                return Err(RspmError::InvalidConfig(format!(
                    "Multiple processes found with name '{}': {:?}",
                    id, matching
                )));
            } else {
                matching.into_iter().next().unwrap()
            }
        };

        if let Some(proc) = processes.get_mut(&process_id) {
            // Check if this is a static server
            if proc.config.server_type == ServerType::StaticServer {
                if let Some(static_id) = &proc.static_server_id {
                    let static_manager = self.static_server_manager.write().await;
                    if let Err(e) = static_manager.stop_server(static_id).await {
                        warn!("Failed to stop static server {}: {}", static_id, e);
                    }
                }
                proc.info.state = rspm_common::ProcessState::Stopped;
                proc.info.pid = None;
            } else {
                // Normal process
                proc.stop(force).await?;
            }

            let name = proc.info.name.clone();
            let exit_code = proc.info.exit_code;

            let _ = self.event_tx.send(ProcessEvent::Stopped {
                id: process_id,
                name,
                exit_code,
            });
        } else {
            return Err(RspmError::ProcessNotFound(id.to_string()));
        }

        Ok(())
    }

    /// Stop all processes
    pub async fn stop_all(&self, _force: bool) -> Result<u32> {
        let mut processes = self.processes.write().await;
        let mut count = 0;

        for (_, proc) in processes.iter_mut() {
            if proc.info.is_running() {
                // Check if this is a static server
                if proc.config.server_type == ServerType::StaticServer {
                    if let Some(static_id) = &proc.static_server_id {
                        let static_manager = self.static_server_manager.write().await;
                        if let Err(e) = static_manager.stop_server(static_id).await {
                            warn!("Failed to stop static server {}: {}", static_id, e);
                        }
                    }
                    proc.info.state = ProcessState::Stopped;
                    proc.info.pid = None;
                } else {
                    // Normal process - always use force mode to ensure child processes are killed
                    proc.stop(true).await?;
                }
                count += 1;
            }
        }

        Ok(count)
    }

    /// Start a process by ID (for already configured processes)
    pub async fn start_process_by_id(&self, id: &str) -> Result<()> {
        // First try to find by exact ID, then by name
        let process_id = {
            let processes = self.processes.read().await;
            if processes.contains_key(id) {
                id.to_string()
            } else {
                // Find by name
                let matching: Vec<String> = processes
                    .iter()
                    .filter(|(_, p)| p.info.name == id)
                    .map(|(k, _)| k.clone())
                    .collect();

                if matching.is_empty() {
                    return Err(RspmError::ProcessNotFound(id.to_string()));
                } else if matching.len() > 1 {
                    return Err(RspmError::InvalidConfig(format!(
                        "Multiple processes found with name '{}': {:?}",
                        id, matching
                    )));
                } else {
                    matching.into_iter().next().unwrap()
                }
            }
        };

        let mut processes = self.processes.write().await;
        if let Some(proc) = processes.get_mut(&process_id) {
            if proc.info.is_running() {
                return Err(RspmError::InvalidConfig(
                    "Process is already running".to_string(),
                ));
            }
            proc.reset_restart_counters();

            // Check if this is a static server
            if proc.config.server_type == ServerType::StaticServer {
                let args = proc.config.args.clone();
                if args.len() >= 3 {
                    let host = args[0].clone();
                    let port: i32 = args[1].parse().unwrap_or(8080);
                    let directory = args[2].clone();

                    let static_manager = self.static_server_manager.write().await;
                    match static_manager
                        .start_server(proc.config.name.clone(), host, port, directory)
                        .await
                    {
                        Ok(static_id) => {
                            proc.static_server_id = Some(static_id);
                            proc.info.state = ProcessState::Running;
                            proc.info.started_at = Some(chrono::Utc::now());
                            info!(
                                "Restarted static file server '{}' with ID: {}",
                                proc.config.name, process_id
                            );
                        }
                        Err(e) => {
                            return Err(RspmError::StartFailed(e.to_string()));
                        }
                    }
                } else {
                    return Err(RspmError::InvalidConfig(
                        "Static server requires 3 arguments: host, port, directory".to_string(),
                    ));
                }
            } else {
                let log_writer = proc.log_writer.clone();
                proc.start(log_writer).await?;
            }
        } else {
            return Err(RspmError::ProcessNotFound(id.to_string()));
        }

        Ok(())
    }

    /// Restart a process (supports both ID and name)
    pub async fn restart_process(&self, id: &str) -> Result<()> {
        // First try to find by exact ID, then by name
        let process_id = {
            let processes = self.processes.read().await;
            if processes.contains_key(id) {
                id.to_string()
            } else {
                // Find by name
                let matching: Vec<String> = processes
                    .iter()
                    .filter(|(_, p)| p.info.name == id)
                    .map(|(k, _)| k.clone())
                    .collect();

                if matching.is_empty() {
                    return Err(RspmError::ProcessNotFound(id.to_string()));
                } else if matching.len() > 1 {
                    return Err(RspmError::InvalidConfig(format!(
                        "Multiple processes found with name '{}': {:?}",
                        id, matching
                    )));
                } else {
                    matching.into_iter().next().unwrap()
                }
            }
        };

        let _config = {
            let processes = self.processes.read().await;
            if let Some(proc) = processes.get(&process_id) {
                proc.config.clone()
            } else {
                return Err(RspmError::ProcessNotFound(id.to_string()));
            }
        };

        self.stop_process(&process_id, false).await?;
        sleep(Duration::from_millis(100)).await;

        // For static servers, use start_process_by_id which handles static server restart
        // For normal processes, start directly
        {
            let processes = self.processes.read().await;
            if let Some(proc) = processes.get(&process_id) {
                if proc.config.server_type == ServerType::StaticServer {
                    drop(processes); // Release lock before calling start_process_by_id
                    return self.start_process_by_id(&process_id).await;
                }
            } else {
                return Err(RspmError::ProcessNotFound(id.to_string()));
            }
        }

        {
            let mut processes = self.processes.write().await;
            if let Some(proc) = processes.get_mut(&process_id) {
                proc.reset_restart_counters();
                let log_writer = proc.log_writer.clone();
                proc.start(log_writer).await?;
            }
        }

        Ok(())
    }

    /// Update process configuration
    pub async fn update_process_config(&self, id: &str, new_config: ProcessConfig) -> Result<()> {
        // Find process by ID or name
        let process_id = {
            let processes = self.processes.read().await;
            if processes.contains_key(id) {
                id.to_string()
            } else {
                let matching: Vec<String> = processes
                    .iter()
                    .filter(|(_, p)| p.info.name == id)
                    .map(|(k, _)| k.clone())
                    .collect();

                if matching.is_empty() {
                    return Err(RspmError::ProcessNotFound(id.to_string()));
                } else if matching.len() > 1 {
                    return Err(RspmError::InvalidConfig(format!(
                        "Multiple processes found with name '{}'",
                        id
                    )));
                } else {
                    matching.into_iter().next().unwrap()
                }
            }
        };

        // Stop the process first
        self.stop_process(&process_id, false).await?;
        tokio::time::sleep(Duration::from_millis(100)).await;

        // Update configuration
        {
            let mut processes = self.processes.write().await;
            if let Some(proc) = processes.get_mut(&process_id) {
                proc.config = new_config.clone();
                proc.info.config = new_config.clone();
                proc.info.name = new_config.clone().name;
            } else {
                return Err(RspmError::ProcessNotFound(process_id));
            }
        }

        // Restart with new config
        self.start_process_by_id(&process_id).await?;

        info!("Updated process '{}' configuration", process_id);
        Ok(())
    }

    /// Delete a process (stop and remove, supports both ID and name)
    pub async fn delete_process(&self, id: &str) -> Result<()> {
        // First try to find by exact ID, then by name
        let process_id = {
            let processes = self.processes.read().await;
            if processes.contains_key(id) {
                id.to_string()
            } else {
                // Find by name
                let matching: Vec<String> = processes
                    .iter()
                    .filter(|(_, p)| p.info.name == id)
                    .map(|(k, _)| k.clone())
                    .collect();

                if matching.is_empty() {
                    return Err(RspmError::ProcessNotFound(id.to_string()));
                } else if matching.len() > 1 {
                    return Err(RspmError::InvalidConfig(format!(
                        "Multiple processes found with name '{}': {:?}",
                        id, matching
                    )));
                } else {
                    matching.into_iter().next().unwrap()
                }
            }
        };

        // Stop the process first (handles static servers correctly)
        {
            let processes = self.processes.read().await;
            if let Some(proc) = processes.get(&process_id) {
                if proc.info.is_running() {
                    drop(processes); // Release lock before calling stop_process
                    self.stop_process(&process_id, false).await?;
                }
            } else {
                return Err(RspmError::ProcessNotFound(id.to_string()));
            }
        }

        let name = {
            let mut processes = self.processes.write().await;
            if let Some(proc) = processes.remove(&process_id) {
                proc.config.name.clone()
            } else {
                return Err(RspmError::ProcessNotFound(id.to_string()));
            }
        };

        // Check if there are other instances with the same name
        let has_other_instances = {
            let processes = self.processes.read().await;
            processes.values().any(|p| p.config.name == name)
        };

        if !has_other_instances {
            self.state_store.remove_process(&name).await?;
        }

        Ok(())
    }

    /// List all processes
    pub async fn list_processes(&self) -> Vec<ProcessInfo> {
        let mut processes = self.processes.write().await;
        let mut result = Vec::new();

        for (_, proc) in processes.iter_mut() {
            // Update uptime and check if alive
            // For static servers, also check if static_server_id is set
            let is_static_running = proc.static_server_id.is_some();
            if proc.info.is_running() || is_static_running {
                proc.is_alive();
                proc.update_uptime();
            }
            result.push(proc.info.clone());
        }

        result
    }

    /// Get a specific process (supports both ID and name)
    pub async fn get_process(&self, id: &str) -> Option<ProcessInfo> {
        let mut processes = self.processes.write().await;

        // First try to find by exact ID
        if let Some(proc) = processes.get_mut(id) {
            if proc.info.is_running() {
                proc.is_alive();
                proc.update_uptime();
            }
            return Some(proc.info.clone());
        }

        // If not found, try to find by name
        let matching: Vec<String> = processes
            .iter()
            .filter(|(_, p)| p.info.name == id)
            .map(|(k, _)| k.clone())
            .collect();

        if matching.len() == 1 {
            let proc = processes.get_mut(&matching[0]).unwrap();
            if proc.info.is_running() {
                proc.is_alive();
                proc.update_uptime();
            }
            return Some(proc.info.clone());
        }

        None
    }

    /// Get process names by name prefix
    pub async fn get_processes_by_name(&self, name: &str) -> Vec<String> {
        let processes = self.processes.read().await;
        processes
            .values()
            .filter(|p| p.config.name == name || p.info.id.starts_with(name))
            .map(|p| p.info.id.clone())
            .collect()
    }

    /// Scale a process to a new number of instances (supports both ID and name)
    pub async fn scale_process(&self, id: &str, instances: u32) -> Result<Vec<String>> {
        // First try to find by exact ID, then by name
        let process_id = {
            let processes = self.processes.read().await;
            if processes.contains_key(id) {
                id.to_string()
            } else {
                // Find by name
                let matching: Vec<String> = processes
                    .iter()
                    .filter(|(_, p)| p.info.name == id)
                    .map(|(k, _)| k.clone())
                    .collect();

                if matching.is_empty() {
                    return Err(RspmError::ProcessNotFound(id.to_string()));
                } else if matching.len() > 1 {
                    return Err(RspmError::InvalidConfig(format!(
                        "Multiple processes found with name '{}': {:?}",
                        id, matching
                    )));
                } else {
                    matching.into_iter().next().unwrap()
                }
            }
        };

        let base_config = {
            let processes = self.processes.read().await;
            if let Some(proc) = processes.get(&process_id) {
                proc.config.clone()
            } else {
                return Err(RspmError::ProcessNotFound(id.to_string()));
            }
        };

        let current_instances = self.get_processes_by_name(&base_config.name).await.len() as u32;

        if instances > current_instances {
            // Scale up - create new independent instances
            let mut new_ids = Vec::new();
            for _ in current_instances..instances {
                // Create a new independent config
                let new_config = ProcessConfig {
                    instances: 1,
                    ..base_config.clone()
                };

                // Save to get independent db_id
                let db_id = self.state_store.save_process(&new_config).await?;
                let display_id = db_id.to_string();

                let mut managed_process = ManagedProcess::new(Some(db_id), new_config, 0);
                managed_process.info.id = display_id.clone();

                let log_writer = Arc::new(self.log_writer.create_instance_writer(
                    &base_config.name,
                    0,
                    base_config.log_file.as_deref(),
                    base_config.error_file.as_deref(),
                    base_config.log_max_size,
                    base_config.log_max_files,
                ));
                managed_process.start(Some(log_writer)).await?;

                let process_id = managed_process.info.id.clone();
                let name = managed_process.info.name.clone();

                {
                    let mut processes = self.processes.write().await;
                    processes.insert(process_id.clone(), managed_process);
                }

                let _ = self.event_tx.send(ProcessEvent::Started {
                    id: process_id.clone(),
                    name,
                });
                new_ids.push(process_id);
            }
            Ok(new_ids)
        } else if instances < current_instances {
            // Scale down
            let all_ids = self.get_processes_by_name(&base_config.name).await;
            for id in all_ids.iter().skip(instances as usize) {
                self.stop_process(id, false).await?;
                {
                    let mut processes = self.processes.write().await;
                    processes.remove(id);
                }
            }
            Ok(Vec::new())
        } else {
            Ok(Vec::new())
        }
    }

    /// Start the event loop for monitoring and auto-restart
    fn start_event_loop(&self) {
        let processes = self.processes.clone();
        let event_tx = self.event_tx.clone();
        let monitor = self.monitor.clone();
        let mut shutdown_rx = self.shutdown_tx.subscribe();

        tokio::spawn(async move {
            let mut interval = tokio::time::interval(Duration::from_millis(500));

            loop {
                tokio::select! {
                    _ = shutdown_rx.changed() => {
                        if *shutdown_rx.borrow() {
                            info!("Event loop received shutdown signal, stopping...");
                            break;
                        }
                    }
                    _ = interval.tick() => {
                        let mut to_restart = Vec::new();
                        let mut to_mark_errored = Vec::new();

                        {
                            let mut procs = processes.write().await;

                            for (id, proc) in procs.iter_mut() {
                                // Check if process is still alive
                                let was_running = proc.info.is_running();
                                let is_alive = proc.is_alive();

                                if was_running && !is_alive {
                                    // Process just died
                                    let name = proc.info.name.clone();
                                    let exit_code = proc.info.exit_code;

                                    info!("Process {} ({}) died with exit code {:?}", id, name, exit_code);

                                    if proc.config.autorestart {
                                        // Try to restart regardless of stability
                                        if let Some(delay) = proc.calculate_restart_delay() {
                                            let _ = event_tx.send(ProcessEvent::Restarting {
                                                id: id.clone(),
                                                name: name.clone(),
                                                delay_ms: delay.as_millis() as u64,
                                            });
                                            to_restart.push((id.clone(), delay));
                                        } else {
                                            // Max restarts exceeded
                                            let _ = event_tx.send(ProcessEvent::Errored {
                                                id: id.clone(),
                                                name,
                                                message: "Max restarts exceeded".to_string(),
                                            });
                                            to_mark_errored.push(id.clone());
                                        }
                                    } else {
                                        let _ = event_tx.send(ProcessEvent::Crashed {
                                            id: id.clone(),
                                            name,
                                            exit_code,
                                        });
                                    }
                                } else if is_alive {
                                    // Update uptime
                                    proc.update_uptime();

                                    // Update stats
                                    if let Some(pid) = proc.info.pid
                                        && let Some(stats) = monitor.get_process_stats(pid as i32) {
                                            proc.update_stats(stats.clone());

                                            // Check memory limit
                                            if proc.config.max_memory_mb > 0 {
                                                let max_bytes = proc.config.max_memory_mb as u64 * 1024 * 1024;
                                                if stats.memory_bytes > max_bytes {
                                                    warn!(
                                                        "Process {} exceeded memory limit ({}MB > {}MB)",
                                                        id,
                                                        stats.memory_bytes / 1024 / 1024,
                                                        proc.config.max_memory_mb
                                                    );
                                                    // Will be handled in restart logic
                                                }
                                            }

                                            let _ = event_tx.send(ProcessEvent::StatsUpdated {
                                                id: id.clone(),
                                                stats,
                                            });
                                        }

                                    // Reset restart counters if stable
                                    if proc.is_stable() && proc.restart_policy.restart_count > 0 {
                                        proc.reset_restart_counters();
                                    }
                                }
                            }
                        }

                        // Mark errored processes
                        {
                            let mut procs = processes.write().await;
                            for id in to_mark_errored {
                                if let Some(proc) = procs.get_mut(&id) {
                                    proc.info.state = ProcessState::Errored;
                                    proc.info.error_message = Some("Max restarts exceeded".to_string());
                                }
                            }
                        }

                        // Restart processes with delay
                        for (id, delay) in to_restart {
                            sleep(delay).await;

                            let mut procs = processes.write().await;
                            if let Some(proc) = procs.get_mut(&id) {
                                // Only restart if process is Stopped, not if it's Errored
                                // Errored processes should be manually restarted by user
                                if proc.info.state == ProcessState::Stopped {
                                    let log_writer = proc.log_writer.clone();
                                    match proc.start(log_writer).await {
                                        Ok(_) => {
                                            proc.info.restart_count += 1;
                                            info!("Restarted process {} (restart count: {})", id, proc.info.restart_count);
                                            let _ = event_tx.send(ProcessEvent::Started {
                                                id: id.clone(),
                                                name: proc.info.name.clone(),
                                            });
                                        }
                                        Err(e) => {
                                            error!("Failed to restart process {}: {}", id, e);
                                        }
                                    }
                                } else if proc.info.state == ProcessState::Errored {
                                    info!("Process {} is in ERRORED state, skipping auto-restart. Manual restart required.", id);
                                }
                            }
                        }
                    }
                }
            }
        });
    }

    /// Shutdown the process manager (stop all processes and scheduler)
    pub async fn shutdown(&self) {
        info!("Shutting down process manager...");

        // Stop all processes
        if let Err(e) = self.stop_all(true).await {
            error!("Error stopping processes during shutdown: {}", e);
        }

        // Stop scheduler
        let mut schedule_manager = self.schedule_manager.write().await;
        if let Some(sm) = schedule_manager.take()
            && let Err(e) = sm.stop().await
        {
            error!("Error stopping scheduler during shutdown: {}", e);
        }

        // Signal event loop to stop
        let _ = self.shutdown_tx.send(true);

        info!("Process manager shutdown complete");
    }

    /// Get daemon status
    pub async fn get_status(&self) -> (u32, u64) {
        let processes = self.processes.read().await;
        let total = processes.len() as u32;
        let uptime = self.start_time.elapsed().as_millis() as u64;
        (total, uptime)
    }

    /// Subscribe to log entries for a specific process (supports both ID and name)
    pub async fn subscribe_logs(
        &self,
        id: &str,
    ) -> Result<broadcast::Receiver<rspm_common::LogEntry>> {
        let processes = self.processes.read().await;

        // First try to find by exact ID
        if let Some(proc) = processes.get(id) {
            if let Some(ref log_writer) = proc.log_writer {
                return Ok(log_writer.subscribe());
            } else {
                return Err(RspmError::LogError("Process has no log writer".to_string()));
            }
        }

        // If not found, try to find by name
        let matching: Vec<_> = processes
            .iter()
            .filter(|(_, p)| p.info.name == id)
            .map(|(k, p)| (k.clone(), p.log_writer.clone()))
            .collect();

        if matching.is_empty() {
            Err(RspmError::ProcessNotFound(id.to_string()))
        } else if matching.len() > 1 {
            Err(RspmError::InvalidConfig(format!(
                "Multiple processes found with name '{}', use ID to specify one",
                id
            )))
        } else if let Some(ref log_writer) = matching[0].1 {
            Ok(log_writer.subscribe())
        } else {
            Err(RspmError::LogError("Process has no log writer".to_string()))
        }
    }

    /// Read log history for a specific process (supports both ID and name)
    pub async fn read_log_history(
        &self,
        id: &str,
        lines: usize,
        include_stderr: bool,
    ) -> Result<Vec<rspm_common::LogEntry>> {
        let processes = self.processes.read().await;

        // First try to find by exact ID
        let log_writer = if let Some(proc) = processes.get(id) {
            proc.log_writer.clone()
        } else {
            // If not found, try to find by name
            let matching: Vec<_> = processes
                .iter()
                .filter(|(_, p)| p.info.name == id)
                .map(|(k, p)| (k.clone(), p.log_writer.clone()))
                .collect();

            if matching.is_empty() {
                return Err(RspmError::ProcessNotFound(id.to_string()));
            } else if matching.len() > 1 {
                return Err(RspmError::InvalidConfig(format!(
                    "Multiple processes found with name '{}', use ID to specify one",
                    id
                )));
            } else {
                matching[0].1.clone()
            }
        };

        drop(processes);

        if let Some(ref writer) = log_writer {
            let mut entries = writer.read_stdout_history(lines)?;
            if include_stderr {
                let stderr_entries = writer.read_stderr_history(lines)?;
                entries.extend(stderr_entries);
                // Sort by timestamp
                entries.sort_by(|a, b| a.timestamp.cmp(&b.timestamp));
            }
            Ok(entries)
        } else {
            Err(RspmError::LogError("Process has no log writer".to_string()))
        }
    }

    // ==================== Schedule Management Methods ====================

    /// Create a new schedule
    pub async fn create_schedule(&self, config: ScheduleConfig) -> Result<String> {
        let id = uuid::Uuid::new_v4().to_string();

        let info = ScheduleInfo {
            id: id.clone(),
            config: config.clone(),
            status: if config.enabled {
                ScheduleStatus::Active
            } else {
                ScheduleStatus::Paused
            },
            created_at: chrono::Utc::now(),
            updated_at: chrono::Utc::now(),
            last_run: None,
            next_run: config.next_run(chrono::Utc::now()),
            run_count: 0,
            success_count: 0,
            fail_count: 0,
        };

        // Save to database
        self.state_store.save_schedule(&info).await?;

        // Schedule the job if enabled
        if config.enabled
            && let Some(ref manager) = *self.schedule_manager.read().await
        {
            manager
                .schedule_job(&info)
                .await
                .map_err(|e| RspmError::SchedulerError(e.to_string()))?;
        }

        info!("Created schedule '{}' ({})", config.name, id);
        Ok(id)
    }

    /// Update a schedule
    pub async fn update_schedule(&self, id: &str, config: ScheduleConfig) -> Result<()> {
        // Get existing schedule
        let mut info = self
            .state_store
            .get_schedule(id)
            .await?
            .ok_or_else(|| RspmError::NotFound(format!("Schedule '{}' not found", id)))?;

        // Cancel existing job
        if let Some(ref manager) = *self.schedule_manager.read().await {
            manager
                .cancel_job(id)
                .await
                .map_err(|e| RspmError::SchedulerError(e.to_string()))?;
        }

        // Update config
        info.config = config;
        info.updated_at = chrono::Utc::now();
        info.next_run = info.config.next_run(chrono::Utc::now());

        // Save to database
        self.state_store.save_schedule(&info).await?;

        // Reschedule if enabled
        if info.config.enabled
            && matches!(info.status, ScheduleStatus::Active)
            && let Some(ref manager) = *self.schedule_manager.read().await
        {
            manager
                .schedule_job(&info)
                .await
                .map_err(|e| RspmError::SchedulerError(e.to_string()))?;
        }

        info!("Updated schedule '{}' ({})", info.config.name, id);
        Ok(())
    }

    /// Delete a schedule
    pub async fn delete_schedule(&self, id: &str) -> Result<()> {
        // Get schedule info for logging
        let info = self.state_store.get_schedule(id).await?;

        // Cancel job
        if let Some(ref manager) = *self.schedule_manager.read().await {
            manager
                .cancel_job(id)
                .await
                .map_err(|e| RspmError::SchedulerError(e.to_string()))?;
        }

        // Remove from database
        self.state_store.remove_schedule(id).await?;

        if let Some(info) = info {
            info!("Deleted schedule '{}' ({})", info.config.name, id);
        }

        Ok(())
    }

    /// Get a schedule by ID
    pub async fn get_schedule(&self, id: &str) -> Result<Option<ScheduleInfo>> {
        self.state_store.get_schedule(id).await
    }

    /// Get a schedule by name
    pub async fn get_schedule_by_name(&self, name: &str) -> Result<Option<ScheduleInfo>> {
        self.state_store.get_schedule_by_name(name).await
    }

    /// List all schedules
    pub async fn list_schedules(&self) -> Result<Vec<ScheduleInfo>> {
        self.state_store.get_all_schedules().await
    }

    /// Pause a schedule
    pub async fn pause_schedule(&self, id: &str) -> Result<()> {
        // Get existing schedule
        let mut info = self
            .state_store
            .get_schedule(id)
            .await?
            .ok_or_else(|| RspmError::NotFound(format!("Schedule '{}' not found", id)))?;

        // Cancel job
        if let Some(ref manager) = *self.schedule_manager.read().await {
            manager
                .cancel_job(id)
                .await
                .map_err(|e| RspmError::SchedulerError(e.to_string()))?;
        }

        // Update status
        info.status = ScheduleStatus::Paused;
        info.updated_at = chrono::Utc::now();
        self.state_store.save_schedule(&info).await?;

        info!("Paused schedule '{}' ({})", info.config.name, id);
        Ok(())
    }

    /// Resume a schedule
    pub async fn resume_schedule(&self, id: &str) -> Result<()> {
        // Get existing schedule
        let mut info = self
            .state_store
            .get_schedule(id)
            .await?
            .ok_or_else(|| RspmError::NotFound(format!("Schedule '{}' not found", id)))?;

        // Update status
        info.status = ScheduleStatus::Active;
        info.updated_at = chrono::Utc::now();
        info.next_run = info.config.next_run(chrono::Utc::now());
        self.state_store.save_schedule(&info).await?;

        // Schedule job
        if info.config.enabled
            && let Some(ref manager) = *self.schedule_manager.read().await
        {
            manager
                .schedule_job(&info)
                .await
                .map_err(|e| RspmError::SchedulerError(e.to_string()))?;
        }

        info!("Resumed schedule '{}' ({})", info.config.name, id);
        Ok(())
    }

    /// Get execution history for a schedule
    pub async fn get_schedule_executions(
        &self,
        id: &str,
        limit: u32,
    ) -> Result<Vec<ScheduleExecution>> {
        self.state_store.get_executions(id, limit as i64).await
    }

    // ==================== Static Server Methods ====================

    /// Start a static file server
    pub async fn start_static_server(
        &self,
        name: String,
        host: String,
        port: i32,
        directory: String,
    ) -> Result<String> {
        let manager = self.static_server_manager.write().await;
        manager.start_server(name, host, port, directory).await
    }

    /// Stop a static file server
    pub async fn stop_static_server(&self, id: &str) -> Result<()> {
        let manager = self.static_server_manager.write().await;
        manager.stop_server(id).await
    }

    /// List all static file servers
    pub async fn list_static_servers(&self) -> Vec<rspm_common::StaticServerInfo> {
        let manager = self.static_server_manager.read().await;
        manager.list_servers().await
    }

    /// Get a specific static file server
    pub async fn get_static_server(&self, id: &str) -> Option<rspm_common::StaticServerInfo> {
        let manager = self.static_server_manager.read().await;
        manager.get_server(id).await
    }
}
