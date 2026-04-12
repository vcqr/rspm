use crate::client::create_client;
use rspm_common::Result;

/// Check if the identifier is a numeric ID or a process name
fn is_numeric_id(id: &str) -> bool {
    id.chars().all(|c| c.is_ascii_digit())
}

pub async fn stop(id: &str, force: bool) -> Result<()> {
    let mut client = create_client().await?;

    let target = if is_numeric_id(id) {
        println!("Stopping process by ID '{}'...", id);
        id.to_string()
    } else {
        println!("Stopping process by name '{}'...", id);
        id.to_string()
    };

    match client.stop_process(&target, force).await {
        Ok(()) => {
            println!(
                "{}",
                colored::Colorize::green(format!("Process '{}' stopped", id).as_str())
            );
        }
        Err(e) => {
            eprintln!(
                "{}",
                colored::Colorize::red(format!("Failed to stop process: {}", e).as_str())
            );
            return Err(e);
        }
    }

    Ok(())
}
