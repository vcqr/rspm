use crate::client::create_client;
use rspm_common::Result;

/// Check if the identifier is a numeric ID or a process name
fn is_numeric_id(id: &str) -> bool {
    id.chars().all(|c| c.is_ascii_digit())
}

pub async fn restart(id: &str) -> Result<()> {
    let mut client = create_client().await?;

    let target = if is_numeric_id(id) {
        println!("Restarting process by ID '{}'...", id);
        id.to_string()
    } else {
        println!("Restarting process by name '{}'...", id);
        id.to_string()
    };

    match client.restart_process(&target).await {
        Ok(()) => {
            println!(
                "{}",
                colored::Colorize::green(format!("Process '{}' restarted", id).as_str())
            );
        }
        Err(e) => {
            eprintln!(
                "{}",
                colored::Colorize::red(format!("Failed to restart process: {}", e).as_str())
            );
            return Err(e);
        }
    }

    Ok(())
}
