use axum::{
    Router,
    body::Body,
    http::{Request, StatusCode},
    response::{Html, IntoResponse},
    routing::get,
};
use chrono::Utc;
use rspm_common::{Result, RspmError, StaticServerInfo};
use std::collections::HashMap;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::{RwLock, watch};
use tower::util::ServiceExt;
use tower_http::services::ServeDir;
use tracing::{error, info};
use uuid::Uuid;

/// Static file server instance
struct StaticServer {
    info: StaticServerInfo,
    shutdown_tx: Option<watch::Sender<bool>>,
}

/// Manager for static file servers
pub struct StaticServerManager {
    servers: Arc<RwLock<HashMap<String, StaticServer>>>,
}

impl StaticServerManager {
    pub fn new() -> Self {
        Self {
            servers: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Start a new static file server
    pub async fn start_server(
        &self,
        name: String,
        host: String,
        port: i32,
        directory: String,
    ) -> Result<String> {
        let id = Uuid::new_v4().to_string();
        let addr: SocketAddr = format!("{}:{}", host, port)
            .parse()
            .map_err(|e| RspmError::InvalidConfig(format!("Invalid address: {}", e)))?;

        // Check if port is already in use
        if self.is_port_in_use(port).await {
            return Err(RspmError::InvalidConfig(format!(
                "Port {} is already in use",
                port
            )));
        }

        // Check if directory exists
        let dir_path = PathBuf::from(&directory);
        if !dir_path.exists() {
            return Err(RspmError::InvalidConfig(format!(
                "Directory does not exist: {}",
                directory
            )));
        }
        if !dir_path.is_dir() {
            return Err(RspmError::InvalidConfig(format!(
                "Path is not a directory: {}",
                directory
            )));
        }

        let (shutdown_tx, mut shutdown_rx) = watch::channel(false);
        let (started_tx, started_rx) = tokio::sync::oneshot::channel();

        let info = StaticServerInfo {
            id: id.clone(),
            name: name.clone(),
            host: host.clone(),
            port,
            directory: directory.clone(),
            running: true,
            started_at: Some(Utc::now()),
        };

        let dir_clone = directory.clone();
        let name_for_log = name.clone();
        let name_for_shutdown = name.clone();

        // Spawn the server
        tokio::spawn(async move {
            // Create router with directory listing support
            let _serve_dir = ServeDir::new(&dir_clone);
            let dir_clone_for_handler = dir_clone.clone();

            let app = Router::new().fallback(get(|req: Request<Body>| async move {
                // First try to serve the file
                let serve_dir = ServeDir::new(&dir_clone_for_handler);
                let uri = req.uri().clone();
                let path = uri.path().trim_start_matches('/');
                let full_path = PathBuf::from(&dir_clone_for_handler).join(path);

                // Check if it's a directory
                if full_path.is_dir() {
                    // Generate directory listing
                    match generate_directory_listing(&dir_clone_for_handler, path, uri.path()).await
                    {
                        Ok(html) => Html(html).into_response(),
                        Err(_) => (
                            StatusCode::INTERNAL_SERVER_ERROR,
                            "Error generating directory listing",
                        )
                            .into_response(),
                    }
                } else {
                    // Try to serve file using ServeDir
                    match serve_dir.oneshot(req).await {
                        Ok(response) => response.into_response(),
                        Err(_) => (StatusCode::NOT_FOUND, "Not Found").into_response(),
                    }
                }
            }));

            info!(
                "Starting static file server '{}' on http://{}:{}",
                name_for_log, host, port
            );

            let listener = match tokio::net::TcpListener::bind(&addr).await {
                Ok(l) => {
                    info!("Static server '{}' bound to {}", name_for_log, addr);
                    let _ = started_tx.send(Ok(()));
                    l
                }
                Err(e) => {
                    error!("Failed to bind to {}: {}", addr, e);
                    let _ = started_tx.send(Err(e.to_string()));
                    return;
                }
            };

            let server = axum::serve(listener, app);

            // Run server with graceful shutdown
            let _ = server
                .with_graceful_shutdown(async move {
                    let _ = shutdown_rx.changed().await;
                    info!(
                        "Static server '{}' shutdown signal received",
                        name_for_shutdown
                    );
                })
                .await;

            info!("Static server '{}' stopped", name_for_log);
        });

        // Wait for server to start
        match tokio::time::timeout(tokio::time::Duration::from_secs(5), started_rx).await {
            Ok(Ok(Ok(()))) => {
                info!("Static server '{}' started successfully", name);
            }
            Ok(Ok(Err(e))) => {
                return Err(RspmError::StartFailed(format!("Failed to bind: {}", e)));
            }
            Ok(Err(_)) => {
                return Err(RspmError::StartFailed(
                    "Server start channel closed".to_string(),
                ));
            }
            Err(_) => {
                return Err(RspmError::StartFailed("Server start timeout".to_string()));
            }
        }

        // Store server info
        let server = StaticServer {
            info,
            shutdown_tx: Some(shutdown_tx),
        };

        self.servers.write().await.insert(id.clone(), server);

        Ok(id)
    }

    /// Stop a static file server
    pub async fn stop_server(&self, id: &str) -> Result<()> {
        let mut servers = self.servers.write().await;

        if let Some(server) = servers.get_mut(id) {
            if let Some(tx) = server.shutdown_tx.take() {
                let _ = tx.send(true);
                server.info.running = false;
                info!("Stopping static server '{}'", id);
                Ok(())
            } else {
                Err(RspmError::StateError("Server already stopping".to_string()))
            }
        } else {
            Err(RspmError::NotFound(format!("Server not found: {}", id)))
        }
    }

    /// Get server info by ID
    pub async fn get_server(&self, id: &str) -> Option<StaticServerInfo> {
        let servers = self.servers.read().await;
        servers.get(id).map(|s| s.info.clone())
    }

    /// List all servers
    pub async fn list_servers(&self) -> Vec<StaticServerInfo> {
        let servers = self.servers.read().await;
        servers.values().map(|s| s.info.clone()).collect()
    }

    /// Check if a port is already in use by any server
    async fn is_port_in_use(&self, port: i32) -> bool {
        let servers = self.servers.read().await;
        servers
            .values()
            .any(|s| s.info.port == port && s.info.running)
    }

    /// Stop all servers
    pub async fn stop_all(&self) {
        let mut servers = self.servers.write().await;
        for (id, server) in servers.iter_mut() {
            if let Some(tx) = server.shutdown_tx.take() {
                let _ = tx.send(true);
                server.info.running = false;
                info!("Stopping static server '{}'", id);
            }
        }
    }
}

impl Default for StaticServerManager {
    fn default() -> Self {
        Self::new()
    }
}

/// Generate HTML directory listing
async fn generate_directory_listing(
    base_dir: &str,
    relative_path: &str,
    uri_path: &str,
) -> anyhow::Result<String> {
    let full_path = PathBuf::from(base_dir).join(relative_path);
    let mut entries = tokio::fs::read_dir(&full_path).await?;
    let mut items = Vec::new();

    // Add parent directory link if not at root
    if uri_path != "/" {
        items.push(format!(
            r#"<tr><td><a href="{}">📁 ..</a></td><td>-</td><td>-</td></tr>"#,
            parent_path(uri_path)
        ));
    }

    while let Some(entry) = entries.next_entry().await? {
        let name = entry.file_name().to_string_lossy().to_string();
        let metadata = entry.metadata().await?;
        let is_dir = metadata.is_dir();

        let size = if is_dir {
            "-".to_string()
        } else {
            format_size(metadata.len())
        };

        let modified = metadata
            .modified()
            .ok()
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
            .and_then(|d| chrono::DateTime::from_timestamp(d.as_secs() as i64, 0))
            .map(|dt| dt.format("%Y-%m-%d %H:%M:%S").to_string())
            .unwrap_or_else(|| "-".to_string());

        let icon = if is_dir { "📁" } else { "📄" };
        let href = format!("{}{}", uri_path, name);
        let href = if is_dir { format!("{}/", href) } else { href };

        items.push(format!(
            r#"<tr><td><a href="{}">{} {}</a></td><td>{}</td><td>{}</td></tr>"#,
            href, icon, name, size, modified
        ));
    }

    // Sort: directories first, then files
    items.sort_by(|a, b| {
        let a_is_dir = a.contains("📁");
        let b_is_dir = b.contains("📁");
        if a_is_dir && !b_is_dir {
            std::cmp::Ordering::Less
        } else if !a_is_dir && b_is_dir {
            std::cmp::Ordering::Greater
        } else {
            a.cmp(b)
        }
    });

    let html = format!(
        r#"<!DOCTYPE html>
<html>
<head>
    <meta charset="UTF-8">
    <title>Index of {}</title>
    <style>
        body {{ font-family: -apple-system, BlinkMacSystemFont, "Segoe UI", Roboto, sans-serif; max-width: 1200px; margin: 40px auto; padding: 0 20px; }}
        h1 {{ border-bottom: 1px solid #ddd; padding-bottom: 10px; }}
        table {{ width: 100%; border-collapse: collapse; }}
        th, td {{ text-align: left; padding: 10px; border-bottom: 1px solid #eee; }}
        th {{ font-weight: 600; color: #666; }}
        a {{ text-decoration: none; color: #0066cc; }}
        a:hover {{ text-decoration: underline; }}
        .footer {{ margin-top: 40px; padding-top: 20px; border-top: 1px solid #ddd; color: #666; font-size: 0.9em; }}
    </style>
</head>
<body>
    <h1>Index of {}</h1>
    <table>
        <thead>
            <tr><th>Name</th><th>Size</th><th>Modified</th></tr>
        </thead>
        <tbody>
            {}
        </tbody>
    </table>
    <div class="footer">RSPM Static File Server</div>
</body>
</html>"#,
        uri_path,
        uri_path,
        items.join("\n")
    );

    Ok(html)
}

fn parent_path(path: &str) -> String {
    if path == "/" {
        "/".to_string()
    } else {
        let mut parts: Vec<&str> = path.trim_end_matches('/').split('/').collect();
        parts.pop();
        if parts.is_empty() || (parts.len() == 1 && parts[0].is_empty()) {
            "/".to_string()
        } else {
            parts.join("/") + "/"
        }
    }
}

fn format_size(size: u64) -> String {
    const UNITS: &[&str] = &["B", "KB", "MB", "GB", "TB"];
    let mut size = size as f64;
    let mut unit_index = 0;

    while size >= 1024.0 && unit_index < UNITS.len() - 1 {
        size /= 1024.0;
        unit_index += 1;
    }

    format!("{:.1} {}", size, UNITS[unit_index])
}
