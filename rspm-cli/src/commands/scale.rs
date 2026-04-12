use crate::client::create_client;
use rspm_common::Result;

/// Check if the identifier is a numeric ID or a process name
fn is_numeric_id(id: &str) -> bool {
    id.chars().all(|c| c.is_ascii_digit())
}

pub async fn scale(id: &str, instances: u32) -> Result<()> {
    let mut client = create_client().await?;

    let lookup_type = if is_numeric_id(id) { "ID" } else { "name" };

    println!(
        "Scaling process '{}' ({}: {}) to {} instances...",
        id, lookup_type, id, instances
    );

    match client.scale_process(id, instances).await {
        Ok(new_ids) => {
            if new_ids.is_empty() {
                println!(
                    "{}",
                    colored::Colorize::green(
                        format!("Process scaled to {} instances", instances).as_str()
                    )
                );
            } else {
                println!(
                    "{}",
                    colored::Colorize::green(
                        format!(
                            "Added {} new instance(s): {}",
                            new_ids.len(),
                            new_ids.join(", ")
                        )
                        .as_str()
                    )
                );
            }
        }
        Err(e) => {
            eprintln!(
                "{}",
                colored::Colorize::red(format!("Failed to scale process: {}", e).as_str())
            );
            return Err(e);
        }
    }

    Ok(())
}
