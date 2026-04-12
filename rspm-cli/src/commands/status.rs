use crate::client::GrpcClient;
use colored::Colorize;
use rspm_common::utils::format_duration;
use rspm_common::{DaemonConfig, Result};

/// Calculate the visible width of a string (excluding ANSI codes)
fn visible_width(s: &str) -> usize {
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
    result.chars().count()
}

pub async fn status() -> Result<()> {
    // Get the actual gRPC address from config
    let config = DaemonConfig::load_default();
    let addr = config.get_grpc_addr();

    // Try to connect directly without auto-starting the daemon
    let mut client = match GrpcClient::connect(&addr).await {
        Ok(c) => c,
        Err(_) => {
            eprintln!();
            eprintln!("{}", colored::Colorize::red("Daemon is not running."));
            eprintln!("Start it with: rspm start-daemon");
            eprintln!();
            return Ok(());
        }
    };

    match client.get_daemon_status().await {
        Ok(status) => {
            // Collect all label-value pairs for dynamic width calculation
            let mut rows: Vec<(String, String)> = Vec::new();

            let status_str = if status.running {
                "running".green().to_string()
            } else {
                "stopped".red().to_string()
            };

            rows.push(("Status:".to_string(), status_str));
            rows.push(("Version:".to_string(), status.version));
            rows.push((
                "Uptime:".to_string(),
                format_duration(std::time::Duration::from_millis(status.uptime_ms as u64)),
            ));
            rows.push(("Processes:".to_string(), status.total_processes.to_string()));

            // Calculate maximum label width and value width
            let max_label_width = rows
                .iter()
                .map(|(label, _)| visible_width(label))
                .max()
                .unwrap_or(10);
            let max_value_width = rows
                .iter()
                .map(|(_, value)| visible_width(value))
                .max()
                .unwrap_or(10);

            // Add padding for aesthetics
            let label_width = max_label_width.max(10);
            let value_width = max_value_width.max(20);

            // Calculate total table width: │(1) + space(1) + label + space(1) + value + │(1)
            let total_width = 1 + 1 + label_width + 1 + value_width + 1;

            // Render the table
            println!();
            println!("┌{}┐", "─".repeat(total_width - 2));
            println!("│ Daemon Status{}│", " ".repeat(total_width - 16));
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
        Err(e) => {
            eprintln!();
            eprintln!("{}", colored::Colorize::red("Daemon is not running."));
            eprintln!("Start it with: rspm start-daemon");
            eprintln!("Error: {}", e);
            eprintln!();
        }
    }

    Ok(())
}
