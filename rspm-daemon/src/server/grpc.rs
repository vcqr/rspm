use rspm_common::{ProcessConfig, ProcessInfo, ProcessState, RspmError};
use rspm_proto::{
    process_manager_server::{ProcessManager as GrpcProcessManager, ProcessManagerServer},
};
use std::collections::HashMap;
use std::sync::Arc;
use std::pin::Pin;
use tokio::sync::{RwLock, watch};
use tokio_stream::{wrappers::ReceiverStream, Stream};
use tonic::{transport::Server, Request, Response, Status};
use tracing::info;

use crate::manager::ProcessManager;
use crate::log_watcher::LogWriter;

// Type aliases for proto types to avoid name collision
type ProtoProcessConfig = rspm_proto::ProcessConfig;
type ProtoProcessInfo = rspm_proto::ProcessInfo;
type ProtoDaemonStatus = rspm_proto::DaemonStatus;
type ProtoLogEntry = rspm_proto::LogEntry;

/// gRPC server for the daemon
pub struct RpcServer {
    process_manager: Arc<ProcessManager>,
    #[allow(dead_code)]
    log_writers: Arc<RwLock<HashMap<String, Arc<LogWriter>>>>,
    #[allow(dead_code)]
    start_time: std::time::Instant,
    shutdown_tx: watch::Sender<bool>,
    token: Option<String>,
    is_remote: bool,
}

impl RpcServer {
    pub fn new(process_manager: Arc<ProcessManager>) -> Self {
        let (shutdown_tx, _) = watch::channel(false);
        let config = rspm_common::DaemonConfig::load_default();
        let is_remote = config.host != "127.0.0.1" && config.host != "localhost" && !config.host.is_empty();
        Self {
            process_manager,
            log_writers: Arc::new(RwLock::new(HashMap::new())),
            start_time: std::time::Instant::now(),
            shutdown_tx,
            token: config.token,
            is_remote,
        }
    }

    /// Verify token from request metadata
    fn verify_token<T>(&self, request: &Request<T>) -> Result<(), Status> {
        // For local connections, skip token verification
        if !self.is_remote {
            return Ok(());
        }

        // For remote connections, token is required
        let metadata = request.metadata();
        let key = tonic::metadata::MetadataKey::from_static("x-rspm-token");
        match metadata.get(&key) {
            Some(value) => {
                let received = value.to_str().map_err(|_| Status::unauthenticated("Invalid token"))?;
                if received == self.token.as_ref().unwrap() {
                    Ok(())
                } else {
                    Err(Status::unauthenticated("Invalid token"))
                }
            }
            None => Err(Status::unauthenticated("Missing token")),
        }
    }

    /// Get a shutdown receiver to monitor for shutdown signal
    pub fn shutdown_receiver(&self) -> watch::Receiver<bool> {
        self.shutdown_tx.subscribe()
    }

    /// Start the gRPC server on a Unix socket
    #[cfg(unix)]
    pub async fn serve_unix(self, socket_path: &str) -> std::result::Result<(), RspmError> {
        use tokio::net::UnixListener;
        use std::os::unix::fs::PermissionsExt;

        // Remove existing socket
        let _ = std::fs::remove_file(socket_path);

        let listener = UnixListener::bind(socket_path)
            .map_err(RspmError::IoError)?;

        // Set socket permissions
        std::fs::set_permissions(socket_path, std::fs::Permissions::from_mode(0o777))
            .map_err(RspmError::IoError)?;

        info!("Daemon listening on {}", socket_path);

        let mut shutdown_rx = self.shutdown_tx.subscribe();

        Server::builder()
            .add_service(ProcessManagerServer::new(self))
            .serve_with_incoming_shutdown(
                tokio_stream::wrappers::UnixListenerStream::new(listener),
                async move {
                    let _ = shutdown_rx.changed().await;
                    info!("Shutdown signal received, stopping gRPC server...");
                }
            )
            .await
            .map_err(|e| RspmError::GrpcError(e.to_string()))?;

        Ok(())
    }

    /// Start the gRPC server on TCP
    pub async fn serve_tcp(self, addr: &str) -> std::result::Result<(), RspmError> {
        let addr: std::net::SocketAddr = addr.parse()
            .map_err(|e| RspmError::InvalidConfig(format!("Invalid address: {}", e)))?;

        info!("Daemon listening on {}", addr);

        let mut shutdown_rx = self.shutdown_tx.subscribe();

        Server::builder()
            .add_service(ProcessManagerServer::new(self))
            .serve_with_shutdown(addr, async move {
                let _ = shutdown_rx.changed().await;
                info!("Shutdown signal received, stopping gRPC server...");
            })
            .await
            .map_err(|e| RspmError::GrpcError(e.to_string()))?;

        Ok(())
    }

    /// Trigger shutdown of the server
    fn trigger_shutdown(&self) {
        let _ = self.shutdown_tx.send(true);
    }
}

// Convert ProcessInfo to proto ProcessInfo
fn to_proto_process_info(info: &ProcessInfo) -> ProtoProcessInfo {
    ProtoProcessInfo {
        db_id: info.db_id.unwrap_or(0),
        id: info.id.clone(),
        name: info.name.clone(),
        state: match info.state {
            ProcessState::Stopped => 0,
            ProcessState::Starting => 1,
            ProcessState::Running => 2,
            ProcessState::Stopping => 3,
            ProcessState::Errored => 4,
        },
        pid: info.pid.unwrap_or(0) as i32,
        instance_id: info.instance_id as i32,
        config: Some(to_proto_process_config(&info.config)),
        uptime_ms: info.uptime_ms as i64,
        restart_count: info.restart_count as i32,
        cpu_percent: info.stats.cpu_percent,
        memory_bytes: info.stats.memory_bytes as i64,
        created_at: info.created_at.timestamp_millis(),
        started_at: info.started_at.map(|t| t.timestamp_millis()).unwrap_or(0),
    }
}

// Convert ProcessConfig to proto ProcessConfig
fn to_proto_process_config(config: &ProcessConfig) -> ProtoProcessConfig {
    ProtoProcessConfig {
        name: config.name.clone(),
        command: config.command.clone(),
        args: config.args.clone(),
        env: config.env.clone(),
        cwd: config.cwd.clone().unwrap_or_default(),
        instances: config.instances as i32,
        autorestart: config.autorestart,
        max_restarts: config.max_restarts as i32,
        max_memory_mb: config.max_memory_mb as i32,
        watch: config.watch,
        watch_paths: config.watch_paths.clone(),
        log_file: config.log_file.clone().unwrap_or_default(),
        error_file: config.error_file.clone().unwrap_or_default(),
        log_max_size: config.log_max_size as i64,
        log_max_files: config.log_max_files as i32,
        server_type: match config.server_type {
            rspm_common::ServerType::StaticServer => 1,
            _ => 0,
        },
    }
}

// Convert proto ProcessConfig to ProcessConfig
fn from_proto_process_config(proto: &ProtoProcessConfig) -> ProcessConfig {
    use rspm_common::ServerType;

    let mut env = HashMap::new();
    for (k, v) in &proto.env {
        env.insert(k.clone(), v.clone());
    }

    ProcessConfig {
        name: proto.name.clone(),
        command: proto.command.clone(),
        args: proto.args.clone(),
        env,
        cwd: if proto.cwd.is_empty() { None } else { Some(proto.cwd.clone()) },
        instances: proto.instances as u32,
        autorestart: proto.autorestart,
        max_restarts: proto.max_restarts as u32,
        max_memory_mb: proto.max_memory_mb as u32,
        watch: proto.watch,
        watch_paths: proto.watch_paths.clone(),
        log_file: if proto.log_file.is_empty() { None } else { Some(proto.log_file.clone()) },
        error_file: if proto.error_file.is_empty() { None } else { Some(proto.error_file.clone()) },
        log_max_size: proto.log_max_size as u64,
        log_max_files: proto.log_max_files as u32,
        server_type: match proto.server_type {
            1 => ServerType::StaticServer,
            _ => ServerType::Process,
        },
    }
}

// Schedule conversion functions
fn from_proto_schedule_config(proto: &rspm_proto::ScheduleConfig) -> rspm_common::ScheduleConfig {
    use rspm_common::{ScheduleType, ScheduleAction};

    let schedule = match proto.schedule_type {
        0 => ScheduleType::Cron(proto.schedule_value.clone()),
        1 => ScheduleType::Interval(proto.schedule_value.parse().unwrap_or(60)),
        2 => ScheduleType::Once(
            chrono::DateTime::parse_from_rfc3339(&proto.schedule_value)
                .map(|dt| dt.with_timezone(&chrono::Utc))
                .unwrap_or_else(|_| chrono::Utc::now())
        ),
        _ => ScheduleType::Interval(60),
    };

    let action = match proto.action {
        0 => ScheduleAction::Start,
        1 => ScheduleAction::Stop,
        2 => ScheduleAction::Restart,
        3 => ScheduleAction::Execute {
            command: proto.action_command.clone(),
            args: proto.action_args.clone(),
        },
        _ => ScheduleAction::Start,
    };

    rspm_common::ScheduleConfig {
        id: if proto.id.is_empty() { None } else { Some(proto.id.clone()) },
        name: proto.name.clone(),
        process_name: if proto.process_name.is_empty() { None } else { Some(proto.process_name.clone()) },
        schedule,
        action,
        enabled: proto.enabled,
        timezone: proto.timezone.clone(),
        max_runs: proto.max_runs as u32,
        description: if proto.description.is_empty() { None } else { Some(proto.description.clone()) },
    }
}

fn to_proto_schedule_info(info: &rspm_common::ScheduleInfo) -> rspm_proto::ScheduleInfo {
    use rspm_common::{ScheduleType, ScheduleAction, ScheduleStatus};

    let (schedule_type, schedule_value) = match &info.config.schedule {
        ScheduleType::Cron(expr) => (0, expr.clone()),
        ScheduleType::Interval(secs) => (1, secs.to_string()),
        ScheduleType::Once(dt) => (2, dt.to_rfc3339()),
    };

    let action = match &info.config.action {
        ScheduleAction::Start => 0,
        ScheduleAction::Stop => 1,
        ScheduleAction::Restart => 2,
        ScheduleAction::Execute { .. } => 3,
    };

    let status = match &info.status {
        ScheduleStatus::Active => 0,
        ScheduleStatus::Paused => 1,
        ScheduleStatus::Completed => 2,
        ScheduleStatus::Error(_) => 3,
    };

    rspm_proto::ScheduleInfo {
        id: info.id.clone(),
        config: Some(rspm_proto::ScheduleConfig {
            id: info.config.id.clone().unwrap_or_default(),
            name: info.config.name.clone(),
            process_name: info.config.process_name.clone().unwrap_or_default(),
            schedule_type,
            schedule_value,
            action,
            action_command: match &info.config.action {
                ScheduleAction::Execute { command, .. } => command.clone(),
                _ => String::new(),
            },
            action_args: match &info.config.action {
                ScheduleAction::Execute { args, .. } => args.clone(),
                _ => vec![],
            },
            enabled: info.config.enabled,
            timezone: info.config.timezone.clone(),
            max_runs: info.config.max_runs as i32,
            description: info.config.description.clone().unwrap_or_default(),
        }),
        status,
        created_at: info.created_at.timestamp_millis(),
        updated_at: info.updated_at.timestamp_millis(),
        last_run: info.last_run.map(|dt| dt.timestamp_millis()).unwrap_or(0),
        next_run: info.next_run.map(|dt| dt.timestamp_millis()).unwrap_or(0),
        run_count: info.run_count as i32,
        success_count: info.success_count as i32,
        fail_count: info.fail_count as i32,
    }
}

fn to_proto_execution(exec: &rspm_common::ScheduleExecution) -> rspm_proto::ScheduleExecution {
    rspm_proto::ScheduleExecution {
        id: exec.id.clone(),
        schedule_id: exec.schedule_id.clone(),
        started_at: exec.started_at.timestamp_millis(),
        ended_at: exec.ended_at.map(|dt| dt.timestamp_millis()).unwrap_or(0),
        status: match exec.status {
            rspm_common::ExecutionStatus::Running => "running".to_string(),
            rspm_common::ExecutionStatus::Success => "success".to_string(),
            rspm_common::ExecutionStatus::Failed => "failed".to_string(),
            rspm_common::ExecutionStatus::Timeout => "timeout".to_string(),
        },
        output: exec.output.clone().unwrap_or_default(),
        error: exec.error.clone().unwrap_or_default(),
    }
}

#[tonic::async_trait]
impl GrpcProcessManager for RpcServer {
    async fn start_process(
        &self,
        request: Request<rspm_proto::StartProcessRequest>,
    ) -> std::result::Result<Response<rspm_proto::StartProcessResponse>, Status> {
        self.verify_token(&request)?;
        let proto_config = request.into_inner()
            .config
            .ok_or_else(|| Status::invalid_argument("Missing config"))?;

        let config = from_proto_process_config(&proto_config);

        match self.process_manager.start_process(config).await {
            Ok(id) => Ok(Response::new(rspm_proto::StartProcessResponse {
                id,
                success: true,
                message: "Process started successfully".to_string(),
            })),
            Err(e) => Ok(Response::new(rspm_proto::StartProcessResponse {
                id: String::new(),
                success: false,
                message: e.to_string(),
            })),
        }
    }

    async fn stop_process(
        &self,
        request: Request<rspm_proto::StopProcessRequest>,
    ) -> std::result::Result<Response<rspm_proto::StopProcessResponse>, Status> {
        self.verify_token(&request)?;
        let req = request.into_inner();

        match self.process_manager.stop_process(&req.id, req.force).await {
            Ok(()) => Ok(Response::new(rspm_proto::StopProcessResponse {
                success: true,
                message: "Process stopped".to_string(),
            })),
            Err(e) => Ok(Response::new(rspm_proto::StopProcessResponse {
                success: false,
                message: e.to_string(),
            })),
        }
    }

    async fn restart_process(
        &self,
        request: Request<rspm_proto::RestartProcessRequest>,
    ) -> std::result::Result<Response<rspm_proto::RestartProcessResponse>, Status> {
        self.verify_token(&request)?;
        let req = request.into_inner();

        match self.process_manager.restart_process(&req.id).await {
            Ok(()) => Ok(Response::new(rspm_proto::RestartProcessResponse {
                success: true,
                message: "Process restarted".to_string(),
            })),
            Err(e) => Ok(Response::new(rspm_proto::RestartProcessResponse {
                success: false,
                message: e.to_string(),
            })),
        }
    }

    async fn delete_process(
        &self,
        request: Request<rspm_proto::DeleteProcessRequest>,
    ) -> std::result::Result<Response<rspm_proto::DeleteProcessResponse>, Status> {
        self.verify_token(&request)?;
        let req = request.into_inner();

        match self.process_manager.delete_process(&req.id).await {
            Ok(()) => Ok(Response::new(rspm_proto::DeleteProcessResponse {
                success: true,
                message: "Process deleted".to_string(),
            })),
            Err(e) => Ok(Response::new(rspm_proto::DeleteProcessResponse {
                success: false,
                message: e.to_string(),
            })),
        }
    }

    async fn stop_all_processes(
        &self,
        request: Request<rspm_proto::StopAllProcessesRequest>,
    ) -> std::result::Result<Response<rspm_proto::StopAllProcessesResponse>, Status> {
        self.verify_token(&request)?;
        match self.process_manager.stop_all(false).await {
            Ok(count) => Ok(Response::new(rspm_proto::StopAllProcessesResponse {
                stopped_count: count as i32,
                message: format!("Stopped {} processes", count),
            })),
            Err(e) => Ok(Response::new(rspm_proto::StopAllProcessesResponse {
                stopped_count: 0,
                message: e.to_string(),
            })),
        }
    }

    async fn list_processes(
        &self,
        request: Request<rspm_proto::ListProcessesRequest>,
    ) -> std::result::Result<Response<rspm_proto::ListProcessesResponse>, Status> {
        self.verify_token(&request)?;
        let req = request.into_inner();
        let processes = self.process_manager.list_processes().await;

        let filtered: Vec<ProtoProcessInfo> = if req.name_filter.is_empty() {
            processes.iter().map(|p| to_proto_process_info(p)).collect()
        } else {
            processes
                .iter()
                .filter(|p| p.name.contains(&req.name_filter) || p.id.contains(&req.name_filter))
                .map(|p| to_proto_process_info(p))
                .collect()
        };

        Ok(Response::new(rspm_proto::ListProcessesResponse {
            processes: filtered,
        }))
    }

    async fn get_process(
        &self,
        request: Request<rspm_proto::GetProcessRequest>,
    ) -> std::result::Result<Response<rspm_proto::GetProcessResponse>, Status> {
        self.verify_token(&request)?;
        let req = request.into_inner();

        match self.process_manager.get_process(&req.id).await {
            Some(info) => Ok(Response::new(rspm_proto::GetProcessResponse {
                process: Some(to_proto_process_info(&info)),
            })),
            None => Err(Status::not_found("Process not found")),
        }
    }

    async fn get_daemon_status(
        &self,
        request: Request<rspm_proto::GetDaemonStatusRequest>,
    ) -> std::result::Result<Response<rspm_proto::GetDaemonStatusResponse>, Status> {
        self.verify_token(&request)?;
        let (total_processes, uptime) = self.process_manager.get_status().await;

        Ok(Response::new(rspm_proto::GetDaemonStatusResponse {
            status: Some(ProtoDaemonStatus {
                running: true,
                total_processes: total_processes as i32,
                uptime_ms: uptime as i64,
                version: env!("CARGO_PKG_VERSION").to_string(),
            }),
        }))
    }

    type StreamLogsStream = Pin<Box<dyn Stream<Item = std::result::Result<ProtoLogEntry, Status>> + Send>>;

    async fn stream_logs(
        &self,
        request: Request<rspm_proto::StreamLogsRequest>,
    ) -> std::result::Result<Response<Self::StreamLogsStream>, Status> {
        self.verify_token(&request)?;
        let req = request.into_inner();
        let id = req.id;
        let lines = req.lines as usize;
        let include_stderr = req.stderr;

        // Read historical logs first
        let history = match self.process_manager.read_log_history(&id, lines, include_stderr).await {
            Ok(entries) => entries,
            Err(e) => return Err(Status::not_found(e.to_string())),
        };

        // Subscribe to real-time logs
        let mut log_rx = match self.process_manager.subscribe_logs(&id).await {
            Ok(rx) => rx,
            Err(e) => return Err(Status::not_found(e.to_string())),
        };

        let (tx, rx) = tokio::sync::mpsc::channel::<std::result::Result<ProtoLogEntry, Status>>(128);

        tokio::spawn(async move {
            // First, send historical logs
            for entry in history {
                let proto_entry = ProtoLogEntry {
                    id: entry.process_id,
                    timestamp: entry.timestamp.timestamp_millis(),
                    message: entry.message,
                    is_error: entry.is_error,
                };

                if tx.send(Ok(proto_entry)).await.is_err() {
                    // Client disconnected
                    return;
                }
            }

            // Then, stream real-time logs
            loop {
                match log_rx.recv().await {
                    Ok(entry) => {
                        // Skip stderr if not requested
                        if entry.is_error && !include_stderr {
                            continue;
                        }

                        // Send to client
                        let proto_entry = ProtoLogEntry {
                            id: entry.process_id,
                            timestamp: entry.timestamp.timestamp_millis(),
                            message: entry.message,
                            is_error: entry.is_error,
                        };

                        if tx.send(Ok(proto_entry)).await.is_err() {
                            // Client disconnected
                            break;
                        }
                    }
                    Err(_) => {
                        // Channel closed or lagged
                        break;
                    }
                }
            }
        });

        Ok(Response::new(Box::pin(ReceiverStream::new(rx))))
    }

    async fn save_process(
        &self,
        request: Request<rspm_proto::SaveProcessRequest>,
    ) -> std::result::Result<Response<rspm_proto::SaveProcessResponse>, Status> {
        self.verify_token(&request)?;
        let proto_config = request.into_inner()
            .config
            .ok_or_else(|| Status::invalid_argument("Missing config"))?;

        let config = from_proto_process_config(&proto_config);

        // Just validate and return success for now
        if config.name.is_empty() || config.command.is_empty() {
            return Ok(Response::new(rspm_proto::SaveProcessResponse {
                success: false,
                message: "Name and command are required".to_string(),
            }));
        }

        Ok(Response::new(rspm_proto::SaveProcessResponse {
            success: true,
            message: "Configuration saved".to_string(),
        }))
    }

    async fn scale_process(
        &self,
        request: Request<rspm_proto::ScaleProcessRequest>,
    ) -> std::result::Result<Response<rspm_proto::ScaleProcessResponse>, Status> {
        self.verify_token(&request)?;
        let req = request.into_inner();

        match self.process_manager.scale_process(&req.id, req.instances as u32).await {
            Ok(new_ids) => Ok(Response::new(rspm_proto::ScaleProcessResponse {
                success: true,
                message: format!("Scaled to {} instances", req.instances),
                new_instance_ids: new_ids,
            })),
            Err(e) => Ok(Response::new(rspm_proto::ScaleProcessResponse {
                success: false,
                message: e.to_string(),
                new_instance_ids: vec![],
            })),
        }
    }

    async fn flush_logs(
        &self,
        request: Request<rspm_proto::FlushLogsRequest>,
    ) -> std::result::Result<Response<rspm_proto::FlushLogsResponse>, Status> {
        self.verify_token(&request)?;
        Ok(Response::new(rspm_proto::FlushLogsResponse {
            success: true,
            message: "Logs flushed".to_string(),
        }))
    }

    async fn reload_logs(
        &self,
        request: Request<rspm_proto::ReloadLogsRequest>,
    ) -> std::result::Result<Response<rspm_proto::ReloadLogsResponse>, Status> {
        self.verify_token(&request)?;
        Ok(Response::new(rspm_proto::ReloadLogsResponse {
            success: true,
            message: "Logs reloaded".to_string(),
        }))
    }

    async fn stop_daemon(
        &self,
        request: Request<rspm_proto::StopDaemonRequest>,
    ) -> std::result::Result<Response<rspm_proto::StopDaemonResponse>, Status> {
        self.verify_token(&request)?;
        info!("Stop daemon request received");

        // Shutdown process manager first (stop all processes and scheduler)
        self.process_manager.shutdown().await;

        // Trigger shutdown
        self.trigger_shutdown();

        Ok(Response::new(rspm_proto::StopDaemonResponse {
            success: true,
            message: "Daemon is shutting down".to_string(),
        }))
    }

    // ==================== Schedule Methods ====================

    async fn create_schedule(
        &self,
        request: Request<rspm_proto::CreateScheduleRequest>,
    ) -> std::result::Result<Response<rspm_proto::CreateScheduleResponse>, Status> {
        self.verify_token(&request)?;
        let proto_config = request.into_inner()
            .config
            .ok_or_else(|| Status::invalid_argument("Missing config"))?;

        let config = from_proto_schedule_config(&proto_config);

        // Validate cron expression if it's a cron schedule
        if let rspm_common::ScheduleType::Cron(ref expr) = config.schedule {
            if let Err(e) = rspm_common::ScheduleConfig::validate_cron(expr) {
                return Ok(Response::new(rspm_proto::CreateScheduleResponse {
                    id: String::new(),
                    success: false,
                    message: e,
                }));
            }
        }

        match self.process_manager.create_schedule(config).await {
            Ok(id) => Ok(Response::new(rspm_proto::CreateScheduleResponse {
                id,
                success: true,
                message: "Schedule created".to_string(),
            })),
            Err(e) => Ok(Response::new(rspm_proto::CreateScheduleResponse {
                id: String::new(),
                success: false,
                message: e.to_string(),
            })),
        }
    }

    async fn update_schedule(
        &self,
        request: Request<rspm_proto::UpdateScheduleRequest>,
    ) -> std::result::Result<Response<rspm_proto::UpdateScheduleResponse>, Status> {
        self.verify_token(&request)?;
        let req = request.into_inner();
        let proto_config = req.config
            .ok_or_else(|| Status::invalid_argument("Missing config"))?;

        let config = from_proto_schedule_config(&proto_config);

        match self.process_manager.update_schedule(&req.id, config).await {
            Ok(_) => Ok(Response::new(rspm_proto::UpdateScheduleResponse {
                success: true,
                message: "Schedule updated".to_string(),
            })),
            Err(e) => Ok(Response::new(rspm_proto::UpdateScheduleResponse {
                success: false,
                message: e.to_string(),
            })),
        }
    }

    async fn delete_schedule(
        &self,
        request: Request<rspm_proto::DeleteScheduleRequest>,
    ) -> std::result::Result<Response<rspm_proto::DeleteScheduleResponse>, Status> {
        self.verify_token(&request)?;
        let req = request.into_inner();

        match self.process_manager.delete_schedule(&req.id).await {
            Ok(_) => Ok(Response::new(rspm_proto::DeleteScheduleResponse {
                success: true,
                message: "Schedule deleted".to_string(),
            })),
            Err(e) => Ok(Response::new(rspm_proto::DeleteScheduleResponse {
                success: false,
                message: e.to_string(),
            })),
        }
    }

    async fn get_schedule(
        &self,
        request: Request<rspm_proto::GetScheduleRequest>,
    ) -> std::result::Result<Response<rspm_proto::GetScheduleResponse>, Status> {
        self.verify_token(&request)?;
        let req = request.into_inner();

        match self.process_manager.get_schedule(&req.id).await {
            Ok(Some(info)) => Ok(Response::new(rspm_proto::GetScheduleResponse {
                schedule: Some(to_proto_schedule_info(&info)),
            })),
            Ok(None) => Err(Status::not_found("Schedule not found")),
            Err(e) => Err(Status::internal(e.to_string())),
        }
    }

    async fn list_schedules(
        &self,
        request: Request<rspm_proto::ListSchedulesRequest>,
    ) -> std::result::Result<Response<rspm_proto::ListSchedulesResponse>, Status> {
        self.verify_token(&request)?;
        match self.process_manager.list_schedules().await {
            Ok(schedules) => {
                let proto_schedules: Vec<_> = schedules
                    .iter()
                    .map(to_proto_schedule_info)
                    .collect();
                Ok(Response::new(rspm_proto::ListSchedulesResponse {
                    schedules: proto_schedules,
                }))
            }
            Err(e) => Err(Status::internal(e.to_string())),
        }
    }

    async fn pause_schedule(
        &self,
        request: Request<rspm_proto::PauseScheduleRequest>,
    ) -> std::result::Result<Response<rspm_proto::PauseScheduleResponse>, Status> {
        self.verify_token(&request)?;
        let req = request.into_inner();

        match self.process_manager.pause_schedule(&req.id).await {
            Ok(_) => Ok(Response::new(rspm_proto::PauseScheduleResponse {
                success: true,
                message: "Schedule paused".to_string(),
            })),
            Err(e) => Ok(Response::new(rspm_proto::PauseScheduleResponse {
                success: false,
                message: e.to_string(),
            })),
        }
    }

    async fn resume_schedule(
        &self,
        request: Request<rspm_proto::ResumeScheduleRequest>,
    ) -> std::result::Result<Response<rspm_proto::ResumeScheduleResponse>, Status> {
        self.verify_token(&request)?;
        let req = request.into_inner();

        match self.process_manager.resume_schedule(&req.id).await {
            Ok(_) => Ok(Response::new(rspm_proto::ResumeScheduleResponse {
                success: true,
                message: "Schedule resumed".to_string(),
            })),
            Err(e) => Ok(Response::new(rspm_proto::ResumeScheduleResponse {
                success: false,
                message: e.to_string(),
            })),
        }
    }

    async fn get_schedule_executions(
        &self,
        request: Request<rspm_proto::GetScheduleExecutionsRequest>,
    ) -> std::result::Result<Response<rspm_proto::GetScheduleExecutionsResponse>, Status> {
        self.verify_token(&request)?;
        let req = request.into_inner();

        match self.process_manager.get_schedule_executions(&req.id, req.limit as u32).await {
            Ok(executions) => {
                let proto_executions: Vec<_> = executions
                    .iter()
                    .map(to_proto_execution)
                    .collect();
                Ok(Response::new(rspm_proto::GetScheduleExecutionsResponse {
                    executions: proto_executions,
                }))
            }
            Err(e) => Err(Status::internal(e.to_string())),
        }
    }

    // ==================== Static Server Methods ====================

    async fn start_static_server(
        &self,
        request: Request<rspm_proto::StartStaticServerRequest>,
    ) -> std::result::Result<Response<rspm_proto::StartStaticServerResponse>, Status> {
        self.verify_token(&request)?;
        let req = request.into_inner();

        match self.process_manager.start_static_server(
            req.name,
            req.host,
            req.port,
            req.directory,
        ).await {
            Ok(id) => Ok(Response::new(rspm_proto::StartStaticServerResponse {
                id,
                success: true,
                message: "Static server started successfully".to_string(),
            })),
            Err(e) => Ok(Response::new(rspm_proto::StartStaticServerResponse {
                id: String::new(),
                success: false,
                message: e.to_string(),
            })),
        }
    }

    async fn stop_static_server(
        &self,
        request: Request<rspm_proto::StopStaticServerRequest>,
    ) -> std::result::Result<Response<rspm_proto::StopStaticServerResponse>, Status> {
        self.verify_token(&request)?;
        let req = request.into_inner();

        match self.process_manager.stop_static_server(&req.id).await {
            Ok(_) => Ok(Response::new(rspm_proto::StopStaticServerResponse {
                success: true,
                message: "Static server stopped".to_string(),
            })),
            Err(e) => Ok(Response::new(rspm_proto::StopStaticServerResponse {
                success: false,
                message: e.to_string(),
            })),
        }
    }

    async fn list_static_servers(
        &self,
        request: Request<rspm_proto::ListStaticServersRequest>,
    ) -> std::result::Result<Response<rspm_proto::ListStaticServersResponse>, Status> {
        self.verify_token(&request)?;
        let servers = self.process_manager.list_static_servers().await;
        let proto_servers: Vec<_> = servers.iter().map(to_proto_static_server_info).collect();

        Ok(Response::new(rspm_proto::ListStaticServersResponse {
            servers: proto_servers,
        }))
    }

    async fn get_static_server(
        &self,
        request: Request<rspm_proto::GetStaticServerRequest>,
    ) -> std::result::Result<Response<rspm_proto::GetStaticServerResponse>, Status> {
        self.verify_token(&request)?;
        let req = request.into_inner();

        match self.process_manager.get_static_server(&req.id).await {
            Some(info) => Ok(Response::new(rspm_proto::GetStaticServerResponse {
                server: Some(to_proto_static_server_info(&info)),
            })),
            None => Err(Status::not_found("Static server not found")),
        }
    }
}

// Convert StaticServerInfo to proto
fn to_proto_static_server_info(info: &rspm_common::StaticServerInfo) -> rspm_proto::StaticServerInfo {
    rspm_proto::StaticServerInfo {
        id: info.id.clone(),
        name: info.name.clone(),
        host: info.host.clone(),
        port: info.port,
        directory: info.directory.clone(),
        running: info.running,
        started_at: info.started_at.map(|t| t.timestamp_millis()).unwrap_or(0),
    }
}
