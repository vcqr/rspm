use crate::client::create_client;
use colored::Colorize;
use rspm_common::ProcessState;
use rspm_common::Result;
use rspm_common::utils::{format_bytes, format_duration, format_timestamp};

/// Check if the identifier is a numeric ID or a process name
fn is_numeric_id(id: &str) -> bool {
    id.chars().all(|c| c.is_ascii_digit())
}

/// Calculate the visible width of a string (excluding ANSI codes)
fn visible_width(s: &str) -> usize {
    strip_ansi_codes(s).chars().count()
}

/// Strip ANSI escape sequences from a string
fn strip_ansi_codes(s: &str) -> String {
    let mut result = String::new();
    let mut chars = s.chars().peekable();

    while let Some(c) = chars.next() {
        if c == '\x1B' {
            if chars.peek() == Some(&'[') {
                chars.next();
                while let Some(c) = chars.peek() {
                    if c.is_ascii_alphabetic() {
                        chars.next();
                        break;
                    }
                    chars.next();
                }
            }
        } else {
            result.push(c);
        }
    }
    result
}

pub async fn show(id: &str) -> Result<()> {
    let mut client = create_client().await?;

    let lookup_type = if is_numeric_id(id) { "ID" } else { "name" };

    match client.get_process(id).await? {
        Some(process) => {
            // Collect all label-value pairs for dynamic width calculation
            let mut rows: Vec<(String, String)> = Vec::new();

            rows.push(("ID:".to_string(), process.id.clone()));
            rows.push(("Name:".to_string(), process.name.clone()));
            rows.push(("Lookup:".to_string(), format!("{}: {}", lookup_type, id)));
            rows.push(("Status:".to_string(), format_status(process.state)));
            rows.push((
                "PID:".to_string(),
                process.pid.map(|p| p.to_string()).unwrap_or_default(),
            ));
            rows.push(("Command:".to_string(), process.config.command.clone()));
            rows.push(("Arguments:".to_string(), process.config.args.join(" ")));

            if let Some(ref cwd) = process.config.cwd {
                rows.push(("Working Dir:".to_string(), cwd.clone()));
            }

            rows.push((
                "CPU Usage:".to_string(),
                format!("{:.1}%", process.stats.cpu_percent),
            ));
            rows.push((
                "Memory:".to_string(),
                format_bytes(process.stats.memory_bytes),
            ));
            rows.push((
                "Uptime:".to_string(),
                format_duration(std::time::Duration::from_millis(process.uptime_ms)),
            ));
            rows.push(("Restarts:".to_string(), process.restart_count.to_string()));
            rows.push((
                "Instances:".to_string(),
                process.config.instances.to_string(),
            ));

            rows.push((
                "Created:".to_string(),
                format_timestamp(&process.created_at),
            ));

            if let Some(ref started_at) = process.started_at {
                rows.push(("Started:".to_string(), format_timestamp(started_at)));
            }

            // Log files info
            if let Some(ref log_file) = process.config.log_file {
                rows.push(("Log File:".to_string(), log_file.clone()));
            } else {
                let log_dir = format!("~/.rspm/logs/{}/", process.name);
                rows.push(("Log Dir:".to_string(), log_dir));
                rows.push(("  stdout:".to_string(), "out.log".to_string()));
                rows.push(("  stderr:".to_string(), "err.log".to_string()));
            }

            if let Some(ref error) = process.error_message {
                rows.push(("Error:".to_string(), error.clone()));
            }

            // Calculate maximum label width and value width
            let max_label_width = rows
                .iter()
                .map(|(label, _)| visible_width(label))
                .max()
                .unwrap_or(20);
            let max_value_width = rows
                .iter()
                .map(|(_, value)| {
                    // For colored values, calculate visible width
                    visible_width(value)
                })
                .max()
                .unwrap_or(35);

            // Add padding for aesthetics
            let label_width = max_label_width.max(18);
            let value_width = max_value_width.max(35);

            // Calculate total table width: │(1) + space(1) + label + space(1) + value + │(1)
            let total_width = 1 + 1 + label_width + 1 + value_width + 1;

            // Render the table
            println!();
            println!("┌{}┐", "─".repeat(total_width - 2));
            println!("│ Process Details{}│", " ".repeat(total_width - 1 - 17));
            println!("├{}┤", "─".repeat(total_width - 2));

            for (label, value) in rows {
                let visible_label_width = visible_width(&label);
                let visible_value_width = visible_width(&value);

                let label_padding = label_width - visible_label_width;
                let value_padding = value_width - visible_value_width;

                println!(
                    "│ {}{} {}{}│",
                    label,
                    " ".repeat(label_padding),
                    value,
                    " ".repeat(value_padding)
                );
            }

            println!("└{}┘", "─".repeat(total_width - 2));
            println!();
        }
        None => {
            eprintln!(
                "{}",
                colored::Colorize::red(format!("Process '{}' not found", id).as_str())
            );
        }
    }

    Ok(())
}

/// Format status with color
fn format_status(state: ProcessState) -> String {
    match state {
        ProcessState::Running => "running".green().to_string(),
        ProcessState::Starting => "starting".yellow().to_string(),
        ProcessState::Stopped => "stopped".cyan().to_string(),
        ProcessState::Stopping => "stopping".yellow().to_string(),
        ProcessState::Errored => "errored".red().to_string(),
    }
}
