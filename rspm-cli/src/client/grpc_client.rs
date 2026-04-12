use rspm_common::{ProcessConfig, ProcessInfo, ProcessState, ProcessStats, Result, RspmError};
use rspm_proto::process_manager_client::ProcessManagerClient;
use std::collections::HashMap;

#[cfg(unix)]
use http::Uri;
#[cfg(unix)]
use tonic::transport::Endpoint;
use tonic::metadata::{MetadataKey, MetadataValue};

/// gRPC client for communicating with the daemon
pub struct GrpcClient {
    client: ProcessManagerClient<tonic::transport::Channel>,
    token: Option<String>,
}

impl GrpcClient {
    /// Connect to the daemon via Unix socket (for local connections)
    #[cfg(unix)]
    pub async fn connect(socket_path: &str) -> Result<Self> {
        // Check if it's a TCP address instead of socket path
        if socket_path.contains(':') {
            // It's a TCP address, use TCP connection
            return Self::connect_tcp(socket_path).await;
        }

        // It's a Unix socket path
        use hyper_util::rt::TokioIo;
        use tokio::net::UnixStream;

        let path = socket_path.to_string();
        let channel = Endpoint::try_from("unix://unused")
            .map_err(|e| RspmError::GrpcError(e.to_string()))?
            .connect_with_connector(tower::service_fn(move |_: Uri| {
                let path = path.clone();
                async move {
                    let stream = UnixStream::connect(&path).await?;
                    Ok::<_, std::io::Error>(TokioIo::new(stream))
                }
            }))
            .await
            .map_err(|e| {
                RspmError::GrpcError(format!(
                    "Failed to connect to daemon: {}. Is the daemon running?",
                    e
                ))
            })?;

        // Get token from config
        let config = rspm_common::DaemonConfig::load_default();
        let token = config.token.clone();

        let client = ProcessManagerClient::new(channel);

        Ok(Self { client, token })
    }

    /// Connect via TCP (for remote connections)
    #[cfg(unix)]
    pub async fn connect_tcp(addr: &str) -> Result<Self> {
        let addr = if addr.starts_with("http") {
            addr.to_string()
        } else {
            format!("http://{}", addr)
        };

        // Get token from config
        let config = rspm_common::DaemonConfig::load_default();
        let token = config.token.clone();

        let client = ProcessManagerClient::connect(addr.to_string())
            .await
            .map_err(|e| RspmError::GrpcError(format!("Failed to connect to daemon: {}", e)))?;

        Ok(Self { client, token })
    }

    #[cfg(not(unix))]
    pub async fn connect(addr: &str) -> Result<Self> {
        // Get the actual address from config if not provided
        let addr = if addr.starts_with("unix://") {
            // For Windows, use the configured port instead of hardcoded one
            let config = rspm_common::DaemonConfig::load_default();
            format!("http://{}", config.get_grpc_addr())
        } else if addr.starts_with("http") {
            addr.to_string()
        } else {
            format!("http://{}", addr)
        };

        // Get token from config
        let config = rspm_common::DaemonConfig::load_default();
        let token = config.token.clone();

        let client = ProcessManagerClient::connect(addr.to_string())
            .await
            .map_err(|e| RspmError::GrpcError(format!("Failed to connect to daemon: {}", e)))?;

        Ok(Self { client, token })
    }

    /// Create a request with token
    fn create_request<T: Default>(&self, msg: T) -> tonic::Request<T> {
        let mut req = tonic::Request::new(msg);
        if let Some(ref token) = self.token {
            let key = MetadataKey::from_static("x-rspm-token");
            if let Ok(value) = MetadataValue::try_from(token.as_str()) {
                req.metadata_mut().insert(key, value);
            }
        }
        req
    }

    /// Start a process
    pub async fn start_process(&mut self, config: ProcessConfig) -> Result<String> {
        let request = rspm_proto::StartProcessRequest {
            config: Some(to_proto_config(&config)),
        };
        let request = self.create_request(request);

        let response = self
            .client
            .start_process(request)
            .await
            .map_err(|e| RspmError::GrpcError(e.to_string()))?
            .into_inner();

        if response.success {
            Ok(response.id)
        } else {
            Err(RspmError::StartFailed(response.message))
        }
    }

    /// Stop a process
    pub async fn stop_process(&mut self, id: &str, force: bool) -> Result<()> {
        let request = rspm_proto::StopProcessRequest {
            id: id.to_string(),
            force,
        };
        let request = self.create_request(request);

        let response = self
            .client
            .stop_process(request)
            .await
            .map_err(|e| RspmError::GrpcError(e.to_string()))?
            .into_inner();

        if response.success {
            Ok(())
        } else {
            Err(RspmError::StopFailed(response.message))
        }
    }

    /// Restart a process
    pub async fn restart_process(&mut self, id: &str) -> Result<()> {
        let request = rspm_proto::RestartProcessRequest { id: id.to_string() };
        let request = self.create_request(request);

        let response = self
            .client
            .restart_process(request)
            .await
            .map_err(|e| RspmError::GrpcError(e.to_string()))?
            .into_inner();

        if response.success {
            Ok(())
        } else {
            Err(RspmError::StateError(response.message))
        }
    }

    /// Delete a process
    pub async fn delete_process(&mut self, id: &str) -> Result<()> {
        let request = rspm_proto::DeleteProcessRequest { id: id.to_string() };
        let request = self.create_request(request);

        let response = self
            .client
            .delete_process(request)
            .await
            .map_err(|e| RspmError::GrpcError(e.to_string()))?
            .into_inner();

        if response.success {
            Ok(())
        } else {
            Err(RspmError::ProcessNotFound(response.message))
        }
    }

    /// List all processes
    pub async fn list_processes(&mut self, name_filter: Option<&str>) -> Result<Vec<ProcessInfo>> {
        let request = rspm_proto::ListProcessesRequest {
            name_filter: name_filter.unwrap_or("").to_string(),
        };
        let request = self.create_request(request);

        let response = self
            .client
            .list_processes(request)
            .await
            .map_err(|e| RspmError::GrpcError(e.to_string()))?
            .into_inner();

        Ok(response
            .processes
            .into_iter()
            .map(from_proto_info)
            .collect())
    }

    /// Get a specific process
    pub async fn get_process(&mut self, id: &str) -> Result<Option<ProcessInfo>> {
        let request = rspm_proto::GetProcessRequest { id: id.to_string() };
        let request = self.create_request(request);

        match self.client.get_process(request).await {
            Ok(response) => Ok(Some(from_proto_info(
                response.into_inner().process.unwrap(),
            ))),
            Err(e) => {
                if e.code() == tonic::Code::NotFound {
                    Ok(None)
                } else {
                    Err(RspmError::GrpcError(e.to_string()))
                }
            }
        }
    }

    /// Scale a process
    pub async fn scale_process(&mut self, id: &str, instances: u32) -> Result<Vec<String>> {
        let request = rspm_proto::ScaleProcessRequest {
            id: id.to_string(),
            instances: instances as i32,
        };
        let request = self.create_request(request);

        let response = self
            .client
            .scale_process(request)
            .await
            .map_err(|e| RspmError::GrpcError(e.to_string()))?
            .into_inner();

        if response.success {
            Ok(response.new_instance_ids)
        } else {
            Err(RspmError::StateError(response.message))
        }
    }

    /// Stop all processes
    pub async fn stop_all_processes(&mut self) -> Result<u32> {
        let request = rspm_proto::StopAllProcessesRequest {};
        let request = self.create_request(request);

        let response = self
            .client
            .stop_all_processes(request)
            .await
            .map_err(|e| RspmError::GrpcError(e.to_string()))?
            .into_inner();

        Ok(response.stopped_count as u32)
    }

    /// Get daemon status
    pub async fn get_daemon_status(&mut self) -> Result<rspm_proto::DaemonStatus> {
        let request = rspm_proto::GetDaemonStatusRequest {};
        let request = self.create_request(request);

        let response = self
            .client
            .get_daemon_status(request)
            .await
            .map_err(|e| RspmError::GrpcError(e.to_string()))?
            .into_inner();

        Ok(response.status.unwrap())
    }

    /// Stop the daemon
    pub async fn stop_daemon(&mut self) -> Result<()> {
        let request = rspm_proto::StopDaemonRequest {};
        let request = self.create_request(request);

        let response = self
            .client
            .stop_daemon(request)
            .await
            .map_err(|e| RspmError::GrpcError(e.to_string()))?
            .into_inner();

        if response.success {
            Ok(())
        } else {
            Err(RspmError::InternalError(response.message))
        }
    }

    /// Stream logs for a process
    pub async fn stream_logs(
        &mut self,
        id: &str,
        follow: bool,
        lines: u32,
        stderr: bool,
    ) -> Result<tonic::Streaming<rspm_proto::LogEntry>> {
        let request = rspm_proto::StreamLogsRequest {
            id: id.to_string(),
            follow,
            lines: lines as i32,
            stderr,
        };
        let request = self.create_request(request);

        let response = self
            .client
            .stream_logs(request)
            .await
            .map_err(|e| RspmError::GrpcError(e.to_string()))?
            .into_inner();

        Ok(response)
    }
}

// Helper functions to convert between proto and domain types

fn to_proto_config(config: &ProcessConfig) -> rspm_proto::ProcessConfig {
    rspm_proto::ProcessConfig {
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

fn from_proto_info(proto: rspm_proto::ProcessInfo) -> ProcessInfo {
    let config_proto = proto.config.unwrap();
    let mut env = HashMap::new();
    for (k, v) in &config_proto.env {
        env.insert(k.clone(), v.clone());
    }

    let state = match proto.state {
        0 => ProcessState::Stopped,
        1 => ProcessState::Starting,
        2 => ProcessState::Running,
        3 => ProcessState::Stopping,
        4 => ProcessState::Errored,
        _ => ProcessState::Stopped,
    };

    let started_at = if proto.started_at > 0 {
        Some(
            chrono::DateTime::from_timestamp_millis(proto.started_at)
                .unwrap_or_else(chrono::Utc::now),
        )
    } else {
        None
    };

    ProcessInfo {
        db_id: Some(proto.db_id),
        id: proto.id,
        name: proto.name,
        state,
        pid: if proto.pid > 0 {
            Some(proto.pid as u32)
        } else {
            None
        },
        instance_id: proto.instance_id as u32,
        config: ProcessConfig {
            name: config_proto.name,
            command: config_proto.command,
            args: config_proto.args,
            env,
            cwd: if config_proto.cwd.is_empty() {
                None
            } else {
                Some(config_proto.cwd)
            },
            instances: config_proto.instances as u32,
            autorestart: config_proto.autorestart,
            max_restarts: config_proto.max_restarts as u32,
            max_memory_mb: config_proto.max_memory_mb as u32,
            watch: config_proto.watch,
            watch_paths: config_proto.watch_paths,
            log_file: if config_proto.log_file.is_empty() {
                None
            } else {
                Some(config_proto.log_file)
            },
            error_file: if config_proto.error_file.is_empty() {
                None
            } else {
                Some(config_proto.error_file)
            },
            log_max_size: config_proto.log_max_size as u64,
            log_max_files: config_proto.log_max_files as u32,
            server_type: match config_proto.server_type {
                1 => rspm_common::ServerType::StaticServer,
                _ => rspm_common::ServerType::Process,
            },
        },
        uptime_ms: proto.uptime_ms as u64,
        restart_count: proto.restart_count as u32,
        stats: ProcessStats {
            cpu_percent: proto.cpu_percent,
            memory_bytes: proto.memory_bytes as u64,
            fd_count: 0,
        },
        created_at: chrono::DateTime::from_timestamp_millis(proto.created_at)
            .unwrap_or_else(chrono::Utc::now),
        started_at,
        exit_code: None,
        error_message: None,
    }
}

// ==================== Schedule Methods ====================

impl GrpcClient {
    /// Create a schedule
    pub async fn create_schedule(
        &mut self,
        config: &rspm_common::ScheduleConfig,
    ) -> Result<String> {
        use rspm_common::{ScheduleAction, ScheduleType};

        let (schedule_type, schedule_value) = match &config.schedule {
            ScheduleType::Cron(expr) => (0, expr.clone()),
            ScheduleType::Interval(secs) => (1, secs.to_string()),
            ScheduleType::Once(dt) => (2, dt.to_rfc3339()),
        };

        let (action, action_command, action_args) = match &config.action {
            ScheduleAction::Start => (0, String::new(), vec![]),
            ScheduleAction::Stop => (1, String::new(), vec![]),
            ScheduleAction::Restart => (2, String::new(), vec![]),
            ScheduleAction::Execute { command, args } => (3, command.clone(), args.clone()),
        };

        let request = rspm_proto::CreateScheduleRequest {
            config: Some(rspm_proto::ScheduleConfig {
                id: config.id.clone().unwrap_or_default(),
                name: config.name.clone(),
                process_name: config.process_name.clone().unwrap_or_default(),
                schedule_type,
                schedule_value,
                action,
                action_command,
                action_args,
                enabled: config.enabled,
                timezone: config.timezone.clone(),
                max_runs: config.max_runs as i32,
                description: config.description.clone().unwrap_or_default(),
            }),
        };
        let request = self.create_request(request);

        let response = self
            .client
            .create_schedule(request)
            .await
            .map_err(|e| RspmError::GrpcError(e.to_string()))?
            .into_inner();

        if response.success {
            Ok(response.id)
        } else {
            Err(RspmError::InvalidConfig(response.message))
        }
    }

    /// List all schedules
    pub async fn list_schedules(&mut self) -> Result<Vec<rspm_common::ScheduleInfo>> {
        let request = rspm_proto::ListSchedulesRequest {
            name_filter: String::new(),
        };
        let request = self.create_request(request);

        let response = self
            .client
            .list_schedules(request)
            .await
            .map_err(|e| RspmError::GrpcError(e.to_string()))?
            .into_inner();

        let schedules: Vec<rspm_common::ScheduleInfo> = response
            .schedules
            .into_iter()
            .filter_map(from_proto_schedule_info)
            .collect();

        Ok(schedules)
    }

    /// Get a schedule by ID
    pub async fn get_schedule(&mut self, id: &str) -> Result<Option<rspm_common::ScheduleInfo>> {
        let request = rspm_proto::GetScheduleRequest { id: id.to_string() };
        let request = self.create_request(request);

        let response = self
            .client
            .get_schedule(request)
            .await
            .map_err(|e| RspmError::GrpcError(e.to_string()))?
            .into_inner();

        match response.schedule {
            Some(s) => Ok(from_proto_schedule_info(s)),
            None => Ok(None),
        }
    }

    /// Delete a schedule
    pub async fn delete_schedule(&mut self, id: &str) -> Result<()> {
        let request = rspm_proto::DeleteScheduleRequest { id: id.to_string() };
        let request = self.create_request(request);

        let response = self
            .client
            .delete_schedule(request)
            .await
            .map_err(|e| RspmError::GrpcError(e.to_string()))?
            .into_inner();

        if response.success {
            Ok(())
        } else {
            Err(RspmError::InvalidConfig(response.message))
        }
    }

    /// Pause a schedule
    pub async fn pause_schedule(&mut self, id: &str) -> Result<()> {
        let request = rspm_proto::PauseScheduleRequest { id: id.to_string() };
        let request = self.create_request(request);

        let response = self
            .client
            .pause_schedule(request)
            .await
            .map_err(|e| RspmError::GrpcError(e.to_string()))?
            .into_inner();

        if response.success {
            Ok(())
        } else {
            Err(RspmError::InvalidConfig(response.message))
        }
    }

    /// Resume a schedule
    pub async fn resume_schedule(&mut self, id: &str) -> Result<()> {
        let request = rspm_proto::ResumeScheduleRequest { id: id.to_string() };
        let request = self.create_request(request);

        let response = self
            .client
            .resume_schedule(request)
            .await
            .map_err(|e| RspmError::GrpcError(e.to_string()))?
            .into_inner();

        if response.success {
            Ok(())
        } else {
            Err(RspmError::InvalidConfig(response.message))
        }
    }

    /// Get schedule execution history
    pub async fn get_schedule_executions(
        &mut self,
        id: &str,
        limit: u32,
    ) -> Result<Vec<rspm_common::ScheduleExecution>> {
        let request = rspm_proto::GetScheduleExecutionsRequest {
            id: id.to_string(),
            limit: limit as i32,
        };
        let request = self.create_request(request);

        let response = self
            .client
            .get_schedule_executions(request)
            .await
            .map_err(|e| RspmError::GrpcError(e.to_string()))?
            .into_inner();

        let executions: Vec<rspm_common::ScheduleExecution> = response
            .executions
            .into_iter()
            .filter_map(from_proto_execution)
            .collect();

        Ok(executions)
    }
}

fn from_proto_schedule_info(proto: rspm_proto::ScheduleInfo) -> Option<rspm_common::ScheduleInfo> {
    use rspm_common::{ScheduleAction, ScheduleConfig, ScheduleStatus, ScheduleType};

    let config_proto = proto.config?;

    let schedule = match config_proto.schedule_type {
        0 => ScheduleType::Cron(config_proto.schedule_value),
        1 => ScheduleType::Interval(config_proto.schedule_value.parse().unwrap_or(60)),
        2 => ScheduleType::Once(
            chrono::DateTime::parse_from_rfc3339(&config_proto.schedule_value)
                .ok()?
                .with_timezone(&chrono::Utc),
        ),
        _ => ScheduleType::Interval(60),
    };

    let action = match config_proto.action {
        0 => ScheduleAction::Start,
        1 => ScheduleAction::Stop,
        2 => ScheduleAction::Restart,
        3 => ScheduleAction::Execute {
            command: config_proto.action_command,
            args: config_proto.action_args,
        },
        _ => ScheduleAction::Start,
    };

    let status = match proto.status {
        0 => ScheduleStatus::Active,
        1 => ScheduleStatus::Paused,
        2 => ScheduleStatus::Completed,
        3 => ScheduleStatus::Error(String::new()),
        _ => ScheduleStatus::Active,
    };

    let config = ScheduleConfig {
        id: if config_proto.id.is_empty() {
            None
        } else {
            Some(config_proto.id)
        },
        name: config_proto.name,
        process_name: if config_proto.process_name.is_empty() {
            None
        } else {
            Some(config_proto.process_name)
        },
        schedule,
        action,
        enabled: config_proto.enabled,
        timezone: config_proto.timezone,
        max_runs: config_proto.max_runs as u32,
        description: if config_proto.description.is_empty() {
            None
        } else {
            Some(config_proto.description)
        },
    };

    Some(rspm_common::ScheduleInfo {
        id: proto.id,
        config,
        status,
        created_at: chrono::DateTime::from_timestamp_millis(proto.created_at)
            .unwrap_or_else(chrono::Utc::now),
        updated_at: chrono::DateTime::from_timestamp_millis(proto.updated_at)
            .unwrap_or_else(chrono::Utc::now),
        last_run: if proto.last_run > 0 {
            chrono::DateTime::from_timestamp_millis(proto.last_run)
        } else {
            None
        },
        next_run: if proto.next_run > 0 {
            chrono::DateTime::from_timestamp_millis(proto.next_run)
        } else {
            None
        },
        run_count: proto.run_count as u32,
        success_count: proto.success_count as u32,
        fail_count: proto.fail_count as u32,
    })
}

fn from_proto_execution(
    proto: rspm_proto::ScheduleExecution,
) -> Option<rspm_common::ScheduleExecution> {
    use rspm_common::{ExecutionStatus, ScheduleExecution};

    let status = match proto.status.as_str() {
        "running" => ExecutionStatus::Running,
        "success" => ExecutionStatus::Success,
        "failed" => ExecutionStatus::Failed,
        "timeout" => ExecutionStatus::Timeout,
        _ => ExecutionStatus::Failed,
    };

    Some(ScheduleExecution {
        id: proto.id,
        schedule_id: proto.schedule_id,
        started_at: chrono::DateTime::from_timestamp_millis(proto.started_at)
            .unwrap_or_else(chrono::Utc::now),
        ended_at: if proto.ended_at > 0 {
            chrono::DateTime::from_timestamp_millis(proto.ended_at)
        } else {
            None
        },
        status,
        output: if proto.output.is_empty() {
            None
        } else {
            Some(proto.output)
        },
        error: if proto.error.is_empty() {
            None
        } else {
            Some(proto.error)
        },
    })
}
