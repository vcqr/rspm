use crate::client::create_client;
use rspm_common::{Result, load_config};

pub async fn load(file: &str, dry_run: bool) -> Result<()> {
    // Load configurations from file
    let configs = match load_config(file) {
        Ok(c) => c,
        Err(e) => {
            eprintln!(
                "{}",
                colored::Colorize::red(format!("Failed to load config file: {}", e).as_str())
            );
            return Err(e);
        }
    };

    println!(
        "Loaded {} process configuration(s) from '{}'",
        configs.len(),
        file
    );

    // Validate mode - just print configs
    if dry_run {
        println!("\n{}:", colored::Colorize::cyan("Configurations (dry-run)"));
        for (i, config) in configs.iter().enumerate() {
            println!(
                "\n  [{}] {}:",
                i + 1,
                colored::Colorize::yellow(config.name.as_str())
            );
            println!("    command: {}", config.command);
            if !config.args.is_empty() {
                println!("    args: {:?}", config.args);
            }
            println!("    instances: {}", config.instances);
            if let Some(ref cwd) = config.cwd {
                println!("    cwd: {}", cwd);
            }
            if !config.env.is_empty() {
                println!("    env: {:?}", config.env);
            }
            println!("    autorestart: {}", config.autorestart);
            println!("    max_restarts: {}", config.max_restarts);
            if config.max_memory_mb > 0 {
                println!("    max_memory_mb: {}", config.max_memory_mb);
            }
        }
        println!(
            "\n{}",
            colored::Colorize::green("Config validation successful. No processes started.")
        );
        return Ok(());
    }

    // Start all processes
    let mut client = create_client().await?;
    let mut success_count = 0;
    let mut fail_count = 0;

    for config in configs {
        println!("\nStarting process '{}'...", config.name);
        match client.start_process(config.clone()).await {
            Ok(id) => {
                println!(
                    "  {}",
                    colored::Colorize::green(format!("Started with ID: {}", id).as_str())
                );
                success_count += 1;
            }
            Err(e) => {
                eprintln!(
                    "  {}",
                    colored::Colorize::red(format!("Failed: {}", e).as_str())
                );
                fail_count += 1;
            }
        }
    }

    println!(
        "\n{}: {} started, {} failed",
        colored::Colorize::cyan("Summary"),
        colored::Colorize::green(format!("{}", success_count).as_str()),
        colored::Colorize::red(format!("{}", fail_count).as_str())
    );

    if fail_count > 0 {
        return Err(rspm_common::RspmError::StartFailed(format!(
            "{} process(es) failed to start",
            fail_count
        )));
    }

    Ok(())
}
