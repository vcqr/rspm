use crate::client::create_client;
use colored::CustomColor;
use rspm_common::utils::{format_bytes, format_duration};
use rspm_common::{ProcessState, Result, Table};

// 状态颜色定义
const COLOR_RUNNING: CustomColor = CustomColor::new(0, 255, 0); // 绿色
const COLOR_STARTING: CustomColor = CustomColor::new(255, 255, 0); // 黄色
const COLOR_STOPPED: CustomColor = CustomColor::new(0, 255, 255); // 青色
const COLOR_STOPPING: CustomColor = CustomColor::new(255, 255, 0); // 黄色
const COLOR_ERRORED: CustomColor = CustomColor::new(255, 0, 0); // 红色

pub async fn list(name_filter: Option<&str>) -> Result<()> {
    let mut client = create_client().await?;

    let processes = client.list_processes(name_filter).await?;

    if processes.is_empty() {
        // 空表格，只显示表头和提示
        let mut table = Table::new();
        let headers: Vec<String> = vec![
            "ID".to_string(),
            "Name".to_string(),
            "Status".to_string(),
            "PID".to_string(),
            "CPU".to_string(),
            "Memory".to_string(),
            "Uptime".to_string(),
            "Restarts".to_string(),
        ];
        table.set_header(&headers);
        println!("\r\n{}", table.render());
        println!("No processes found.");
        return Ok(());
    }

    // 创建表格
    let mut table = Table::new();

    // 设置表头
    let headers: Vec<String> = vec![
        "ID".to_string(),
        "Name".to_string(),
        "Status".to_string(),
        "PID".to_string(),
        "CPU".to_string(),
        "Memory".to_string(),
        "Uptime".to_string(),
        "Restarts".to_string(),
    ];
    table.set_header(&headers);

    // 添加数据行
    for (row_idx, p) in processes.iter().enumerate() {
        let status_color = get_status_color(p.state);
        let status_str = format_status(p.state);

        let row: Vec<String> = vec![
            p.id.clone(),
            p.name.clone(),
            status_str,
            p.pid.map(|pid| pid.to_string()).unwrap_or_default(),
            format!("{:.1}%", p.stats.cpu_percent),
            format_bytes(p.stats.memory_bytes),
            format_duration(std::time::Duration::from_millis(p.uptime_ms)),
            p.restart_count.to_string(),
        ];
        table.add_row(&row);

        // 设置状态列的颜色
        table.set_cell_colors(row_idx, 2, status_color);
    }

    println!("\r\n{}", table.render());

    Ok(())
}

fn get_status_color(state: ProcessState) -> CustomColor {
    match state {
        ProcessState::Running => COLOR_RUNNING,
        ProcessState::Starting => COLOR_STARTING,
        ProcessState::Stopped => COLOR_STOPPED,
        ProcessState::Stopping => COLOR_STOPPING,
        ProcessState::Errored => COLOR_ERRORED,
    }
}

fn format_status(state: ProcessState) -> String {
    match state {
        ProcessState::Running => "running".to_string(),
        ProcessState::Starting => "starting".to_string(),
        ProcessState::Stopped => "stopped".to_string(),
        ProcessState::Stopping => "stopping".to_string(),
        ProcessState::Errored => "errored".to_string(),
    }
}
