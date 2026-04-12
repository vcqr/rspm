use anyhow::Result;
use clap::Parser;
use rspm_common::{DEFAULT_GRPC_PORT, DaemonConfig, print_banner};
use rspm_daemon::{ProcessManager, RpcServer, WebServer};
use std::path::PathBuf;
use std::sync::Arc;
use tracing::info;
use tracing_subscriber::EnvFilter;

#[derive(Parser, Debug)]
#[command(name = "rspmd")]
struct Args {
    /// Run as daemon (background process)
    #[arg(short, long)]
    daemon: bool,
}

// Parse args before main to handle daemonization early
fn main() {
    let args = Args::parse();

    // Handle daemonization BEFORE tokio starts
    if args.daemon {
        daemonize_early();
    }

    // Continue with async main
    let rt = tokio::runtime::Runtime::new().unwrap();
    rt.block_on(async {
        if let Err(e) = run().await {
            eprintln!("Error: {}", e);
            std::process::exit(1);
        }
    });
}

#[cfg(unix)]
fn daemonize_early() {
    use std::io::Write;

    // Save PID before daemonizing
    let home_dir = dirs::home_dir().unwrap_or_else(|| PathBuf::from("/tmp"));
    let pid_dir = home_dir.join(".rspm").join("pid");
    std::fs::create_dir_all(&pid_dir).ok();
    if let Ok(mut f) = std::fs::File::create(pid_dir.join("rspmd.pid")) {
        let _ = write!(f, "{}", std::process::id());
    }

    // Use double fork to properly daemonize
    match unsafe { libc::fork() } {
        -1 => {
            eprintln!("Failed to fork: {}", std::io::Error::last_os_error());
            std::process::exit(1);
        }
        0 => {
            // First child - become session leader
            if unsafe { libc::setsid() } == -1 {
                std::process::exit(1);
            }

            // Second fork to fully detach
            match unsafe { libc::fork() } {
                -1 => {
                    std::process::exit(1);
                }
                0 => {
                    // Grandchild - redirect stdio to /dev/null
                    use std::os::unix::io::AsRawFd;
                    let devnull = std::fs::OpenOptions::new()
                        .write(true)
                        .open("/dev/null").ok();

                    if let Some(fd) = devnull {
                        unsafe {
                            libc::dup2(fd.as_raw_fd(), libc::STDIN_FILENO);
                            libc::dup2(fd.as_raw_fd(), libc::STDOUT_FILENO);
                            libc::dup2(fd.as_raw_fd(), libc::STDERR_FILENO);
                        }
                    }

                    // Continue running in background
                }
                _ => {
                    // First child exits
                    std::process::exit(0);
                }
            }
        }
        pid => {
            // Parent waits briefly then exits
            std::thread::sleep(std::time::Duration::from_millis(100));
            println!("Daemon started with PID: {}", pid);
            std::process::exit(0);
        }
    }
}

#[cfg(windows)]
fn daemonize_early() {
    use std::io::Write;
    use windows::Win32::System::Console::{AllocConsole, FreeConsole};

    // Save PID before daemonizing
    let home_dir = dirs::home_dir().unwrap_or_else(|| PathBuf::from("C:\\Users\\Administrator"));
    let pid_dir = home_dir.join(".rspm").join("pid");
    std::fs::create_dir_all(&pid_dir).ok();
    if let Ok(mut f) = std::fs::File::create(pid_dir.join("rspmd.pid")) {
        let _ = write!(f, "{}", std::process::id());
    }

    // On Windows, use nohup-like approach
    // Detach from console and run in background
    unsafe {
        // Free console to detach from terminal
        let _ = FreeConsole();

        // Re-attach to new console if needed (optional)
        let _ = AllocConsole();
    }

    println!("Daemon started");
}

async fn run() -> Result<()> {
    // Determine directories first (before any output)
    let home_dir = dirs::home_dir().unwrap_or_else(|| PathBuf::from("C:\\Users\\Administrator"));

    let rspm_dir = home_dir.join(".rspm");
    let base_dir = rspm_dir.join("logs");
    let db_path = rspm_dir.join("db").join("rspm.db");
    let sock_dir = rspm_dir.join("sock");

    // Ensure directories exist
    std::fs::create_dir_all(&base_dir)?;
    std::fs::create_dir_all(&sock_dir)?;

    // Ensure database directory exists
    if let Some(db_dir) = db_path.parent() {
        std::fs::create_dir_all(db_dir)?;
    }

    // Load configuration from environment variable or default
    let config = load_config();
    let host = config.host.clone();
    let grpc_addr = format!("{}:{}", config.host, config.port);
    let web_port = config.get_web_port();

    // Print banner
    print_banner();

    // Initialize logging after directory setup
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| EnvFilter::new("info,rspm_daemon=debug")),
        )
        .init();

    info!("Starting rspm daemon...");
    info!("Version: {}", rspm_common::VERSION);
    info!("RSPM directory: {:?}", rspm_dir);
    info!("Logs directory: {:?}", base_dir);
    info!("Database path: {:?}", db_path);
    info!("gRPC address: {}", grpc_addr);
    info!("Web dashboard port: {}", web_port);

    // Create process manager
    let process_manager = match ProcessManager::new(rspm_dir, &db_path).await {
        Ok(pm) => Arc::new(pm),
        Err(e) => {
            eprintln!("Failed to create process manager: {}", e);
            return Err(e.into());
        }
    };

    if let Err(e) = process_manager.init().await {
        eprintln!("Failed to initialize process manager: {}", e);
        return Err(e.into());
    }

    // Initialize schedule manager
    if let Err(e) = process_manager.init_scheduler().await {
        eprintln!("Failed to initialize schedule manager: {}", e);
        return Err(e.into());
    }

    // Clone process_manager for web server
    let process_manager_web = Arc::clone(&process_manager);

    // Start gRPC server
    let server = RpcServer::new(process_manager);

    // Get shutdown receiver for web server
    let web_shutdown_rx = server.shutdown_receiver();

    // Start Web server in background
    let host_for_web = host.clone();
    tokio::spawn(async move {
        let web_server = WebServer::new(process_manager_web, &host_for_web, web_port)
            .with_shutdown(web_shutdown_rx);
        if let Err(e) = web_server.serve().await {
            eprintln!("Web server error: {}", e);
        }
    });

    #[cfg(unix)]
    {
        use rspm_proto::process_manager_server::ProcessManagerServer;
        use std::os::unix::fs::PermissionsExt;
        use tokio::net::UnixListener;
        use tokio_stream::wrappers::UnixListenerStream;
        use tonic::transport::Server;

        // Determine connection type based on host config
        let use_tcp = host != "127.0.0.1" && host != "localhost" && !host.is_empty();

        if use_tcp {
            // Remote config: use TCP only
            let addr: std::net::SocketAddr = grpc_addr.parse()?;
            info!("Starting TCP server on {}", addr);

            let mut shutdown_rx = server.shutdown_receiver();
            Server::builder()
                .add_service(ProcessManagerServer::new(server))
                .serve_with_shutdown(addr, async move {
                    let _ = shutdown_rx.changed().await;
                    info!("Shutdown signal received, stopping gRPC server...");
                })
                .await?;
        } else {
            // Local config: use Unix socket only
            let socket_path = sock_dir.join("rspm.sock");
            let _ = std::fs::remove_file(&socket_path);
            let listener = UnixListener::bind(&socket_path)?;
            std::fs::set_permissions(&socket_path, std::fs::Permissions::from_mode(0o777))?;

            info!("Daemon listening on {}", socket_path.display());
            info!("Web Dashboard available at http://{}:{}", host, web_port);

            let mut shutdown_rx = server.shutdown_receiver();
            Server::builder()
                .add_service(ProcessManagerServer::new(server))
                .serve_with_incoming_shutdown(UnixListenerStream::new(listener), async move {
                    let _ = shutdown_rx.changed().await;
                    info!("Shutdown signal received, stopping gRPC server...");
                })
                .await?;
        }
    }

    #[cfg(not(unix))]
    {
        use rspm_proto::process_manager_server::ProcessManagerServer;
        use tonic::transport::Server;

        let addr: std::net::SocketAddr = grpc_addr.parse()?;

        info!("Starting TCP server on {}", grpc_addr);
        info!("Web Dashboard available at http://{}:{}", host, web_port);

        let mut shutdown_rx = server.shutdown_receiver();

        Server::builder()
            .add_service(ProcessManagerServer::new(server))
            .serve_with_shutdown(addr, async move {
                let _ = shutdown_rx.changed().await;
                info!("Shutdown signal received, stopping gRPC server...");
            })
            .await?;
    }

    // Wait for web server to shutdown (give it some time to clean up)
    tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;

    info!("Daemon shutdown complete");
    Ok(())
}

/// Load configuration from environment variable or use default
fn load_config() -> DaemonConfig {
    // Check for config file path in environment variable
    if let Ok(config_path) = std::env::var("RSPM_CONFIG_FILE") {
        let path = PathBuf::from(config_path);
        if path.exists() {
            match DaemonConfig::from_file(&path) {
                Ok(config) => {
                    eprintln!("Loaded config from: {}", path.display());
                    return config;
                }
                Err(e) => {
                    eprintln!(
                        "Warning: Failed to load config from {}: {}",
                        path.display(),
                        e
                    );
                }
            }
        } else {
            eprintln!("Warning: Config file not found: {}", path.display());
        }
    }

    // Try default config file
    let default_path = DaemonConfig::get_default_config_path();
    if default_path.exists() {
        match DaemonConfig::from_file(&default_path) {
            Ok(config) => {
                eprintln!("Loaded default config from: {}", default_path.display());
                return config;
            }
            Err(e) => {
                eprintln!("Warning: Failed to load default config: {}", e);
            }
        }
    }

    // Use default configuration
    eprintln!("Using default configuration (port: {})", DEFAULT_GRPC_PORT);
    DaemonConfig::default()
}
