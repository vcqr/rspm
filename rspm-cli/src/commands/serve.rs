use crate::client::create_client;
use rspm_common::{ProcessConfig, Result, ServerType};
use std::collections::HashMap;
use std::env;

pub async fn serve(
    name: Option<String>,
    port: u16,
    host: String,
    dir: Option<String>,
) -> Result<()> {
    let mut client = create_client().await?;

    // Generate default name if not provided
    let server_name = name.unwrap_or_else(|| format!("static-server-{}", port));

    // Resolve directory path
    let directory = match dir {
        Some(d) => {
            let path = std::path::PathBuf::from(&d);
            if path.is_absolute() {
                d
            } else {
                env::current_dir()
                    .map(|cwd| cwd.join(&d))
                    .ok()
                    .and_then(|p| p.canonicalize().ok())
                    .map(|p| p.to_string_lossy().to_string())
                    .unwrap_or(d)
            }
        }
        None => env::current_dir()
            .ok()
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_else(|| ".".to_string()),
    };

    // Check if directory exists
    if !std::path::Path::new(&directory).exists() {
        return Err(rspm_common::RspmError::InvalidConfig(format!(
            "Directory does not exist: {}",
            directory
        )));
    }

    // Check if directory is readable
    if !std::path::Path::new(&directory).is_dir() {
        return Err(rspm_common::RspmError::InvalidConfig(format!(
            "Path is not a directory: {}",
            directory
        )));
    }

    println!("Starting static file server...");
    println!("  Name: {}", server_name);
    println!("  Host: {}", host);
    println!("  Port: {}", port);
    println!("  Directory: {}", directory);

    // Create a process config for the static server
    // Store server parameters in environment variables
    let mut env = HashMap::new();
    env.insert("RSPM_STATIC_HOST".to_string(), host.clone());
    env.insert("RSPM_STATIC_PORT".to_string(), port.to_string());
    env.insert("RSPM_STATIC_DIR".to_string(), directory.clone());

    let config = ProcessConfig {
        name: server_name.clone(),
        command: "__rspm_static_server__".to_string(), // Special marker command
        args: vec![host.clone(), port.to_string(), directory.clone()],
        env,
        cwd: Some(directory.clone()),
        instances: 1,
        autorestart: false, // Static servers should not auto-restart
        max_restarts: 0,
        max_memory_mb: 0,
        watch: false,
        watch_paths: vec![],
        log_file: None,
        error_file: None,
        log_max_size: 10 * 1024 * 1024,
        log_max_files: 5,
        server_type: ServerType::StaticServer,
    };

    let process_id = client.start_process(config).await?;

    println!("\n✓ Static server started successfully");
    println!("  Server ID: {}", process_id);
    println!("  URL: http://{}:{}", host, port);
    println!("\nYou can manage this server using:");
    println!("  rspm list              # List all processes");
    println!(
        "  rspm stop {}     # Stop the server",
        &process_id[..process_id.len().min(8)]
    );
    println!(
        "  rspm delete {}   # Delete the server",
        &process_id[..process_id.len().min(8)]
    );

    Ok(())
}
