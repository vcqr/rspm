use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::{RwLock, watch};
use tokio::task::JoinHandle;
use tracing::{info, error, warn};
use axum::{
    routing::get,
    Router,
    response::Html,
    http::StatusCode,
    extract::State,
};
use tower_http::services::ServeDir;
use chrono::Utc;

/// Static file server information
#[derive(Debug, Clone)]
pub struct StaticServerInfo {
    pub id: String,
    pub name: String,
    pub host: String,
    pub port: i32,
    pub directory: String,
    pub running: bool,
    pub started_at: Option<chrono::DateTime<Utc>>,
}

/// Static file server instance
struct StaticServer {
    info: StaticServerInfo,
    shutdown_tx: watch::Sender<bool>,
    handle: Option<JoinHandle<()>>,
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
    ) -> anyhow::Result<String> {
        let id = format!("static-{}", uuid::Uuid::new_v4().to_string().split('-').next().unwrap());

        // Check if port is already in use
        let servers = self.servers.read().await;
        for (_, server) in servers.iter() {
            if server.info.port == port && server.info.running {
                return Err(anyhow::anyhow!("Port {} is already in use", port));
            }
        }
        drop(servers);

        // Validate directory
        let dir_path = PathBuf::from(&directory);
        if !dir_path.exists() {
            return Err(anyhow::anyhow!("Directory does not exist: {}", directory));
        }
        if !dir_path.is_dir() {
            return Err(anyhow::anyhow!("Path is not a directory: {}", directory));
        }

        let (shutdown_tx, mut shutdown_rx) = watch::channel(false);

        let info = StaticServerInfo {
            id: id.clone(),
            name: name.clone(),
            host: host.clone(),
            port,
            directory: directory.clone(),
            running: true,
            started_at: Some(Utc::now()),
        };

        let addr = format!("{}:{}", host, port);
        let dir_clone = directory.clone();
        let id_clone = id.clone();

        // Spawn the server task
        let handle = tokio::spawn(async move {
            let app = Router::new()
                .route("/", get(serve_index))
                .nest_service("/", ServeDir::new(&dir_clone))
                .with_state(dir_clone.clone());

            info!("Static server {} starting on http://{}", id_clone, addr);

            let listener = match tokio::net::TcpListener::bind(&addr).await {
                Ok(l) => l,
                Err(e) => {
                    error!("Failed to bind to {}: {}", addr, e);
                    return;
                }
            };

            let server = axum::serve(listener, app);

            // Run server with graceful shutdown
            let graceful = server.with_graceful_shutdown(async move {
                let _ = shutdown_rx.changed().await;
                info!("Static server {} shutdown signal received", id_clone);
            });

            if let Err(e) = graceful.await {
                error!("Static server {} error: {}", id_clone, e);
            }

            info!("Static server {} stopped", id_clone);
        });

        let server = StaticServer {
            info: info.clone(),
            shutdown_tx,
            handle: Some(handle),
        };

        let mut servers = self.servers.write().await;
        servers.insert(id.clone(), server);

        info!("Started static file server '{}' on http://{}:{}", name, host, port);

        Ok(id)
    }

    /// Stop a static file server
    pub async fn stop_server(&self, id: &str) -> anyhow::Result<()> {
        let mut servers = self.servers.write().await;

        if let Some(server) = servers.get_mut(id) {
            // Send shutdown signal
            let _ = server.shutdown_tx.send(true);

            // Wait for the task to complete
            if let Some(handle) = server.handle.take() {
                drop(servers); // Release the lock before awaiting
                let _ = tokio::time::timeout(tokio::time::Duration::from_secs(5), handle).await;

                // Update the server status
                let mut servers = self.servers.write().await;
                if let Some(server) = servers.get_mut(id) {
                    server.info.running = false;
                }
            }

            info!("Stopped static file server {}", id);
            Ok(())
        } else {
            Err(anyhow::anyhow!("Static server not found: {}", id))
        }
    }

    /// Get information about a specific server
    pub async fn get_server(&self, id: &str) -> Option<StaticServerInfo> {
        let servers = self.servers.read().await;
        servers.get(id).map(|s| s.info.clone())
    }

    /// List all static file servers
    pub async fn list_servers(&self) -> Vec<StaticServerInfo> {
        let servers = self.servers.read().await;
        servers.values().map(|s| s.info.clone()).collect()
    }

    /// Stop all static file servers
    pub async fn stop_all(&self) {
        let ids: Vec<String> = {
            let servers = self.servers.read().await;
            servers.keys().cloned().collect()
        };

        for id in ids {
            if let Err(e) = self.stop_server(&id).await {
                warn!("Failed to stop static server {}: {}", id, e);
            }
        }
    }
}

/// Handler for serving index.html or directory listing
async fn serve_index(State(dir): State<String>) -> Html<String> {
    let index_path = PathBuf::from(&dir).join("index.html");

    if index_path.exists() {
        match tokio::fs::read_to_string(&index_path).await {
            Ok(content) => Html(content),
            Err(_) => Html(generate_directory_listing(&dir)),
        }
    } else {
        Html(generate_directory_listing(&dir))
    }
}

/// Generate a simple directory listing HTML page
fn generate_directory_listing(dir: &str) -> String {
    let path = PathBuf::from(dir);
    let mut entries = Vec::new();

    if let Ok(read_dir) = std::fs::read_dir(&path) {
        for entry in read_dir.flatten() {
            let name = entry.file_name().to_string_lossy().to_string();
            let is_dir = entry.file_type().map(|ft| ft.is_dir()).unwrap_or(false);
            let suffix = if is_dir { "/" } else { "" };
            entries.push(format!(
                r#"<li><a href="{0}{1}">{0}{1}</a></li>"#,
                name, suffix
            ));
        }
    }

    entries.sort();

    format!(
        r#"<!DOCTYPE html>
<html>
<head>
    <meta charset="utf-8">
    <title>Index of {0}</title>
    <style>
        body {{ font-family: -apple-system, BlinkMacSystemFont, "Segoe UI", Roboto, sans-serif; max-width: 800px; margin: 40px auto; padding: 0 20px; }}
        h1 {{ color: #333; border-bottom: 1px solid #ddd; padding-bottom: 10px; }}
        ul {{ list-style: none; padding: 0; }}
        li {{ padding: 5px 0; }}
        a {{ color: #0066cc; text-decoration: none; }}
        a:hover {{ text-decoration: underline; }}
        .path {{ color: #666; font-size: 14px; margin-bottom: 20px; }}
    </style>
</head>
<body>
    <h1>Index of {0}</h1>
    <div class="path">{0}</div>
    <ul>
        <li><a href="../">../</a></li>
        {1}
    </ul>
    <hr>
    <footer style="color: #666; font-size: 12px; margin-top: 40px;">
        RSPM Static File Server
    </footer>
</body>
</html>"#,
        dir,
        entries.join("\n        ")
    )
}
