mod grpc_client;

pub use grpc_client::GrpcClient;

use rspm_common::{DaemonConfig, Result, print_banner};

#[cfg(unix)]
use rspm_common::get_socket_path;

/// Get the gRPC address for connecting to daemon
/// Uses config file to determine connection method:
/// - If host is "127.0.0.1" or "localhost" on Unix: uses Unix socket
/// - Otherwise: uses TCP connection (supports remote hosts)
fn get_grpc_addr() -> String {
    let config = DaemonConfig::load_default();

    // Check if we should use Unix socket (local connection only)
    #[cfg(unix)]
    {
        if config.host == "127.0.0.1" || config.host == "localhost" || config.host.is_empty() {
            return get_socket_path().to_string_lossy().to_string();
        }
    }

    // Use TCP connection (works for both local and remote)
    config.get_grpc_addr()
}

/// Check if using local connection (should auto-start daemon)
fn is_local_connection() -> bool {
    #[cfg(unix)]
    {
        let config = DaemonConfig::load_default();
        config.host == "127.0.0.1" || config.host == "localhost" || config.host.is_empty()
    }
    #[cfg(not(unix))]
    {
        // On Windows, always use TCP but still auto-start for local connections
        let config = DaemonConfig::load_default();
        config.host == "127.0.0.1" || config.host == "localhost" || config.host.is_empty()
    }
}

/// Get the web dashboard URL
fn get_web_dashboard_url() -> String {
    let config = DaemonConfig::load_default();
    format!("http://{}:{}", config.host, config.get_web_port())
}

/// Start the daemon process (only for local connections)
async fn start_daemon_process() -> Result<()> {
    // Don't try to start remote daemon
    if !is_local_connection() {
        return Err(rspm_common::RspmError::DaemonNotRunning);
    }

    // Remove stale socket if exists (Unix only)
    #[cfg(unix)]
    {
        let socket_path = get_socket_path();
        if socket_path.exists() {
            let _ = std::fs::remove_file(&socket_path);
        }
    }

    // Get the path to the daemon binary
    let daemon_path = std::env::current_exe()
        .map(|p| {
            let parent = p.parent().map(|p| p.to_path_buf()).unwrap_or_default();
            #[cfg(windows)]
            let daemon_name = "rspmd.exe";
            #[cfg(not(windows))]
            let daemon_name = "rspmd";
            parent.join(daemon_name)
        })
        .unwrap_or_else(|_| std::path::PathBuf::from("rspmd"));

    // Get default config file path
    let config_file = DaemonConfig::get_default_config_path();

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

    cmd.spawn().map_err(|e| {
        rspm_common::RspmError::InternalError(format!("Failed to start daemon: {}", e))
    })?;

    // Wait for daemon to start (with timeout)
    let mut attempts = 0;
    let addr = get_grpc_addr();

    while attempts < 20 {
        tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

        #[cfg(unix)]
        {
            let socket_path = get_socket_path();
            if socket_path.exists()
                && let Ok(mut client) = GrpcClient::connect(&addr).await
                && client.get_daemon_status().await.is_ok()
            {
                return Ok(());
            }
        }

        #[cfg(not(unix))]
        {
            // Windows uses TCP
            if let Ok(mut client) = GrpcClient::connect(&addr).await {
                if client.get_daemon_status().await.is_ok() {
                    return Ok(());
                }
            }
        }

        attempts += 1;
    }

    Err(rspm_common::RspmError::DaemonNotRunning)
}

/// Create a client connection to the daemon, starting it if necessary
pub async fn create_client() -> Result<GrpcClient> {
    let addr = get_grpc_addr();
    eprintln!("Connecting to: {}", addr);

    // Try to connect directly first
    match GrpcClient::connect(&addr).await {
        Ok(client) => return Ok(client),
        Err(e) => {
            eprintln!("Connection failed: {}", e);
            // For remote connections, don't try to start local daemon
            if !is_local_connection() {
                return Err(rspm_common::RspmError::DaemonNotRunning);
            }
        }
    }

    // Daemon is not running, try to start it
    eprintln!(
        "{}",
        colored::Colorize::yellow("Daemon is not running, starting it...")
    );
    start_daemon_process().await?;
    print_banner();
    println!("[Web Dashboard] {}\n", get_web_dashboard_url());

    // Now try to connect again
    GrpcClient::connect(&addr).await
}
