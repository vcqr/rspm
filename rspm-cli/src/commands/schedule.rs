use colored::Colorize;
use colored::CustomColor;
use rspm_common::Table;
use rspm_common::{
    Result, RspmError, ScheduleAction, ScheduleConfig, ScheduleStatus, ScheduleType,
};

use crate::client::create_client;

/// Create a new schedule
pub async fn create(
    name: String,
    process: Option<String>,
    cron: Option<String>,
    interval: Option<u64>,
    once: Option<String>,
    action: String,
    _command: Option<String>,
    _args: Vec<String>,
    trailing_args: Vec<String>,
    timezone: String,
    max_runs: u32,
    description: Option<String>,
    disabled: bool,
) -> Result<()> {
    let mut client = create_client().await?;

    // Determine schedule type
    let schedule = if let Some(cron_expr) = cron {
        // Validate cron expression
        if let Err(e) = ScheduleConfig::validate_cron(&cron_expr) {
            return Err(RspmError::InvalidConfig(format!(
                "Invalid cron expression: {}",
                e
            )));
        }
        ScheduleType::Cron(cron_expr)
    } else if let Some(secs) = interval {
        ScheduleType::Interval(secs)
    } else if let Some(once_time) = once {
        let dt = chrono::DateTime::parse_from_rfc3339(&once_time)
            .map_err(|e| RspmError::InvalidConfig(format!("Invalid datetime: {}", e)))?
            .with_timezone(&chrono::Utc);
        ScheduleType::Once(dt)
    } else {
        return Err(RspmError::InvalidConfig(
            "Either --cron, --interval, or --once must be specified".to_string(),
        ));
    };

    // Determine action
    let action = match action.as_str() {
        "start" => ScheduleAction::Start,
        "stop" => ScheduleAction::Stop,
        "restart" => ScheduleAction::Restart,
        "execute" => {
            // 优先使用 trailing_args (-- 后面的参数)
            let (cmd, args) = if !trailing_args.is_empty() {
                // trailing_args[0] 是命令，后面是参数
                let cmd = trailing_args[0].clone();
                let args = trailing_args[1..].to_vec();
                (cmd, args)
            } else if let Some(cmd) = _command {
                // 使用 --command 和 --args
                (cmd, _args)
            } else {
                return Err(RspmError::InvalidConfig(
                    "For execute action, either use '--command' or provide command after '--'"
                        .to_string(),
                ));
            };
            ScheduleAction::Execute { command: cmd, args }
        }
        _ => {
            return Err(RspmError::InvalidConfig(format!(
                "Invalid action: {}. Must be start, stop, restart, or execute",
                action
            )));
        }
    };

    // Validate process name for process actions
    if matches!(
        action,
        ScheduleAction::Start | ScheduleAction::Stop | ScheduleAction::Restart
    ) && process.is_none()
    {
        return Err(RspmError::InvalidConfig(format!(
            "--process is required for {:?} action",
            action
        )));
    }

    let config = ScheduleConfig {
        id: None,
        name: name.clone(),
        process_name: process,
        schedule,
        action,
        enabled: !disabled,
        timezone,
        max_runs,
        description,
    };

    let id = client
        .create_schedule(&config)
        .await
        .map_err(|e| RspmError::SchedulerError(e.to_string()))?;

    println!(
        "{} Created schedule '{}' with ID: {}",
        "✓".green(),
        name.cyan(),
        id.yellow()
    );

    // Show next run time if available
    if let Ok(Some(info)) = client.get_schedule(&id).await
        && let Some(next) = info.next_run
    {
        println!("  Next run: {}", next.to_rfc3339().dimmed());
    }

    Ok(())
}

/// List all schedules
pub async fn list(name_filter: Option<String>) -> Result<()> {
    let mut client = create_client().await?;

    let schedules = client
        .list_schedules()
        .await
        .map_err(|e| RspmError::SchedulerError(e.to_string()))?;

    // 创建表格
    let mut table = Table::new();

    // 设置表头
    let headers: Vec<String> = vec![
        "ID".to_string(),
        "Name".to_string(),
        "Schedule".to_string(),
        "Action".to_string(),
        "Status".to_string(),
        "Next Run".to_string(),
        "Success/Fail/Total".to_string(),
    ];
    table.set_header(&headers);

    if schedules.is_empty() {
        println!("\r\n{}", table.render());
        println!("No schedules found.");
        return Ok(());
    }

    // Filter by name if specified
    let schedules: Vec<_> = schedules
        .into_iter()
        .filter(|s| {
            if let Some(ref filter) = name_filter {
                s.config.name.contains(filter)
            } else {
                true
            }
        })
        .collect();

    if schedules.is_empty() {
        println!("\r\n{}", table.render());
        println!("No schedules found matching '{}'", name_filter.unwrap());
        return Ok(());
    }

    // 添加数据行
    for (row_idx, info) in schedules.iter().enumerate() {
        let schedule_str = match &info.config.schedule {
            ScheduleType::Cron(expr) => format!("cron: {}", expr),
            ScheduleType::Interval(secs) => format!("every {}s", secs),
            ScheduleType::Once(dt) => format!("once: {}", dt.format("%Y-%m-%d %H:%M:%S")),
        };

        let action_str = format!("{:?}", info.config.action).to_lowercase();
        let status_str = format_status(info.status.clone());
        let status_color = get_status_color(info.status.clone());

        let next_run_str = info
            .next_run
            .map(|dt| dt.format("%Y-%m-%d %H:%M:%S").to_string())
            .unwrap_or_else(|| "-".to_string());

        let runs_str = format!(
            "{}/{}/{}",
            info.success_count, info.fail_count, info.run_count
        );

        let row: Vec<String> = vec![
            info.id[..8].to_string(),
            info.config.name.clone(),
            schedule_str,
            action_str,
            status_str,
            next_run_str,
            runs_str,
        ];
        table.add_row(&row);

        // 设置状态列的颜色（第5列，索引4）
        table.set_cell_colors(row_idx, 4, status_color);
    }

    println!("\r\n{}", table.render());
    Ok(())
}

fn get_status_color(status: ScheduleStatus) -> CustomColor {
    match status {
        ScheduleStatus::Active => CustomColor::new(0, 255, 0), // 绿色
        ScheduleStatus::Paused => CustomColor::new(255, 255, 0), // 黄色
        ScheduleStatus::Completed => CustomColor::new(0, 150, 255), // 蓝色
        ScheduleStatus::Error(_) => CustomColor::new(255, 0, 0), // 红色
    }
}

fn format_status(status: ScheduleStatus) -> String {
    match status {
        ScheduleStatus::Active => "active".to_string(),
        ScheduleStatus::Paused => "paused".to_string(),
        ScheduleStatus::Completed => "completed".to_string(),
        ScheduleStatus::Error(_) => "error".to_string(),
    }
}

/// Show schedule details
pub async fn show(id: &str) -> Result<()> {
    let mut client = create_client().await?;

    // Try to find schedule by ID (full or partial) or name
    let info = if let Ok(Some(info)) = client.get_schedule(id).await {
        info
    } else {
        // Try to find by name or partial ID
        let schedules = client
            .list_schedules()
            .await
            .map_err(|e| RspmError::SchedulerError(e.to_string()))?;
        schedules
            .into_iter()
            .find(|s| s.config.name == id || s.id.starts_with(id))
            .ok_or_else(|| RspmError::NotFound(format!("Schedule not found: {}", id)))?
    };

    println!("{}", "Schedule Details".bold().underline());
    println!("  ID:          {}", info.id.cyan());
    println!("  Name:        {}", info.config.name.bold());
    println!(
        "  Status:      {}",
        format!("{:?}", info.status).to_lowercase()
    );

    match &info.config.schedule {
        ScheduleType::Cron(expr) => {
            println!("  Type:        cron");
            println!("  Expression:  {}", expr.yellow());
        }
        ScheduleType::Interval(secs) => {
            println!("  Type:        interval");
            println!("  Interval:    {} seconds", secs);
        }
        ScheduleType::Once(dt) => {
            println!("  Type:        once");
            println!("  Time:        {}", dt.to_rfc3339().yellow());
        }
    }

    println!("  Action:      {:?}", info.config.action);
    if let Some(ref process) = info.config.process_name {
        println!("  Process:     {}", process.cyan());
    }
    println!("  Timezone:    {}", info.config.timezone);
    println!(
        "  Max Runs:    {}",
        if info.config.max_runs == 0 {
            "unlimited".to_string()
        } else {
            info.config.max_runs.to_string()
        }
    );
    println!(
        "  Enabled:     {}",
        if info.config.enabled {
            "yes".green()
        } else {
            "no".red()
        }
    );

    if let Some(ref desc) = info.config.description {
        println!("  Description: {}", desc);
    }

    println!("\n{}", "Statistics".bold().underline());
    println!(
        "  Created:     {}",
        info.created_at.format("%Y-%m-%d %H:%M:%S")
    );
    println!(
        "  Updated:     {}",
        info.updated_at.format("%Y-%m-%d %H:%M:%S")
    );
    println!(
        "  Last Run:    {}",
        info.last_run
            .map(|dt| dt.format("%Y-%m-%d %H:%M:%S").to_string())
            .unwrap_or_else(|| "never".to_string())
    );
    println!(
        "  Next Run:    {}",
        info.next_run
            .map(|dt| dt.format("%Y-%m-%d %H:%M:%S").to_string())
            .unwrap_or_else(|| "-".to_string())
    );
    println!(
        "  Run Count:   {} ({} success, {} fail)",
        info.run_count, info.success_count, info.fail_count
    );

    Ok(())
}

/// Delete a schedule
pub async fn delete(id: &str) -> Result<()> {
    let mut client = create_client().await?;

    // Try to find schedule by ID (full or partial) or name
    let schedule_id = if let Ok(Some(info)) = client.get_schedule(id).await {
        info.id
    } else {
        // Try to find by name or partial ID
        let schedules = client
            .list_schedules()
            .await
            .map_err(|e| RspmError::SchedulerError(e.to_string()))?;
        schedules
            .into_iter()
            .find(|s| s.config.name == id || s.id.starts_with(id))
            .map(|s| s.id)
            .ok_or_else(|| RspmError::NotFound(format!("Schedule not found: {}", id)))?
    };

    client
        .delete_schedule(&schedule_id)
        .await
        .map_err(|e| RspmError::SchedulerError(e.to_string()))?;

    println!("{} Deleted schedule: {}", "✓".green(), id.cyan());
    Ok(())
}

/// Delete all schedules
pub async fn delete_all() -> Result<()> {
    let mut client = create_client().await?;

    let schedules = client
        .list_schedules()
        .await
        .map_err(|e| RspmError::SchedulerError(e.to_string()))?;

    if schedules.is_empty() {
        println!("No schedules to delete");
        return Ok(());
    }

    let count = schedules.len();
    for info in schedules {
        client
            .delete_schedule(&info.id)
            .await
            .map_err(|e| RspmError::SchedulerError(e.to_string()))?;
        println!(
            "{} Deleted schedule: {} ({})",
            "✓".green(),
            info.config.name.cyan(),
            info.id[..8].to_string().dimmed()
        );
    }

    println!(
        "\n{} Deleted {} schedule(s)",
        "✓".green(),
        count.to_string().yellow()
    );
    Ok(())
}

/// Pause a schedule
pub async fn pause(id: &str) -> Result<()> {
    let mut client = create_client().await?;

    // Try to find schedule by ID (full or partial) or name
    let schedule_id = if let Ok(Some(info)) = client.get_schedule(id).await {
        info.id
    } else {
        // Try to find by name or partial ID
        let schedules = client
            .list_schedules()
            .await
            .map_err(|e| RspmError::SchedulerError(e.to_string()))?;
        schedules
            .into_iter()
            .find(|s| s.config.name == id || s.id.starts_with(id))
            .map(|s| s.id)
            .ok_or_else(|| RspmError::NotFound(format!("Schedule not found: {}", id)))?
    };

    client
        .pause_schedule(&schedule_id)
        .await
        .map_err(|e| RspmError::SchedulerError(e.to_string()))?;

    println!("{} Paused schedule: {}", "✓".green(), id.cyan());
    Ok(())
}

/// Resume a schedule
pub async fn resume(id: &str) -> Result<()> {
    let mut client = create_client().await?;

    // Try to find schedule by ID (full or partial) or name
    let schedule_id = if let Ok(Some(info)) = client.get_schedule(id).await {
        info.id
    } else {
        // Try to find by name or partial ID
        let schedules = client
            .list_schedules()
            .await
            .map_err(|e| RspmError::SchedulerError(e.to_string()))?;
        schedules
            .into_iter()
            .find(|s| s.config.name == id || s.id.starts_with(id))
            .map(|s| s.id)
            .ok_or_else(|| RspmError::NotFound(format!("Schedule not found: {}", id)))?
    };

    client
        .resume_schedule(&schedule_id)
        .await
        .map_err(|e| RspmError::SchedulerError(e.to_string()))?;

    println!("{} Resumed schedule: {}", "✓".green(), id.cyan());
    Ok(())
}

/// Show execution history
pub async fn history(id: &str, limit: u32) -> Result<()> {
    let mut client = create_client().await?;

    // Try to find schedule by ID (full or partial) or name
    let schedule_id = if let Ok(Some(info)) = client.get_schedule(id).await {
        info.id
    } else {
        // Try to find by name or partial ID
        let schedules = client
            .list_schedules()
            .await
            .map_err(|e| RspmError::SchedulerError(e.to_string()))?;
        schedules
            .into_iter()
            .find(|s| s.config.name == id || s.id.starts_with(id))
            .map(|s| s.id)
            .ok_or_else(|| RspmError::NotFound(format!("Schedule not found: {}", id)))?
    };

    let executions = client
        .get_schedule_executions(&schedule_id, limit)
        .await
        .map_err(|e| RspmError::SchedulerError(e.to_string()))?;

    if executions.is_empty() {
        println!("No execution history found for schedule: {}", id);
        return Ok(());
    }

    println!(
        "{}",
        format!("Execution History for {}", id).bold().underline()
    );

    let mut table = Table::new();
    table.set_header(&vec![
        "ID".to_string(),
        "Started At".to_string(),
        "Duration".to_string(),
        "Status".to_string(),
        "Output".to_string(),
    ]);

    for exec in executions {
        let duration = exec
            .ended_at
            .map(|end| {
                let dur = end - exec.started_at;
                format!(
                    "{}.{:03}s",
                    dur.num_seconds(),
                    dur.num_milliseconds() % 1000
                )
            })
            .unwrap_or_else(|| "-".to_string());

        let status_str = format!("{:?}", exec.status).to_lowercase();
        let status_colored = match exec.status {
            rspm_common::ExecutionStatus::Success => status_str.green(),
            rspm_common::ExecutionStatus::Failed => status_str.red(),
            rspm_common::ExecutionStatus::Timeout => status_str.yellow(),
            rspm_common::ExecutionStatus::Running => status_str.blue(),
        };

        let output_preview = exec
            .output
            .map(|o| {
                if o.len() > 30 {
                    format!("{}...", &o[..30])
                } else {
                    o
                }
            })
            .unwrap_or_else(|| "-".to_string());

        table.add_row(&vec![
            exec.id[..8].to_string(),
            exec.started_at.format("%Y-%m-%d %H:%M:%S").to_string(),
            duration,
            status_colored.to_string(),
            output_preview,
        ]);
    }

    println!("\r\n{}", table.render());
    Ok(())
}
