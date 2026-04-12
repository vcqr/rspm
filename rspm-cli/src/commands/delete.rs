use crate::client::create_client;
use colored::Colorize;
use rspm_common::Result;

/// Check if the identifier is a numeric ID or a process name
fn is_numeric_id(id: &str) -> bool {
    id.chars().all(|c| c.is_ascii_digit())
}

pub async fn delete(id: &str) -> Result<()> {
    let mut client = create_client().await?;

    let target = if is_numeric_id(id) {
        println!("Deleting process by ID '{}'...", id);
        id.to_string()
    } else {
        println!("Deleting process by name '{}'...", id);
        id.to_string()
    };

    match client.delete_process(&target).await {
        Ok(()) => {
            println!("{}", format!("Process '{}' deleted", id).green());
        }
        Err(e) => {
            eprintln!("{}", format!("Failed to delete process: {}", e).red());
            return Err(e);
        }
    }

    Ok(())
}

/// Delete all processes
pub async fn delete_all() -> Result<()> {
    let mut client = create_client().await?;

    let processes = client.list_processes(None).await?;

    if processes.is_empty() {
        println!("No processes to delete");
        return Ok(());
    }

    let count = processes.len();
    for p in processes {
        match client.delete_process(&p.id).await {
            Ok(()) => {
                let id_short = if p.id.len() > 8 { &p.id[..8] } else { &p.id };
                println!(
                    "{} Deleted process: {} (ID: {})",
                    "✓".green(),
                    p.name.cyan(),
                    id_short.dimmed()
                );
            }
            Err(e) => {
                eprintln!("{} Failed to delete process '{}': {}", "✗".red(), p.name, e);
            }
        }
    }

    println!(
        "\n{} Deleted {} process(es)",
        "✓".green(),
        count.to_string().yellow()
    );
    Ok(())
}
