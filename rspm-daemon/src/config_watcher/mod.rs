use notify::{Config, Event, RecommendedWatcher, RecursiveMode, Watcher};
use rspm_common::{ConfigFile, ConfigFormat, RspmError};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::mpsc;
use tracing::{error, info, warn};

use crate::manager::ProcessManager;

pub type Result<T> = std::result::Result<T, RspmError>;

/// Configuration file watcher for hot reload
pub struct ConfigWatcher {
    process_manager: Arc<ProcessManager>,
    watched_configs: HashMap<String, WatchedConfig>,
}

#[derive(Debug, Clone)]
struct WatchedConfig {
    path: PathBuf,
    processes: Vec<String>, // Process names managed by this config
}

impl ConfigWatcher {
    pub fn new(process_manager: Arc<ProcessManager>) -> Self {
        Self {
            process_manager,
            watched_configs: HashMap::new(),
        }
    }

    /// Start watching a configuration file
    pub async fn watch_config(&mut self, path: PathBuf) -> Result<()> {
        let path_str = path.to_string_lossy().to_string();

        if self.watched_configs.contains_key(&path_str) {
            warn!("Config file already watched: {}", path_str);
            return Ok(());
        }

        // Load initial config
        let config_file = self.load_config_file(&path).await?;

        // Start processes from config
        let mut process_names = Vec::new();
        for config in config_file.processes {
            let name = config.name.clone();
            match self.process_manager.start_process(config).await {
                Ok(id) => {
                    info!("Started process '{}' from config: {}", name, id);
                    process_names.push(name);
                }
                Err(e) => {
                    error!("Failed to start process from config: {}", e);
                }
            }
        }

        self.watched_configs.insert(
            path_str.clone(),
            WatchedConfig {
                path: path.clone(),
                processes: process_names,
            },
        );

        info!("Started watching config file: {}", path_str);
        Ok(())
    }

    /// Load and parse a configuration file
    async fn load_config_file(&self, path: &PathBuf) -> Result<ConfigFile> {
        let content = tokio::fs::read_to_string(path).await.map_err(|e| {
            RspmError::ConfigParseError(format!("Failed to read config file: {}", e))
        })?;

        let format = ConfigFormat::from_path(path).ok_or_else(|| {
            RspmError::UnsupportedConfigFormat(
                path.extension()
                    .and_then(|e| e.to_str())
                    .unwrap_or("unknown")
                    .to_string(),
            )
        })?;

        let config_file: ConfigFile = match format {
            ConfigFormat::Yaml => serde_yaml::from_str(&content)
                .map_err(|e| RspmError::ConfigParseError(format!("YAML parse error: {}", e)))?,
            ConfigFormat::Json => serde_json::from_str(&content)
                .map_err(|e| RspmError::ConfigParseError(format!("JSON parse error: {}", e)))?,
            ConfigFormat::Toml => toml::from_str(&content)
                .map_err(|e| RspmError::ConfigParseError(format!("TOML parse error: {}", e)))?,
        };

        Ok(config_file)
    }

    /// Start the file watcher
    pub async fn start_watching(&mut self) -> Result<()> {
        let (tx, mut rx) = mpsc::channel(100);

        let mut watcher: RecommendedWatcher = Watcher::new(
            move |res: std::result::Result<Event, notify::Error>| {
                if let Ok(event) = res {
                    let _ = tx.blocking_send(event);
                }
            },
            Config::default(),
        )
        .map_err(|e| RspmError::IoError(std::io::Error::other(e)))?;

        // Watch all config files
        for (path_str, watched) in &self.watched_configs {
            watcher
                .watch(&watched.path, RecursiveMode::NonRecursive)
                .map_err(|e| {
                    RspmError::IoError(std::io::Error::other(format!(
                        "Failed to watch {}: {}",
                        path_str, e
                    )))
                })?;
        }

        // Process events
        let process_manager = Arc::clone(&self.process_manager);
        let watched_configs = self.watched_configs.clone();

        tokio::spawn(async move {
            while let Some(event) = rx.recv().await {
                match event.kind {
                    notify::EventKind::Modify(_) | notify::EventKind::Create(_) => {
                        for path in event.paths {
                            let path_str = path.to_string_lossy().to_string();

                            if let Some(watched) = watched_configs.get(&path_str) {
                                info!("Config file changed: {}", path_str);

                                // Reload config
                                match Self::reload_config(
                                    Arc::clone(&process_manager),
                                    &path,
                                    watched.clone(),
                                )
                                .await
                                {
                                    Ok(_) => info!("Config reloaded successfully: {}", path_str),
                                    Err(e) => error!("Failed to reload config: {}", e),
                                }
                            }
                        }
                    }
                    _ => {}
                }
            }
        });

        Ok(())
    }

    /// Reload a configuration file
    async fn reload_config(
        process_manager: Arc<ProcessManager>,
        path: &PathBuf,
        watched: WatchedConfig,
    ) -> Result<()> {
        // Load new config
        let content = tokio::fs::read_to_string(path).await.map_err(|e| {
            RspmError::ConfigParseError(format!("Failed to read config file: {}", e))
        })?;

        let format = ConfigFormat::from_path(path)
            .ok_or_else(|| RspmError::UnsupportedConfigFormat("unknown".to_string()))?;

        let config_file: ConfigFile = match format {
            ConfigFormat::Yaml => serde_yaml::from_str(&content)
                .map_err(|e| RspmError::ConfigParseError(format!("YAML parse error: {}", e)))?,
            ConfigFormat::Json => serde_json::from_str(&content)
                .map_err(|e| RspmError::ConfigParseError(format!("JSON parse error: {}", e)))?,
            ConfigFormat::Toml => toml::from_str(&content)
                .map_err(|e| RspmError::ConfigParseError(format!("TOML parse error: {}", e)))?,
        };

        // Stop old processes
        for name in &watched.processes {
            if let Err(e) = process_manager.stop_process(name, false).await {
                warn!("Failed to stop process '{}': {}", name, e);
            }
            if let Err(e) = process_manager.delete_process(name).await {
                warn!("Failed to delete process '{}': {}", name, e);
            }
        }

        // Start new processes
        for config in config_file.processes {
            let name = config.name.clone();
            match process_manager.start_process(config).await {
                Ok(id) => {
                    info!("Reloaded process '{}' from config: {}", name, id);
                }
                Err(e) => {
                    error!("Failed to start process from config: {}", e);
                }
            }
        }

        Ok(())
    }

    /// Get list of watched config files
    pub fn get_watched_configs(&self) -> Vec<String> {
        self.watched_configs.keys().cloned().collect()
    }
}
