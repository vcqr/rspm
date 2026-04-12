use axum::{
    Json,
    extract::{Path, State, WebSocketUpgrade},
    response::IntoResponse,
};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

use crate::manager::ProcessManager;

#[derive(Serialize)]
pub struct ProcessListResponse {
    pub processes: Vec<ProcessInfoDto>,
}

#[derive(Serialize)]
pub struct ProcessInfoDto {
    pub id: String,
    pub name: String,
    pub status: String,
    pub pid: Option<u32>,
    pub cpu_percent: f64,
    pub memory_bytes: u64,
    pub uptime_ms: u64,
    pub restart_count: u32,
    pub command: String,
    pub cwd: Option<String>,
    pub config: Option<rspm_common::ProcessConfig>,
}

#[derive(Serialize)]
pub struct ApiResponse<T> {
    pub success: bool,
    pub data: Option<T>,
    pub error: Option<String>,
}

impl<T> ApiResponse<T> {
    pub fn success(data: T) -> Self {
        Self {
            success: true,
            data: Some(data),
            error: None,
        }
    }

    pub fn error(msg: String) -> Self {
        Self {
            success: false,
            data: None,
            error: Some(msg),
        }
    }
}

pub async fn list_processes(
    State(manager): State<Arc<ProcessManager>>,
) -> Json<ApiResponse<ProcessListResponse>> {
    let processes = manager.list_processes().await;

    let dtos: Vec<ProcessInfoDto> = processes
        .into_iter()
        .map(|p| ProcessInfoDto {
            id: p.id,
            name: p.name,
            status: format!("{:?}", p.state).to_lowercase(),
            pid: p.pid,
            cpu_percent: p.stats.cpu_percent,
            memory_bytes: p.stats.memory_bytes,
            uptime_ms: p.uptime_ms,
            restart_count: p.restart_count,
            command: format!("{} {}", p.config.command, p.config.args.join(" ")),
            cwd: p.config.cwd.clone(),
            config: Some(p.config),
        })
        .collect();

    Json(ApiResponse::success(ProcessListResponse {
        processes: dtos,
    }))
}

pub async fn get_process(
    State(manager): State<Arc<ProcessManager>>,
    Path(id): Path<String>,
) -> Json<ApiResponse<ProcessInfoDto>> {
    match manager.get_process(&id).await {
        Some(p) => {
            let dto = ProcessInfoDto {
                id: p.id,
                name: p.name,
                status: format!("{:?}", p.state).to_lowercase(),
                pid: p.pid,
                cpu_percent: p.stats.cpu_percent,
                memory_bytes: p.stats.memory_bytes,
                uptime_ms: p.uptime_ms,
                restart_count: p.restart_count,
                command: format!("{} {}", p.config.command, p.config.args.join(" ")),
                cwd: p.config.cwd.clone(),
                config: Some(p.config),
            };
            Json(ApiResponse::success(dto))
        }
        None => Json(ApiResponse::error(format!("Process '{}' not found", id))),
    }
}

#[derive(Deserialize)]
pub struct StartProcessRequest {
    pub name: String,
    pub command: String,
    pub args: Vec<String>,
    pub instances: Option<u32>,
}

pub async fn start_process(
    State(manager): State<Arc<ProcessManager>>,
    Json(req): Json<StartProcessRequest>,
) -> Json<ApiResponse<String>> {
    let config = rspm_common::ProcessConfig {
        name: req.name,
        command: req.command,
        args: req.args,
        instances: req.instances.unwrap_or(1),
        ..Default::default()
    };

    match manager.start_process(config).await {
        Ok(id) => Json(ApiResponse::success(id)),
        Err(e) => Json(ApiResponse::error(e.to_string())),
    }
}

pub async fn stop_process(
    State(manager): State<Arc<ProcessManager>>,
    Path(id): Path<String>,
) -> Json<ApiResponse<String>> {
    match manager.stop_process(&id, false).await {
        Ok(_) => Json(ApiResponse::success("Process stopped".to_string())),
        Err(e) => Json(ApiResponse::error(e.to_string())),
    }
}

pub async fn start_process_by_id(
    State(manager): State<Arc<ProcessManager>>,
    Path(id): Path<String>,
) -> Json<ApiResponse<String>> {
    match manager.start_process_by_id(&id).await {
        Ok(_) => Json(ApiResponse::success("Process started".to_string())),
        Err(e) => Json(ApiResponse::error(e.to_string())),
    }
}

pub async fn restart_process(
    State(manager): State<Arc<ProcessManager>>,
    Path(id): Path<String>,
) -> Json<ApiResponse<String>> {
    match manager.restart_process(&id).await {
        Ok(_) => Json(ApiResponse::success("Process restarted".to_string())),
        Err(e) => Json(ApiResponse::error(e.to_string())),
    }
}

pub async fn delete_process(
    State(manager): State<Arc<ProcessManager>>,
    Path(id): Path<String>,
) -> Json<ApiResponse<String>> {
    match manager.delete_process(&id).await {
        Ok(_) => Json(ApiResponse::success("Process deleted".to_string())),
        Err(e) => Json(ApiResponse::error(e.to_string())),
    }
}

#[derive(Deserialize)]
pub struct UpdateProcessRequest {
    pub name: Option<String>,
    pub command: Option<String>,
    pub args: Option<Vec<String>>,
    pub instances: Option<u32>,
    pub cwd: Option<String>,
    pub env: Option<std::collections::HashMap<String, String>>,
    pub autorestart: Option<bool>,
    pub max_restarts: Option<u32>,
    pub max_memory_mb: Option<u32>,
    pub watch: Option<bool>,
}

pub async fn update_process(
    State(manager): State<Arc<ProcessManager>>,
    Path(id): Path<String>,
    Json(req): Json<UpdateProcessRequest>,
) -> Json<ApiResponse<String>> {
    // Get existing process config
    let processes = manager.list_processes().await;
    let existing = processes.iter().find(|p| p.id == id || p.name == id);

    if let Some(existing_proc) = existing {
        let mut new_config = existing_proc.config.clone();

        // Update only provided fields
        if let Some(name) = req.name {
            new_config.name = name;
        }
        if let Some(command) = req.command {
            new_config.command = command;
        }
        if let Some(args) = req.args {
            new_config.args = args;
        }
        if let Some(instances) = req.instances {
            new_config.instances = instances;
        }
        if let Some(cwd) = req.cwd {
            new_config.cwd = Some(cwd);
        }
        if let Some(env) = req.env {
            new_config.env = env;
        }
        if let Some(autorestart) = req.autorestart {
            new_config.autorestart = autorestart;
        }
        if let Some(max_restarts) = req.max_restarts {
            new_config.max_restarts = max_restarts;
        }
        if let Some(max_memory_mb) = req.max_memory_mb {
            new_config.max_memory_mb = max_memory_mb;
        }
        if let Some(watch) = req.watch {
            new_config.watch = watch;
        }

        match manager.update_process_config(&id, new_config).await {
            Ok(_) => Json(ApiResponse::success("Process updated".to_string())),
            Err(e) => Json(ApiResponse::error(e.to_string())),
        }
    } else {
        Json(ApiResponse::error(format!("Process '{}' not found", id)))
    }
}

pub async fn stop_all_processes(
    State(manager): State<Arc<ProcessManager>>,
) -> Json<ApiResponse<String>> {
    match manager.stop_all(false).await {
        Ok(count) => Json(ApiResponse::success(format!("Stopped {} processes", count))),
        Err(e) => Json(ApiResponse::error(e.to_string())),
    }
}

pub async fn get_logs(
    State(manager): State<Arc<ProcessManager>>,
    Path(id): Path<String>,
) -> Json<ApiResponse<Vec<rspm_common::LogEntry>>> {
    match manager.read_log_history(&id, 100, true).await {
        Ok(entries) => Json(ApiResponse::success(entries)),
        Err(e) => Json(ApiResponse::error(e.to_string())),
    }
}

pub async fn ws_logs(
    ws: WebSocketUpgrade,
    State(manager): State<Arc<ProcessManager>>,
    Path(id): Path<String>,
) -> impl IntoResponse {
    ws.on_upgrade(move |socket| handle_logs_socket(socket, manager, id))
}

async fn handle_logs_socket(
    mut socket: axum::extract::ws::WebSocket,
    manager: Arc<ProcessManager>,
    id: String,
) {
    use axum::extract::ws::Message;

    let mut rx = match manager.subscribe_logs(&id).await {
        Ok(rx) => rx,
        Err(_) => return,
    };

    while let Ok(entry) = rx.recv().await {
        let msg = serde_json::json!({
            "timestamp": entry.timestamp.timestamp_millis(),
            "message": entry.message,
            "is_error": entry.is_error,
        });

        if socket
            .send(Message::Text(msg.to_string().into()))
            .await
            .is_err()
        {
            break;
        }
    }
}

#[derive(Serialize)]
pub struct DaemonStatus {
    pub running: bool,
    pub total_processes: i32,
    pub uptime_ms: u64,
    pub version: String,
}

pub async fn get_status(
    State(manager): State<Arc<ProcessManager>>,
) -> Json<ApiResponse<DaemonStatus>> {
    let (total, uptime) = manager.get_status().await;

    let status = DaemonStatus {
        running: true,
        total_processes: total as i32,
        uptime_ms: uptime,
        version: env!("CARGO_PKG_VERSION").to_string(),
    };

    Json(ApiResponse::success(status))
}

// ============ Schedule API ============

#[derive(Serialize)]
pub struct ScheduleListResponse {
    pub schedules: Vec<ScheduleInfoDto>,
}

#[derive(Serialize)]
pub struct ScheduleInfoDto {
    pub id: String,
    pub name: String,
    pub description: Option<String>,
    pub status: String,
    pub schedule_type: String,
    pub schedule_value: String,
    pub action: String,
    pub target_process: Option<String>,
    pub command: Option<String>,
    pub args: Option<Vec<String>>,
    pub next_run: Option<String>,
    pub last_run: Option<String>,
    pub run_count: u32,
    pub success_count: u32,
    pub fail_count: u32,
    pub created_at: String,
}

impl From<rspm_common::ScheduleInfo> for ScheduleInfoDto {
    fn from(info: rspm_common::ScheduleInfo) -> Self {
        let schedule_type = match &info.config.schedule {
            rspm_common::ScheduleType::Cron(expr) => format!("cron({})", expr),
            rspm_common::ScheduleType::Interval(secs) => format!("interval({}s)", secs),
            rspm_common::ScheduleType::Once(dt) => {
                format!("once({})", dt.format("%Y-%m-%d %H:%M:%S"))
            }
        };

        let (action, command, args) = match &info.config.action {
            rspm_common::ScheduleAction::Start => ("start", None, None),
            rspm_common::ScheduleAction::Stop => ("stop", None, None),
            rspm_common::ScheduleAction::Restart => ("restart", None, None),
            rspm_common::ScheduleAction::Execute { command, args } => {
                ("execute", Some(command.clone()), Some(args.clone()))
            }
        };

        let schedule_value = schedule_type.clone();

        Self {
            id: info.id,
            name: info.config.name,
            description: info.config.description,
            status: format!("{:?}", info.status).to_lowercase(),
            schedule_type,
            schedule_value,
            action: action.to_string(),
            target_process: info.config.process_name,
            command,
            args,
            next_run: info
                .next_run
                .map(|dt| dt.format("%Y-%m-%d %H:%M:%S").to_string()),
            last_run: info
                .last_run
                .map(|dt| dt.format("%Y-%m-%d %H:%M:%S").to_string()),
            run_count: info.run_count,
            success_count: info.success_count,
            fail_count: info.fail_count,
            created_at: info.created_at.format("%Y-%m-%d %H:%M:%S").to_string(),
        }
    }
}

pub async fn list_schedules(
    State(manager): State<Arc<ProcessManager>>,
) -> Json<ApiResponse<ScheduleListResponse>> {
    match manager.list_schedules().await {
        Ok(schedules) => {
            let dtos: Vec<ScheduleInfoDto> =
                schedules.into_iter().map(ScheduleInfoDto::from).collect();
            Json(ApiResponse::success(ScheduleListResponse {
                schedules: dtos,
            }))
        }
        Err(e) => Json(ApiResponse::error(e.to_string())),
    }
}

pub async fn get_schedule(
    State(manager): State<Arc<ProcessManager>>,
    Path(id): Path<String>,
) -> Json<ApiResponse<ScheduleInfoDto>> {
    match manager.get_schedule(&id).await {
        Ok(Some(info)) => Json(ApiResponse::success(ScheduleInfoDto::from(info))),
        Ok(None) => Json(ApiResponse::error(format!("Schedule '{}' not found", id))),
        Err(e) => Json(ApiResponse::error(e.to_string())),
    }
}

pub async fn pause_schedule(
    State(manager): State<Arc<ProcessManager>>,
    Path(id): Path<String>,
) -> Json<ApiResponse<String>> {
    match manager.pause_schedule(&id).await {
        Ok(_) => Json(ApiResponse::success("Schedule paused".to_string())),
        Err(e) => Json(ApiResponse::error(e.to_string())),
    }
}

pub async fn resume_schedule(
    State(manager): State<Arc<ProcessManager>>,
    Path(id): Path<String>,
) -> Json<ApiResponse<String>> {
    match manager.resume_schedule(&id).await {
        Ok(_) => Json(ApiResponse::success("Schedule resumed".to_string())),
        Err(e) => Json(ApiResponse::error(e.to_string())),
    }
}

pub async fn delete_schedule(
    State(manager): State<Arc<ProcessManager>>,
    Path(id): Path<String>,
) -> Json<ApiResponse<String>> {
    match manager.delete_schedule(&id).await {
        Ok(_) => Json(ApiResponse::success("Schedule deleted".to_string())),
        Err(e) => Json(ApiResponse::error(e.to_string())),
    }
}

#[derive(Deserialize)]
pub struct UpdateScheduleRequest {
    pub name: Option<String>,
    pub description: Option<String>,
    pub schedule_type: Option<String>,
    pub schedule_value: Option<String>,
    pub action: Option<String>,
    pub process_name: Option<String>,
    pub command: Option<String>,
    pub args: Option<Vec<String>>,
    pub enabled: Option<bool>,
}

pub async fn update_schedule(
    State(manager): State<Arc<ProcessManager>>,
    Path(id): Path<String>,
    Json(req): Json<UpdateScheduleRequest>,
) -> Json<ApiResponse<String>> {
    // Get existing schedule
    let existing = match manager.get_schedule(&id).await {
        Ok(Some(s)) => s,
        Ok(None) => return Json(ApiResponse::error(format!("Schedule '{}' not found", id))),
        Err(e) => return Json(ApiResponse::error(e.to_string())),
    };

    use rspm_common::schedule::{ScheduleAction, ScheduleType};

    let schedule_type = match req.schedule_type.as_deref() {
        Some("cron") => {
            if let Some(value) = &req.schedule_value {
                ScheduleType::Cron(value.clone())
            } else {
                existing.config.schedule.clone()
            }
        }
        Some("interval") => {
            if let Some(value) = &req.schedule_value {
                let secs: u64 = value.parse().unwrap_or(60);
                ScheduleType::Interval(secs)
            } else {
                existing.config.schedule.clone()
            }
        }
        Some("once") => {
            if let Some(value) = &req.schedule_value {
                match chrono::DateTime::parse_from_rfc3339(value) {
                    Ok(dt) => ScheduleType::Once(dt.with_timezone(&chrono::Utc)),
                    Err(_) => match chrono::DateTime::parse_from_str(value, "%Y-%m-%d %H:%M:%S") {
                        Ok(dt) => ScheduleType::Once(dt.with_timezone(&chrono::Utc)),
                        Err(_) => existing.config.schedule.clone(),
                    },
                }
            } else {
                existing.config.schedule.clone()
            }
        }
        None => existing.config.schedule.clone(),
        _ => existing.config.schedule.clone(),
    };

    let action = match req.action.as_deref() {
        Some("start") => ScheduleAction::Start,
        Some("stop") => ScheduleAction::Stop,
        Some("restart") => ScheduleAction::Restart,
        Some("execute") => {
            let command = req.command.unwrap_or_default();
            let args = req.args.unwrap_or_default();
            ScheduleAction::Execute { command, args }
        }
        _ => match &existing.config.action {
            ScheduleAction::Execute { command, args } => ScheduleAction::Execute {
                command: req.command.clone().unwrap_or_else(|| command.clone()),
                args: req.args.clone().unwrap_or_else(|| args.clone()),
            },
            _ => existing.config.action.clone(),
        },
    };

    let config = rspm_common::ScheduleConfig {
        id: Some(id.clone()),
        name: req.name.unwrap_or(existing.config.name),
        process_name: req.process_name.or(existing.config.process_name),
        schedule: schedule_type,
        action,
        enabled: req.enabled.unwrap_or(existing.config.enabled),
        timezone: existing.config.timezone.clone(),
        max_runs: existing.config.max_runs,
        description: req.description.or(existing.config.description),
    };

    match manager.update_schedule(&id, config).await {
        Ok(_) => Json(ApiResponse::success("Schedule updated".to_string())),
        Err(e) => Json(ApiResponse::error(e.to_string())),
    }
}

// ============ Batch Schedule Operations ============

#[derive(Deserialize)]
pub struct BatchScheduleIdsRequest {
    pub ids: Vec<String>,
}

pub async fn batch_pause_schedules(
    State(manager): State<Arc<ProcessManager>>,
    Json(req): Json<BatchScheduleIdsRequest>,
) -> Json<ApiResponse<BatchOperationResult>> {
    let mut success_count = 0;
    let mut failed: Vec<String> = Vec::new();

    for id in req.ids {
        match manager.pause_schedule(&id).await {
            Ok(_) => success_count += 1,
            Err(e) => failed.push(format!("{}: {}", id, e)),
        }
    }

    let failed_count = failed.len();
    if failed.is_empty() {
        Json(ApiResponse::success(BatchOperationResult {
            success_count,
            failed_count: 0,
            failed_ids: vec![],
            message: format!("Successfully paused {} schedules", success_count),
        }))
    } else {
        Json(ApiResponse::success(BatchOperationResult {
            success_count,
            failed_count,
            failed_ids: failed,
            message: format!(
                "Paused {} schedules, {} failed",
                success_count, failed_count
            ),
        }))
    }
}

pub async fn batch_resume_schedules(
    State(manager): State<Arc<ProcessManager>>,
    Json(req): Json<BatchScheduleIdsRequest>,
) -> Json<ApiResponse<BatchOperationResult>> {
    let mut success_count = 0;
    let mut failed: Vec<String> = Vec::new();

    for id in req.ids {
        match manager.resume_schedule(&id).await {
            Ok(_) => success_count += 1,
            Err(e) => failed.push(format!("{}: {}", id, e)),
        }
    }

    let failed_count = failed.len();
    if failed.is_empty() {
        Json(ApiResponse::success(BatchOperationResult {
            success_count,
            failed_count: 0,
            failed_ids: vec![],
            message: format!("Successfully resumed {} schedules", success_count),
        }))
    } else {
        Json(ApiResponse::success(BatchOperationResult {
            success_count,
            failed_count,
            failed_ids: failed,
            message: format!(
                "Resumed {} schedules, {} failed",
                success_count, failed_count
            ),
        }))
    }
}

pub async fn batch_delete_schedules(
    State(manager): State<Arc<ProcessManager>>,
    Json(req): Json<BatchScheduleIdsRequest>,
) -> Json<ApiResponse<BatchOperationResult>> {
    let mut success_count = 0;
    let mut failed: Vec<String> = Vec::new();

    for id in req.ids {
        match manager.delete_schedule(&id).await {
            Ok(_) => success_count += 1,
            Err(e) => failed.push(format!("{}: {}", id, e)),
        }
    }

    let failed_count = failed.len();
    if failed.is_empty() {
        Json(ApiResponse::success(BatchOperationResult {
            success_count,
            failed_count: 0,
            failed_ids: vec![],
            message: format!("Successfully deleted {} schedules", success_count),
        }))
    } else {
        Json(ApiResponse::success(BatchOperationResult {
            success_count,
            failed_count,
            failed_ids: failed,
            message: format!(
                "Deleted {} schedules, {} failed",
                success_count, failed_count
            ),
        }))
    }
}

#[derive(Serialize)]
pub struct BatchOperationResult {
    pub success_count: u32,
    pub failed_count: usize,
    pub failed_ids: Vec<String>,
    pub message: String,
}

#[derive(Deserialize)]
pub struct CreateScheduleRequest {
    pub name: String,
    pub description: Option<String>,
    pub schedule_type: String,  // "cron", "interval", "once"
    pub schedule_value: String, // cron expression, interval seconds, or datetime
    pub action: String,         // "start", "stop", "restart", "execute"
    pub process_name: Option<String>,
    pub command: Option<String>,
    pub args: Option<Vec<String>>,
    pub enabled: Option<bool>,
}

pub async fn create_schedule(
    State(manager): State<Arc<ProcessManager>>,
    Json(req): Json<CreateScheduleRequest>,
) -> Json<ApiResponse<String>> {
    use rspm_common::schedule::{ScheduleAction, ScheduleType};

    let schedule_type = match req.schedule_type.as_str() {
        "cron" => ScheduleType::Cron(req.schedule_value),
        "interval" => {
            let secs: u64 = req.schedule_value.parse().unwrap_or(60);
            ScheduleType::Interval(secs)
        }
        "once" => match chrono::DateTime::parse_from_rfc3339(&req.schedule_value) {
            Ok(dt) => ScheduleType::Once(dt.with_timezone(&chrono::Utc)),
            Err(_) => {
                match chrono::DateTime::parse_from_str(&req.schedule_value, "%Y-%m-%d %H:%M:%S") {
                    Ok(dt) => ScheduleType::Once(dt.with_timezone(&chrono::Utc)),
                    Err(e) => {
                        return Json(ApiResponse::error(format!(
                            "Invalid datetime format: {}",
                            e
                        )));
                    }
                }
            }
        },
        _ => return Json(ApiResponse::error("Invalid schedule type".to_string())),
    };

    let action = match req.action.as_str() {
        "start" => ScheduleAction::Start,
        "stop" => ScheduleAction::Stop,
        "restart" => ScheduleAction::Restart,
        "execute" => {
            let command = req.command.unwrap_or_default();
            let args = req.args.unwrap_or_default();
            ScheduleAction::Execute { command, args }
        }
        _ => return Json(ApiResponse::error("Invalid action type".to_string())),
    };

    let config = rspm_common::ScheduleConfig {
        id: None,
        name: req.name,
        process_name: req.process_name,
        schedule: schedule_type,
        action,
        enabled: req.enabled.unwrap_or(true),
        timezone: "UTC".to_string(),
        max_runs: 0,
        description: req.description,
    };

    match manager.create_schedule(config).await {
        Ok(id) => Json(ApiResponse::success(id)),
        Err(e) => Json(ApiResponse::error(e.to_string())),
    }
}
