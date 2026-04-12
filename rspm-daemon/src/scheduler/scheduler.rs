use chrono::Utc;
use rspm_common::{
    ExecutionStatus, ScheduleAction, ScheduleExecution, ScheduleInfo, ScheduleType, get_logs_dir,
};
use std::collections::HashMap;
use std::fs::OpenOptions;
use std::io::Write;
use std::sync::Arc;
use tokio::sync::RwLock;
use tokio_cron_scheduler::{Job, JobScheduler};
use tracing::{debug, error, info, warn};
use uuid::Uuid;

use crate::log_watcher::LogWriter;
use crate::manager::{ManagedProcess, StateStore};

/// 定时任务调度器
pub struct ScheduleManager {
    /// 调度器实例
    scheduler: JobScheduler,
    /// 状态存储
    state_store: Arc<StateStore>,
    /// 进程集合
    processes: Arc<RwLock<HashMap<String, ManagedProcess>>>,
    /// 日志写入器
    log_writer: Arc<LogWriter>,
    /// 已注册的任务 ID 映射 (schedule_id -> job_id)
    job_ids: Arc<RwLock<HashMap<String, Uuid>>>,
}

impl ScheduleManager {
    /// 创建新的调度器
    pub async fn new(
        state_store: Arc<StateStore>,
        processes: Arc<RwLock<HashMap<String, ManagedProcess>>>,
        log_writer: Arc<LogWriter>,
    ) -> anyhow::Result<Self> {
        let scheduler = JobScheduler::new().await?;

        Ok(Self {
            scheduler,
            state_store,
            processes,
            log_writer,
            job_ids: Arc::new(RwLock::new(HashMap::new())),
        })
    }

    /// 启动调度器
    pub async fn start(&self) -> anyhow::Result<()> {
        self.scheduler.start().await?;
        info!("Schedule manager started");
        Ok(())
    }

    /// 调度一个定时任务
    pub async fn schedule_job(&self, info: &ScheduleInfo) -> anyhow::Result<()> {
        let schedule_id = info.id.clone();
        let config = info.config.clone();
        let state_store = self.state_store.clone();
        let processes = self.processes.clone();
        let log_writer = self.log_writer.clone();

        // Wrap info in Arc for sharing across async closures
        let info_arc = std::sync::Arc::new(info.clone());

        let job: Job = match &config.schedule {
            ScheduleType::Cron(expr) => {
                // tokio-cron-scheduler 的 CronParser 使用 .seconds(Seconds::Required)
                // 支持 6 字段表达式（秒 分 时 天 月 周），无需转换
                Job::new_async(expr, move |_uuid, _l| {
                    let info_arc = info_arc.clone();
                    let state_store_clone = state_store.clone();
                    let processes_clone = processes.clone();
                    let log_writer_clone = log_writer.clone();

                    Box::pin(async move {
                        execute_job_internal(
                            (*info_arc).clone(),
                            state_store_clone,
                            processes_clone,
                            log_writer_clone,
                        )
                        .await;
                    })
                })?
            }
            ScheduleType::Interval(secs) => {
                let duration = std::time::Duration::from_secs(*secs);
                Job::new_repeated_async(duration, move |_uuid, _l| {
                    let info_arc = info_arc.clone();
                    let state_store_clone = state_store.clone();
                    let processes_clone = processes.clone();
                    let log_writer_clone = log_writer.clone();

                    Box::pin(async move {
                        execute_job_internal(
                            (*info_arc).clone(),
                            state_store_clone,
                            processes_clone,
                            log_writer_clone,
                        )
                        .await;
                    })
                })?
            }
            ScheduleType::Once(dt) => {
                let now = Utc::now();
                if *dt <= now {
                    warn!("Once schedule {} is in the past, skipping", info_arc.id);
                    return Ok(());
                }

                let duration = (*dt - now).to_std()?;
                Job::new_one_shot_async(duration, move |_uuid, _l| {
                    let info_arc = info_arc.clone();
                    let state_store_clone = state_store.clone();
                    let processes_clone = processes.clone();
                    let log_writer_clone = log_writer.clone();

                    Box::pin(async move {
                        execute_job_internal(
                            (*info_arc).clone(),
                            state_store_clone,
                            processes_clone,
                            log_writer_clone,
                        )
                        .await;
                    })
                })?
            }
        };

        let job_id = job.guid();
        self.scheduler.add(job).await?;

        // 记录 job_id
        let mut job_ids = self.job_ids.write().await;
        job_ids.insert(schedule_id, job_id);

        debug!("Added job {} for schedule {}", job_id, info.id);

        Ok(())
    }

    /// 取消一个定时任务
    pub async fn cancel_job(&self, schedule_id: &str) -> anyhow::Result<()> {
        let mut job_ids = self.job_ids.write().await;

        if let Some(job_id) = job_ids.remove(schedule_id) {
            self.scheduler.remove(&job_id).await?;
            debug!("Removed job {} for schedule {}", job_id, schedule_id);
        }

        Ok(())
    }

    /// 停止调度器
    pub async fn stop(mut self) -> anyhow::Result<()> {
        self.scheduler.shutdown().await?;
        info!("Schedule manager stopped");
        Ok(())
    }
}

/// 读取进程最近的日志输出
async fn read_process_logs(
    process_id: &str,
    processes: Arc<RwLock<HashMap<String, ManagedProcess>>>,
) -> String {
    let procs = processes.read().await;

    if let Some(proc) = procs.get(process_id) {
        if let Some(ref log_writer) = proc.log_writer {
            let mut output = String::new();

            // 读取最近的 stdout 日志（最后10行）
            match log_writer.read_stdout_history(10) {
                Ok(entries) if !entries.is_empty() => {
                    output.push_str("Recent stdout:\n");
                    for entry in entries {
                        output.push_str(&format!(
                            "  [{}] {}\n",
                            entry.timestamp.format("%H:%M:%S"),
                            entry.message
                        ));
                    }
                }
                _ => {}
            }

            // 读取最近的 stderr 日志（最后10行）
            match log_writer.read_stderr_history(10) {
                Ok(entries) if !entries.is_empty() => {
                    if !output.is_empty() {
                        output.push('\n');
                    }
                    output.push_str("Recent stderr:\n");
                    for entry in entries {
                        output.push_str(&format!(
                            "  [{}] {}\n",
                            entry.timestamp.format("%H:%M:%S"),
                            entry.message
                        ));
                    }
                }
                _ => {}
            }

            if output.is_empty() {
                "(No recent log output)".to_string()
            } else {
                output
            }
        } else {
            "(Log writer not available)".to_string()
        }
    } else {
        "(Process not found)".to_string()
    }
}

/// 内部执行函数
async fn execute_job_internal(
    info: ScheduleInfo,
    state_store: Arc<StateStore>,
    processes: Arc<RwLock<HashMap<String, ManagedProcess>>>,
    log_writer: Arc<LogWriter>,
) {
    let schedule_id = info.id.clone();
    let started_at = Utc::now();

    info!("Executing schedule: {} ({})", info.config.name, schedule_id);

    // 创建执行记录
    let execution_id = Uuid::new_v4().to_string();
    let execution = ScheduleExecution {
        id: execution_id.clone(),
        schedule_id: schedule_id.clone(),
        started_at,
        ended_at: None,
        status: ExecutionStatus::Running,
        output: None,
        error: None,
    };

    if let Err(e) = state_store.record_execution(&execution).await {
        error!("Failed to record execution start: {}", e);
    }

    // 执行操作
    let result = execute_action(&info, processes).await;

    let ended_at = Utc::now();
    let (status, output, error) = match result {
        Ok(msg) => {
            info!("Schedule {} executed successfully: {}", schedule_id, msg);
            (ExecutionStatus::Success, Some(msg), None)
        }
        Err(e) => {
            error!("Schedule {} execution failed: {}", schedule_id, e);
            (ExecutionStatus::Failed, None, Some(e.to_string()))
        }
    };

    // 更新执行记录
    let execution = ScheduleExecution {
        id: execution_id.clone(),
        schedule_id: schedule_id.clone(),
        started_at,
        ended_at: Some(ended_at),
        status: status.clone(),
        output: output.clone(),
        error: error.clone(),
    };

    if let Err(e) = state_store.record_execution(&execution).await {
        error!("Failed to record execution end: {}", e);
    }

    // 写入执行日志到文件
    if let Err(e) = write_execution_log(&info, &execution, &log_writer).await {
        error!("Failed to write execution log: {}", e);
    }

    // 更新调度器统计信息
    let success = matches!(status, ExecutionStatus::Success);
    let next_run = info.config.next_run(ended_at);

    if let Err(e) = state_store
        .update_schedule_run(&schedule_id, started_at, next_run, success)
        .await
    {
        error!("Failed to update schedule run: {}", e);
    }

    // 检查是否达到最大执行次数
    if info.config.max_runs > 0 && info.run_count + 1 >= info.config.max_runs {
        info!(
            "Schedule {} reached max runs, marking as completed",
            schedule_id
        );
        if let Err(e) = state_store
            .update_schedule_status(&schedule_id, "completed")
            .await
        {
            error!("Failed to update schedule status: {}", e);
        }
    }
}

/// 写入执行日志到文件（追加模式，所有执行记录在一个文件中）
async fn write_execution_log(
    info: &ScheduleInfo,
    execution: &ScheduleExecution,
    _log_writer: &LogWriter,
) -> anyhow::Result<()> {
    // 构建日志目录: ~/.rspm/logs/schedules/
    let mut log_dir = get_logs_dir();
    log_dir.push("schedules");

    // 创建目录
    std::fs::create_dir_all(&log_dir)?;

    // 构建日志文件名: <schedule-name>.log（所有执行记录在一个文件）
    let log_filename = format!("{}.log", info.config.name);
    let log_path = log_dir.join(&log_filename);

    // 构建日志内容
    let mut content = String::new();
    content.push('\n');
    content.push_str(&"=".repeat(80));
    content.push('\n');

    let timestamp = execution.started_at.format("%Y-%m-%d %H:%M:%S");
    content.push_str(&format!("[{}] Schedule Execution\n", timestamp));
    content.push_str(&"=".repeat(80));
    content.push_str("\n\n");

    content.push_str(&format!("Execution ID:   {}\n", execution.id));
    content.push_str(&format!("Action:         {:?}\n", info.config.action));
    if let Some(ref process_name) = info.config.process_name {
        content.push_str(&format!("Process:        {}\n", process_name));
    }
    content.push_str(&format!("Schedule Type:  {}\n", info.config.schedule));

    if let Some(ended_at) = execution.ended_at {
        let duration = ended_at - execution.started_at;
        content.push_str(&format!(
            "Duration:       {}.{:03}s\n",
            duration.num_seconds(),
            duration.num_milliseconds() % 1000
        ));
    }
    content.push_str(&format!("Status:         {:?}\n", execution.status));
    content.push('\n');

    if let Some(ref output) = execution.output {
        content.push_str(&"-".repeat(40));
        content.push('\n');
        content.push_str("OUTPUT:\n");
        content.push_str(&"-".repeat(40));
        content.push('\n');
        content.push_str(output);
        content.push('\n');
    }

    if let Some(ref error) = execution.error {
        content.push_str(&"-".repeat(40));
        content.push('\n');
        content.push_str("ERROR:\n");
        content.push_str(&"-".repeat(40));
        content.push('\n');
        content.push_str(error);
        content.push('\n');
    }

    // 追加写入文件
    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(&log_path)?;

    file.write_all(content.as_bytes())?;
    file.flush()?;

    Ok(())
}

/// 执行具体操作
async fn execute_action(
    info: &ScheduleInfo,
    processes: Arc<RwLock<HashMap<String, ManagedProcess>>>,
) -> anyhow::Result<String> {
    match &info.config.action {
        ScheduleAction::Start => {
            let process_name = info
                .config
                .process_name
                .as_ref()
                .ok_or_else(|| anyhow::anyhow!("Process name not specified"))?;

            // 查找进程 ID
            let process_id = {
                let procs = processes.read().await;
                procs
                    .values()
                    .find(|p| p.info.name == *process_name)
                    .map(|p| p.info.id.clone())
                    .ok_or_else(|| anyhow::anyhow!("Process not found: {}", process_name))?
            };

            // 获取进程的可变引用并启动
            let mut procs = processes.write().await;
            let proc = procs
                .get_mut(&process_id)
                .ok_or_else(|| anyhow::anyhow!("Process not found: {}", process_name))?;

            proc.start(None)
                .await
                .map_err(|e| anyhow::anyhow!("Failed to start process: {}", e))?;

            // 等待日志写入
            tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

            // 读取进程最近的日志
            let log_output = read_process_logs(&process_id, processes.clone()).await;

            Ok(format!("Started process: {}\n{}", process_name, log_output))
        }
        ScheduleAction::Stop => {
            let process_name = info
                .config
                .process_name
                .as_ref()
                .ok_or_else(|| anyhow::anyhow!("Process name not specified"))?;

            // 查找进程 ID
            let process_id = {
                let procs = processes.read().await;
                procs
                    .values()
                    .find(|p| p.info.name == *process_name)
                    .map(|p| p.info.id.clone())
                    .ok_or_else(|| anyhow::anyhow!("Process not found: {}", process_name))?
            };

            // 读取进程停止前的日志
            let log_output = read_process_logs(&process_id, processes.clone()).await;

            // 获取进程的可变引用并停止
            let mut procs = processes.write().await;
            let proc = procs
                .get_mut(&process_id)
                .ok_or_else(|| anyhow::anyhow!("Process not found: {}", process_name))?;

            proc.stop(false)
                .await
                .map_err(|e| anyhow::anyhow!("Failed to stop process: {}", e))?;

            Ok(format!("Stopped process: {}\n{}", process_name, log_output))
        }
        ScheduleAction::Restart => {
            let process_name = info
                .config
                .process_name
                .as_ref()
                .ok_or_else(|| anyhow::anyhow!("Process name not specified"))?;

            // 查找进程 ID
            let process_id = {
                let procs = processes.read().await;
                procs
                    .values()
                    .find(|p| p.info.name == *process_name)
                    .map(|p| p.info.id.clone())
                    .ok_or_else(|| anyhow::anyhow!("Process not found: {}", process_name))?
            };

            // 读取重启前的日志
            let log_output_before = read_process_logs(&process_id, processes.clone()).await;

            // 获取进程的可变引用并重启
            let mut procs = processes.write().await;
            let proc = procs
                .get_mut(&process_id)
                .ok_or_else(|| anyhow::anyhow!("Process not found: {}", process_name))?;

            proc.stop(false)
                .await
                .map_err(|e| anyhow::anyhow!("Failed to stop process: {}", e))?;
            proc.start(None)
                .await
                .map_err(|e| anyhow::anyhow!("Failed to start process: {}", e))?;

            // 等待日志写入
            drop(procs);
            tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

            // 读取重启后的日志
            let log_output_after = read_process_logs(&process_id, processes.clone()).await;

            Ok(format!(
                "Restarted process: {}\n\nBefore restart:\n{}\n\nAfter restart:\n{}",
                process_name, log_output_before, log_output_after
            ))
        }
        ScheduleAction::Execute { command, args } => {
            // 执行自定义命令
            let output = tokio::process::Command::new(command)
                .args(args)
                .output()
                .await?;

            let stdout = String::from_utf8_lossy(&output.stdout);
            let stderr = String::from_utf8_lossy(&output.stderr);

            // 构建完整输出（包含 stdout 和 stderr）
            let mut full_output = String::new();
            if !stdout.is_empty() {
                full_output.push_str(&format!("STDOUT:\n{}\n", stdout));
            }
            if !stderr.is_empty() {
                full_output.push_str(&format!("STDERR:\n{}\n", stderr));
            }

            if output.status.success() {
                Ok(format!("Command executed: {}\n{}", command, full_output))
            } else {
                Err(anyhow::anyhow!(
                    "Command failed (exit code: {})\n{}",
                    output.status.code().unwrap_or(-1),
                    full_output
                ))
            }
        }
    }
}
