use axum::{
    Router,
    response::Html,
    routing::{get, post, put},
};

#[cfg(feature = "embed-static")]
use axum::{http::StatusCode, response::IntoResponse};
use std::fs;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::watch;
use tower_http::cors::CorsLayer;
use tower_http::services::ServeDir;
use tracing::info;

use crate::manager::ProcessManager;
use crate::web::api;

// Release 模式下嵌入静态文件
#[cfg(feature = "embed-static")]
use rust_embed::RustEmbed;

#[cfg(feature = "embed-static")]
#[derive(RustEmbed)]
#[folder = "static/"]
struct StaticAssets;

/// Web Dashboard server
pub struct WebServer {
    process_manager: Arc<ProcessManager>,
    host: String,
    port: u16,
    shutdown_rx: Option<watch::Receiver<bool>>,
}

impl WebServer {
    pub fn new(process_manager: Arc<ProcessManager>, host: &str, port: u16) -> Self {
        Self {
            process_manager,
            host: host.to_string(),
            port,
            shutdown_rx: None,
        }
    }

    pub fn with_shutdown(mut self, rx: watch::Receiver<bool>) -> Self {
        self.shutdown_rx = Some(rx);
        self
    }

    pub async fn serve(&self) -> anyhow::Result<()> {
        let app = self.create_router();
        let addr = format!("{}:{}", self.host, self.port);

        info!("Starting Web Dashboard on http://{}", addr);

        let listener = tokio::net::TcpListener::bind(&addr).await?;

        if let Some(mut shutdown_rx) = self.shutdown_rx.clone() {
            let _ = axum::serve(listener, app)
                .with_graceful_shutdown(async move {
                    let _ = shutdown_rx.changed().await;
                    info!("Web server shutdown signal received");
                })
                .await;
        } else {
            let _ = axum::serve(listener, app).await;
        }

        Ok(())
    }

    fn create_router(&self) -> Router {
        let state = Arc::clone(&self.process_manager);

        #[cfg(not(feature = "embed-static"))]
        {
            // Debug 模式：从可执行文件旁边读取 static 目录
            let exe_dir = std::env::current_exe()
                .ok()
                .and_then(|p| p.parent().map(|p| p.to_path_buf()))
                .unwrap_or_else(|| PathBuf::from("."));

            let static_dir = exe_dir.join("static");

            Router::new()
                .route("/api/processes", get(api::list_processes))
                .route("/api/processes", post(api::start_process))
                .route("/api/processes/stop-all", post(api::stop_all_processes))
                .route("/api/processes/{id}", get(api::get_process))
                .route("/api/processes/{id}/start", post(api::start_process_by_id))
                .route("/api/processes/{id}/stop", post(api::stop_process))
                .route("/api/processes/{id}/restart", post(api::restart_process))
                .route("/api/processes/{id}/delete", post(api::delete_process))
                .route("/api/processes/{id}", put(api::update_process))
                .route("/api/processes/{id}/logs", get(api::get_logs))
                .route("/api/status", get(api::get_status))
                // Schedule routes
                .route("/api/schedules", get(api::list_schedules))
                .route("/api/schedules", post(api::create_schedule))
                .route("/api/schedules/{id}", get(api::get_schedule))
                .route("/api/schedules/{id}/pause", post(api::pause_schedule))
                .route("/api/schedules/{id}/resume", post(api::resume_schedule))
                .route("/api/schedules/{id}/delete", post(api::delete_schedule))
                .route("/api/schedules/{id}", put(api::update_schedule))
                // Batch schedule operations
                .route(
                    "/api/schedules/batch/pause",
                    post(api::batch_pause_schedules),
                )
                .route(
                    "/api/schedules/batch/resume",
                    post(api::batch_resume_schedules),
                )
                .route(
                    "/api/schedules/batch/delete",
                    post(api::batch_delete_schedules),
                )
                .route("/", get(index_handler_debug))
                .nest_service("/static", ServeDir::new(&static_dir))
                .layer(CorsLayer::permissive())
                .with_state(state)
        }

        #[cfg(feature = "embed-static")]
        {
            // Release 模式：使用嵌入的静态文件
            Router::new()
                .route("/api/processes", get(api::list_processes))
                .route("/api/processes", post(api::start_process))
                .route("/api/processes/stop-all", post(api::stop_all_processes))
                .route("/api/processes/{id}", get(api::get_process))
                .route("/api/processes/{id}/start", post(api::start_process_by_id))
                .route("/api/processes/{id}/stop", post(api::stop_process))
                .route("/api/processes/{id}/restart", post(api::restart_process))
                .route("/api/processes/{id}/delete", post(api::delete_process))
                .route("/api/processes/{id}", put(api::update_process))
                .route("/api/processes/{id}/logs", get(api::get_logs))
                .route("/api/status", get(api::get_status))
                // Schedule routes
                .route("/api/schedules", get(api::list_schedules))
                .route("/api/schedules", post(api::create_schedule))
                .route("/api/schedules/{id}", get(api::get_schedule))
                .route("/api/schedules/{id}/pause", post(api::pause_schedule))
                .route("/api/schedules/{id}/resume", post(api::resume_schedule))
                .route("/api/schedules/{id}/delete", post(api::delete_schedule))
                .route("/api/schedules/{id}", put(api::update_schedule))
                // Batch schedule operations
                .route(
                    "/api/schedules/batch/pause",
                    post(api::batch_pause_schedules),
                )
                .route(
                    "/api/schedules/batch/resume",
                    post(api::batch_resume_schedules),
                )
                .route(
                    "/api/schedules/batch/delete",
                    post(api::batch_delete_schedules),
                )
                .route("/", get(index_handler_embedded))
                .route("/static/{*path}", get(serve_embedded_static))
                .layer(CorsLayer::permissive())
                .with_state(state)
        }
    }
}

// Debug 模式：从文件系统读取 index.html
#[cfg(not(feature = "embed-static"))]
async fn index_handler_debug() -> Html<String> {
    let exe_dir = std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|p| p.to_path_buf()))
        .unwrap_or_else(|| PathBuf::from("."));

    let html_path = exe_dir.join("static").join("index.html");

    let html_content = fs::read_to_string(&html_path).unwrap_or_else(|_| {
        "<html><body><h1>Error: Could not load index.html</h1></body></html>".to_string()
    });

    Html(html_content)
}

// Release 模式：从嵌入的资源读取 index.html
#[cfg(feature = "embed-static")]
async fn index_handler_embedded() -> impl IntoResponse {
    let result: Result<Html<String>, (StatusCode, &str)> = StaticAssets::get("index.html")
        .map(|file| Html(String::from_utf8_lossy(file.data.as_ref()).to_string()))
        .ok_or((
            StatusCode::NOT_FOUND,
            "<h1>Error: index.html not found</h1>",
        ));
    result
}

// Release 模式：服务嵌入的静态文件
#[cfg(feature = "embed-static")]
async fn serve_embedded_static(
    axum::extract::Path(path): axum::extract::Path<String>,
) -> impl IntoResponse {
    // 防止路径遍历攻击
    let safe_path = path.replace("..", "").replace("//", "/");

    let result: Result<
        (
            [(axum::http::HeaderName, axum::http::HeaderValue); 1],
            Vec<u8>,
        ),
        (StatusCode, &str),
    > = StaticAssets::get(&safe_path)
        .map(|file| {
            let mime = guess_mime_type(&safe_path);
            let header_value: axum::http::HeaderValue = mime.parse().unwrap();
            let data = file.data.as_ref().to_vec();
            (
                [(
                    axum::http::HeaderName::from_static("content-type"),
                    header_value,
                )],
                data,
            )
        })
        .ok_or((StatusCode::NOT_FOUND, "File not found"));
    result
}

#[cfg(feature = "embed-static")]
fn guess_mime_type(path: &str) -> &'static str {
    if path.ends_with(".html") || path.ends_with(".htm") {
        "text/html"
    } else if path.ends_with(".css") {
        "text/css"
    } else if path.ends_with(".js") {
        "application/javascript"
    } else if path.ends_with(".json") {
        "application/json"
    } else if path.ends_with(".svg") {
        "image/svg+xml"
    } else if path.ends_with(".png") {
        "image/png"
    } else if path.ends_with(".jpg") || path.ends_with(".jpeg") {
        "image/jpeg"
    } else if path.ends_with(".gif") {
        "image/gif"
    } else if path.ends_with(".ico") {
        "image/x-icon"
    } else {
        "application/octet-stream"
    }
}
