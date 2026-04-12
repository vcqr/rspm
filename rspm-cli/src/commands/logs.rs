use crate::client::create_client;
use chrono::TimeZone;
use rspm_common::Result;
use tokio_stream::StreamExt;

/// Check if the identifier is a numeric ID or a process name
fn is_numeric_id(id: &str) -> bool {
    id.chars().all(|c| c.is_ascii_digit())
}

pub async fn logs(id: &str, follow: bool, lines: u32, stderr: bool) -> Result<()> {
    let mut client = create_client().await?;

    let lookup_type = if is_numeric_id(id) { "ID" } else { "name" };

    // Check if process exists
    match client.get_process(id).await? {
        Some(process) => {
            println!(
                "Viewing logs for process '{}' ({}: {})",
                process.name, lookup_type, id
            );
            if follow {
                println!("Press Ctrl+C to exit");
            }
            println!();

            // Stream logs from daemon
            let mut stream = client.stream_logs(id, follow, lines, stderr).await?;

            while let Some(result) = stream.next().await {
                match result {
                    Ok(entry) => {
                        let timestamp = match chrono::Utc.timestamp_millis_opt(entry.timestamp) {
                            chrono::LocalResult::Single(dt) => dt,
                            _ => chrono::Utc::now(),
                        };
                        let time_str = timestamp.format("%Y-%m-%d %H:%M:%S%.3f");

                        if entry.is_error {
                            eprintln!("[{}] {}", time_str, entry.message);
                        } else {
                            println!("[{}] {}", time_str, entry.message);
                        }
                    }
                    Err(e) => {
                        eprintln!("Error receiving log: {}", e);
                        break;
                    }
                }
            }
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
