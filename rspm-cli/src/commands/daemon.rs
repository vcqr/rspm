use crate::client::create_client;
use rspm_common::{DaemonConfig, Result, get_socket_path};
use std::path::PathBuf;

pub async fn start_daemon(config_path: Option<PathBuf>) -> Result<()> {
    // Check if daemon is already running
    let socket_path = get_socket_path();
    if socket_path.exists() {
        if let Ok(mut client) = create_client().await
            && client.get_daemon_status().await.is_ok()
        {
            println!(
                "{}",
                colored::Colorize::yellow("Daemon is already running.")
            );
            return Ok(());
        }
        // Socket exists but daemon is not running, remove the socket
        let _ = std::fs::remove_file(&socket_path);
    }

    println!("Starting daemon...");

    // Determine config file path
    let config_file = config_path.unwrap_or_else(DaemonConfig::get_default_config_path);

    // Load config to display port info
    let _config = if config_file.exists() {
        println!("Using config file: {}", config_file.display());
        DaemonConfig::from_file(&config_file).unwrap_or_default()
    } else {
        println!(
            "Using default config (port: {})",
            DaemonConfig::default().port
        );
        DaemonConfig::default()
    };

    // Get the path to the daemon binary
    let daemon_path = std::env::current_exe()
        .map(|p| {
            let parent = p.parent().map(|p| p.to_path_buf()).unwrap_or_default();
            parent.join("rspmd")
        })
        .unwrap_or_else(|_| std::path::PathBuf::from("rspmd"));

    // Spawn the daemon process
    let mut cmd = std::process::Command::new(&daemon_path);

    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        cmd.process_group(0);
    }

    // Pass config file path via environment variable
    cmd.env("RSPM_CONFIG_FILE", &config_file)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null());

    match cmd.spawn() {
        Ok(_) => {
            // Wait a bit for the daemon to start
            tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;

            // Check if daemon is now running
            if let Ok(mut client) = create_client().await
                && client.get_daemon_status().await.is_ok()
            {
                println!(
                    "{}",
                    colored::Colorize::green("Daemon started successfully.")
                );
                return Ok(());
            }

            println!(
                "{}",
                colored::Colorize::yellow("Daemon started, but connection could not be verified.")
            );
        }
        Err(e) => {
            eprintln!(
                "{}",
                colored::Colorize::red(format!("Failed to start daemon: {}", e).as_str())
            );
            return Err(rspm_common::RspmError::InternalError(e.to_string()));
        }
    }

    Ok(())
}

pub async fn stop_daemon() -> Result<()> {
    // Create client to check if daemon is running
    let mut client = match create_client().await {
        Ok(c) => c,
        Err(_) => {
            println!("{}", colored::Colorize::yellow("Daemon is not running."));
            return Ok(());
        }
    };

    // Check if daemon is running
    if client.get_daemon_status().await.is_err() {
        println!("{}", colored::Colorize::yellow("Daemon is not running."));
        return Ok(());
    }

    // First stop all processes
    match client.stop_all_processes().await {
        Ok(count) if count > 0 => {
            println!("Stopped {} process(es).", count);
        }
        _ => {}
    }

    // Send stop daemon request
    println!("Stopping daemon...");
    client.stop_daemon().await?;

    // Wait for daemon to actually shutdown
    tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;

    // Verify daemon is stopped
    #[cfg(unix)]
    {
        let socket_path = rspm_common::get_socket_path();
        if socket_path.exists() {
            // Give it a bit more time
            tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;
            let _ = std::fs::remove_file(&socket_path);
        }
    }

    println!(
        "{}",
        colored::Colorize::green("Daemon stopped successfully.")
    );

    Ok(())
}

pub async fn stop_all() -> Result<()> {
    let mut client = create_client().await?;

    match client.stop_all_processes().await {
        Ok(count) => {
            println!(
                "{}",
                colored::Colorize::green(format!("Stopped {} process(es).", count).as_str())
            );
        }
        Err(e) => {
            eprintln!(
                "{}",
                colored::Colorize::red(format!("Failed to stop processes: {}", e).as_str())
            );
            return Err(e);
        }
    }

    Ok(())
}
