use rspm_common::{ProcessConfig, Result, RspmError, ScheduleInfo};
use sqlx::{Pool, Sqlite, sqlite::SqlitePoolOptions};
use std::collections::HashMap;
use std::path::Path;

/// Persistent state store for processes using SQLite
pub struct StateStore {
    pool: Pool<Sqlite>,
}

impl StateStore {
    /// Create a new StateStore with the given database path
    /// The database file and parent directories will be created if they don't exist
    pub async fn new(db_path: &Path) -> Result<Self> {
        // Ensure parent directory exists (using sync version for reliability)
        if let Some(parent) = db_path.parent() {
            std::fs::create_dir_all(parent).map_err(RspmError::IoError)?;
        }

        // Convert path to absolute for SQLite
        let abs_path = if db_path.is_absolute() {
            db_path.to_path_buf()
        } else {
            std::env::current_dir()
                .map(|cwd| cwd.join(db_path))
                .unwrap_or_else(|_| db_path.to_path_buf())
        };

        // For Windows, we need to use the file:// scheme with proper path
        // Convert backslashes to forward slashes and ensure proper URL format
        let path_str = abs_path.to_string_lossy().replace('\\', "/");
        // Use sqlite:// with mode=rwc to create the database file if it doesn't exist
        let db_url = if path_str.starts_with('/') {
            // Unix absolute path
            format!("sqlite://{}?mode=rwc", path_str)
        } else {
            // Windows path (e.g., C:/...) - use three slashes after sqlite:
            format!("sqlite:///{}?mode=rwc", path_str)
        };

        // Create connection pool
        let pool = SqlitePoolOptions::new()
            .max_connections(5)
            .connect(&db_url)
            .await
            .map_err(|e| {
                RspmError::DatabaseError(format!("Failed to connect to database: {}", e))
            })?;

        let store = Self { pool };
        store.init().await?;

        Ok(store)
    }

    /// Initialize the database schema
    async fn init(&self) -> Result<()> {
        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS processes (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                name TEXT UNIQUE NOT NULL,
                command TEXT NOT NULL,
                args TEXT NOT NULL,  -- JSON array
                env TEXT NOT NULL,   -- JSON object
                cwd TEXT,
                instances INTEGER NOT NULL DEFAULT 1,
                autorestart INTEGER NOT NULL DEFAULT 1,
                max_restarts INTEGER NOT NULL DEFAULT 15,
                max_memory_mb INTEGER NOT NULL DEFAULT 0,
                watch INTEGER NOT NULL DEFAULT 0,
                watch_paths TEXT NOT NULL,  -- JSON array
                log_file TEXT,
                error_file TEXT,
                log_max_size INTEGER NOT NULL DEFAULT 10485760,
                log_max_files INTEGER NOT NULL DEFAULT 5,
                server_type INTEGER NOT NULL DEFAULT 0,  -- 0: process, 1: static_server
                created_at DATETIME DEFAULT CURRENT_TIMESTAMP,
                updated_at DATETIME DEFAULT CURRENT_TIMESTAMP
            )
            "#,
        )
        .execute(&self.pool)
        .await
        .map_err(|e| RspmError::DatabaseError(format!("Failed to create table: {}", e)))?;

        // Create index on name for faster lookups
        sqlx::query(
            r#"
            CREATE INDEX IF NOT EXISTS idx_processes_name ON processes(name)
            "#,
        )
        .execute(&self.pool)
        .await
        .map_err(|e| RspmError::DatabaseError(format!("Failed to create index: {}", e)))?;

        // Create schedules table for cron jobs
        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS schedules (
                id TEXT PRIMARY KEY,
                name TEXT UNIQUE NOT NULL,
                process_name TEXT,
                schedule_type TEXT NOT NULL,
                schedule_value TEXT NOT NULL,
                action_type TEXT NOT NULL,
                action_command TEXT,
                action_args TEXT,
                enabled INTEGER NOT NULL DEFAULT 1,
                timezone TEXT NOT NULL DEFAULT 'UTC',
                max_runs INTEGER NOT NULL DEFAULT 0,
                description TEXT,
                status TEXT NOT NULL DEFAULT 'active',
                created_at DATETIME DEFAULT CURRENT_TIMESTAMP,
                updated_at DATETIME DEFAULT CURRENT_TIMESTAMP,
                last_run DATETIME,
                next_run DATETIME,
                run_count INTEGER NOT NULL DEFAULT 0,
                success_count INTEGER NOT NULL DEFAULT 0,
                fail_count INTEGER NOT NULL DEFAULT 0
            )
            "#,
        )
        .execute(&self.pool)
        .await
        .map_err(|e| {
            RspmError::DatabaseError(format!("Failed to create schedules table: {}", e))
        })?;

        // Create index on schedules name
        sqlx::query(
            r#"
            CREATE INDEX IF NOT EXISTS idx_schedules_name ON schedules(name)
            "#,
        )
        .execute(&self.pool)
        .await
        .map_err(|e| {
            RspmError::DatabaseError(format!("Failed to create schedules index: {}", e))
        })?;

        // Create schedule executions table for execution history
        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS schedule_executions (
                id TEXT PRIMARY KEY,
                schedule_id TEXT NOT NULL,
                started_at DATETIME NOT NULL,
                ended_at DATETIME,
                status TEXT NOT NULL,
                output TEXT,
                error TEXT,
                FOREIGN KEY (schedule_id) REFERENCES schedules(id)
            )
            "#,
        )
        .execute(&self.pool)
        .await
        .map_err(|e| {
            RspmError::DatabaseError(format!("Failed to create schedule_executions table: {}", e))
        })?;

        // Create index on schedule_executions schedule_id
        sqlx::query(
            r#"
            CREATE INDEX IF NOT EXISTS idx_exec_schedule_id ON schedule_executions(schedule_id)
            "#,
        )
        .execute(&self.pool)
        .await
        .map_err(|e| {
            RspmError::DatabaseError(format!("Failed to create execution index: {}", e))
        })?;

        Ok(())
    }

    /// Load saved process configurations from database
    pub async fn load(&self) -> Result<HashMap<String, ProcessConfig>> {
        let rows = sqlx::query_as::<_, ProcessRow>(
            r#"
            SELECT
                id, name, command, args, env, cwd, instances,
                autorestart, max_restarts, max_memory_mb, watch,
                watch_paths, log_file, error_file, log_max_size, log_max_files,
                server_type
            FROM processes
            "#,
        )
        .fetch_all(&self.pool)
        .await
        .map_err(|e| RspmError::DatabaseError(format!("Failed to load processes: {}", e)))?;

        let mut processes = HashMap::new();
        for row in rows {
            let config = row.to_process_config()?;
            processes.insert(config.name.clone(), config);
        }

        Ok(processes)
    }

    /// Save process configurations to database
    pub async fn save(&self, processes: &HashMap<String, ProcessConfig>) -> Result<()> {
        for config in processes.values() {
            self.save_process(config).await?;
        }
        Ok(())
    }

    /// Add or update a process configuration
    /// Returns the database ID of the process
    pub async fn save_process(&self, config: &ProcessConfig) -> Result<i64> {
        let args_json = serde_json::to_string(&config.args)
            .map_err(|e| RspmError::ConfigParseError(format!("Failed to serialize args: {}", e)))?;
        let env_json = serde_json::to_string(&config.env)
            .map_err(|e| RspmError::ConfigParseError(format!("Failed to serialize env: {}", e)))?;
        let watch_paths_json = serde_json::to_string(&config.watch_paths).map_err(|e| {
            RspmError::ConfigParseError(format!("Failed to serialize watch_paths: {}", e))
        })?;

        // Check if process exists
        let existing = sqlx::query_scalar::<_, i64>("SELECT id FROM processes WHERE name = ?")
            .bind(&config.name)
            .fetch_optional(&self.pool)
            .await
            .map_err(|e| {
                RspmError::DatabaseError(format!("Failed to check existing process: {}", e))
            })?;

        if let Some(id) = existing {
            // Update existing
            let server_type = match config.server_type {
                rspm_common::ServerType::StaticServer => 1i64,
                _ => 0i64,
            };
            sqlx::query(
                r#"
                UPDATE processes SET
                    command = ?2, args = ?3, env = ?4, cwd = ?5, instances = ?6,
                    autorestart = ?7, max_restarts = ?8, max_memory_mb = ?9, watch = ?10,
                    watch_paths = ?11, log_file = ?12, error_file = ?13, log_max_size = ?14,
                    log_max_files = ?15, server_type = ?16, updated_at = CURRENT_TIMESTAMP
                WHERE id = ?1
                "#,
            )
            .bind(id)
            .bind(&config.command)
            .bind(&args_json)
            .bind(&env_json)
            .bind(&config.cwd)
            .bind(config.instances as i64)
            .bind(config.autorestart as i64)
            .bind(config.max_restarts as i64)
            .bind(config.max_memory_mb as i64)
            .bind(config.watch as i64)
            .bind(&watch_paths_json)
            .bind(&config.log_file)
            .bind(&config.error_file)
            .bind(config.log_max_size as i64)
            .bind(config.log_max_files as i64)
            .bind(server_type)
            .execute(&self.pool)
            .await
            .map_err(|e| RspmError::DatabaseError(format!("Failed to update process: {}", e)))?;

            Ok(id)
        } else {
            // Insert new
            let server_type = match config.server_type {
                rspm_common::ServerType::StaticServer => 1i64,
                _ => 0i64,
            };
            let result = sqlx::query(
                r#"
                INSERT INTO processes (
                    name, command, args, env, cwd, instances,
                    autorestart, max_restarts, max_memory_mb, watch,
                    watch_paths, log_file, error_file, log_max_size, log_max_files, server_type
                ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16)
                "#,
            )
            .bind(&config.name)
            .bind(&config.command)
            .bind(&args_json)
            .bind(&env_json)
            .bind(&config.cwd)
            .bind(config.instances as i64)
            .bind(config.autorestart as i64)
            .bind(config.max_restarts as i64)
            .bind(config.max_memory_mb as i64)
            .bind(config.watch as i64)
            .bind(&watch_paths_json)
            .bind(&config.log_file)
            .bind(&config.error_file)
            .bind(config.log_max_size as i64)
            .bind(config.log_max_files as i64)
            .bind(server_type)
            .execute(&self.pool)
            .await
            .map_err(|e| RspmError::DatabaseError(format!("Failed to insert process: {}", e)))?;

            Ok(result.last_insert_rowid())
        }
    }

    /// Insert a new process configuration (for multi-instance, always creates new record)
    /// Returns the database ID of the process
    pub async fn insert_process(&self, config: &ProcessConfig) -> Result<i64> {
        let args_json = serde_json::to_string(&config.args)
            .map_err(|e| RspmError::ConfigParseError(format!("Failed to serialize args: {}", e)))?;
        let env_json = serde_json::to_string(&config.env)
            .map_err(|e| RspmError::ConfigParseError(format!("Failed to serialize env: {}", e)))?;
        let watch_paths_json = serde_json::to_string(&config.watch_paths).map_err(|e| {
            RspmError::ConfigParseError(format!("Failed to serialize watch_paths: {}", e))
        })?;

        let server_type = match config.server_type {
            rspm_common::ServerType::StaticServer => 1i64,
            _ => 0i64,
        };

        let result = sqlx::query(
            r#"
            INSERT INTO processes (
                name, command, args, env, cwd, instances,
                autorestart, max_restarts, max_memory_mb, watch,
                watch_paths, log_file, error_file, log_max_size, log_max_files, server_type
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16)
            "#,
        )
        .bind(&config.name)
        .bind(&config.command)
        .bind(&args_json)
        .bind(&env_json)
        .bind(&config.cwd)
        .bind(config.instances as i64)
        .bind(config.autorestart as i64)
        .bind(config.max_restarts as i64)
        .bind(config.max_memory_mb as i64)
        .bind(config.watch as i64)
        .bind(&watch_paths_json)
        .bind(&config.log_file)
        .bind(&config.error_file)
        .bind(config.log_max_size as i64)
        .bind(config.log_max_files as i64)
        .bind(server_type)
        .execute(&self.pool)
        .await
        .map_err(|e| RspmError::DatabaseError(format!("Failed to insert process: {}", e)))?;

        Ok(result.last_insert_rowid())
    }

    /// Remove a process configuration by name
    pub async fn remove_process(&self, name: &str) -> Result<()> {
        sqlx::query("DELETE FROM processes WHERE name = ?")
            .bind(name)
            .execute(&self.pool)
            .await
            .map_err(|e| RspmError::DatabaseError(format!("Failed to remove process: {}", e)))?;

        Ok(())
    }

    /// Remove a process configuration by ID
    pub async fn remove_process_by_id(&self, id: i64) -> Result<()> {
        sqlx::query("DELETE FROM processes WHERE id = ?")
            .bind(id)
            .execute(&self.pool)
            .await
            .map_err(|e| {
                RspmError::DatabaseError(format!("Failed to remove process by id: {}", e))
            })?;

        Ok(())
    }

    /// Get a specific process configuration by ID
    pub async fn get_by_id(&self, id: i64) -> Result<Option<ProcessConfig>> {
        let row = sqlx::query_as::<_, ProcessRow>(
            r#"
            SELECT
                id, name, command, args, env, cwd, instances,
                autorestart, max_restarts, max_memory_mb, watch,
                watch_paths, log_file, error_file, log_max_size, log_max_files,
                server_type
            FROM processes
            WHERE id = ?
            "#,
        )
        .bind(id)
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| RspmError::DatabaseError(format!("Failed to get process by id: {}", e)))?;

        match row {
            Some(row) => Ok(Some(row.to_process_config()?)),
            None => Ok(None),
        }
    }

    /// Get process ID by name
    pub async fn get_id_by_name(&self, name: &str) -> Result<Option<i64>> {
        let row: Option<(i64,)> = sqlx::query_as("SELECT id FROM processes WHERE name = ?")
            .bind(name)
            .fetch_optional(&self.pool)
            .await
            .map_err(|e| RspmError::DatabaseError(format!("Failed to get process id: {}", e)))?;

        Ok(row.map(|r| r.0))
    }

    /// Get all processes with their IDs
    pub async fn get_all_with_ids(&self) -> Result<Vec<(i64, ProcessConfig)>> {
        let rows = sqlx::query_as::<_, ProcessRow>(
            r#"
            SELECT
                id, name, command, args, env, cwd, instances,
                autorestart, max_restarts, max_memory_mb, watch,
                watch_paths, log_file, error_file, log_max_size, log_max_files,
                server_type
            FROM processes
            ORDER BY id
            "#,
        )
        .fetch_all(&self.pool)
        .await
        .map_err(|e| {
            RspmError::DatabaseError(format!("Failed to load processes with ids: {}", e))
        })?;

        let mut result = Vec::new();
        for row in rows {
            let id = row.id;
            let config = row.to_process_config()?;
            result.push((id, config));
        }

        Ok(result)
    }

    /// Get all saved process configurations
    pub async fn get_all(&self) -> Result<HashMap<String, ProcessConfig>> {
        self.load().await
    }

    /// Get a specific process configuration
    pub async fn get(&self, name: &str) -> Result<Option<ProcessConfig>> {
        let row = sqlx::query_as::<_, ProcessRow>(
            r#"
            SELECT
                id, name, command, args, env, cwd, instances,
                autorestart, max_restarts, max_memory_mb, watch,
                watch_paths, log_file, error_file, log_max_size, log_max_files,
                server_type
            FROM processes
            WHERE name = ?
            "#,
        )
        .bind(name)
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| RspmError::DatabaseError(format!("Failed to get process: {}", e)))?;

        match row {
            Some(row) => Ok(Some(row.to_process_config()?)),
            None => Ok(None),
        }
    }

    // ==================== Schedule Methods ====================

    /// Save a schedule configuration
    pub async fn save_schedule(&self, info: &ScheduleInfo) -> Result<()> {
        let schedule_type = match &info.config.schedule {
            rspm_common::ScheduleType::Cron(_) => "cron",
            rspm_common::ScheduleType::Interval(_) => "interval",
            rspm_common::ScheduleType::Once(_) => "once",
        };

        let schedule_value = match &info.config.schedule {
            rspm_common::ScheduleType::Cron(expr) => expr.clone(),
            rspm_common::ScheduleType::Interval(secs) => secs.to_string(),
            rspm_common::ScheduleType::Once(dt) => dt.to_rfc3339(),
        };

        let action_type = match &info.config.action {
            rspm_common::ScheduleAction::Start => "start",
            rspm_common::ScheduleAction::Stop => "stop",
            rspm_common::ScheduleAction::Restart => "restart",
            rspm_common::ScheduleAction::Execute { .. } => "execute",
        };

        let (action_command, action_args) = match &info.config.action {
            rspm_common::ScheduleAction::Execute { command, args } => {
                let args_json = serde_json::to_string(args).map_err(|e| {
                    RspmError::ConfigParseError(format!("Failed to serialize args: {}", e))
                })?;
                (Some(command.clone()), Some(args_json))
            }
            _ => (None, None),
        };

        let status = match &info.status {
            rspm_common::ScheduleStatus::Active => "active",
            rspm_common::ScheduleStatus::Paused => "paused",
            rspm_common::ScheduleStatus::Completed => "completed",
            rspm_common::ScheduleStatus::Error(_) => "error",
        };

        let last_run_str = info.last_run.map(|dt| dt.to_rfc3339());
        let next_run_str = info.next_run.map(|dt| dt.to_rfc3339());

        // Check if schedule exists
        let existing: Option<(String,)> = sqlx::query_as("SELECT id FROM schedules WHERE id = ?")
            .bind(&info.id)
            .fetch_optional(&self.pool)
            .await
            .map_err(|e| {
                RspmError::DatabaseError(format!("Failed to check existing schedule: {}", e))
            })?;

        if existing.is_some() {
            // Update existing
            sqlx::query(
                r#"
                UPDATE schedules SET
                    name = ?2, process_name = ?3, schedule_type = ?4, schedule_value = ?5,
                    action_type = ?6, action_command = ?7, action_args = ?8, enabled = ?9,
                    timezone = ?10, max_runs = ?11, description = ?12, status = ?13,
                    updated_at = CURRENT_TIMESTAMP, last_run = ?14, next_run = ?15,
                    run_count = ?16, success_count = ?17, fail_count = ?18
                WHERE id = ?1
                "#,
            )
            .bind(&info.id)
            .bind(&info.config.name)
            .bind(&info.config.process_name)
            .bind(schedule_type)
            .bind(&schedule_value)
            .bind(action_type)
            .bind(&action_command)
            .bind(&action_args)
            .bind(info.config.enabled as i64)
            .bind(&info.config.timezone)
            .bind(info.config.max_runs as i64)
            .bind(&info.config.description)
            .bind(status)
            .bind(&last_run_str)
            .bind(&next_run_str)
            .bind(info.run_count as i64)
            .bind(info.success_count as i64)
            .bind(info.fail_count as i64)
            .execute(&self.pool)
            .await
            .map_err(|e| RspmError::DatabaseError(format!("Failed to update schedule: {}", e)))?;
        } else {
            // Insert new
            sqlx::query(
                r#"
                INSERT INTO schedules (
                    id, name, process_name, schedule_type, schedule_value, action_type,
                    action_command, action_args, enabled, timezone, max_runs, description,
                    status, created_at, updated_at, last_run, next_run, run_count,
                    success_count, fail_count
                ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13,
                    CURRENT_TIMESTAMP, CURRENT_TIMESTAMP, ?14, ?15, ?16, ?17, ?18)
                "#,
            )
            .bind(&info.id)
            .bind(&info.config.name)
            .bind(&info.config.process_name)
            .bind(schedule_type)
            .bind(&schedule_value)
            .bind(action_type)
            .bind(&action_command)
            .bind(&action_args)
            .bind(info.config.enabled as i64)
            .bind(&info.config.timezone)
            .bind(info.config.max_runs as i64)
            .bind(&info.config.description)
            .bind(status)
            .bind(&last_run_str)
            .bind(&next_run_str)
            .bind(info.run_count as i64)
            .bind(info.success_count as i64)
            .bind(info.fail_count as i64)
            .execute(&self.pool)
            .await
            .map_err(|e| RspmError::DatabaseError(format!("Failed to insert schedule: {}", e)))?;
        }

        Ok(())
    }

    /// Get all schedules
    pub async fn get_all_schedules(&self) -> Result<Vec<ScheduleInfo>> {
        let rows = sqlx::query_as::<_, ScheduleRow>(
            r#"
            SELECT
                id, name, process_name, schedule_type, schedule_value, action_type,
                action_command, action_args, enabled, timezone, max_runs, description,
                status, created_at, updated_at, last_run, next_run, run_count,
                success_count, fail_count
            FROM schedules
            ORDER BY created_at DESC
            "#,
        )
        .fetch_all(&self.pool)
        .await
        .map_err(|e| RspmError::DatabaseError(format!("Failed to load schedules: {}", e)))?;

        let mut schedules = Vec::new();
        for row in rows {
            if let Ok(info) = row.to_schedule_info() {
                schedules.push(info);
            }
        }

        Ok(schedules)
    }

    /// Get a schedule by ID
    pub async fn get_schedule(&self, id: &str) -> Result<Option<ScheduleInfo>> {
        let row = sqlx::query_as::<_, ScheduleRow>(
            r#"
            SELECT
                id, name, process_name, schedule_type, schedule_value, action_type,
                action_command, action_args, enabled, timezone, max_runs, description,
                status, created_at, updated_at, last_run, next_run, run_count,
                success_count, fail_count
            FROM schedules
            WHERE id = ?
            "#,
        )
        .bind(id)
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| RspmError::DatabaseError(format!("Failed to get schedule: {}", e)))?;

        match row {
            Some(row) => Ok(row.to_schedule_info().ok()),
            None => Ok(None),
        }
    }

    /// Get a schedule by name
    pub async fn get_schedule_by_name(&self, name: &str) -> Result<Option<ScheduleInfo>> {
        let row = sqlx::query_as::<_, ScheduleRow>(
            r#"
            SELECT
                id, name, process_name, schedule_type, schedule_value, action_type,
                action_command, action_args, enabled, timezone, max_runs, description,
                status, created_at, updated_at, last_run, next_run, run_count,
                success_count, fail_count
            FROM schedules
            WHERE name = ?
            "#,
        )
        .bind(name)
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| RspmError::DatabaseError(format!("Failed to get schedule by name: {}", e)))?;

        match row {
            Some(row) => Ok(row.to_schedule_info().ok()),
            None => Ok(None),
        }
    }

    /// Remove a schedule by ID
    pub async fn remove_schedule(&self, id: &str) -> Result<()> {
        // First remove execution history
        sqlx::query("DELETE FROM schedule_executions WHERE schedule_id = ?")
            .bind(id)
            .execute(&self.pool)
            .await
            .map_err(|e| {
                RspmError::DatabaseError(format!("Failed to remove schedule executions: {}", e))
            })?;

        // Then remove schedule
        sqlx::query("DELETE FROM schedules WHERE id = ?")
            .bind(id)
            .execute(&self.pool)
            .await
            .map_err(|e| RspmError::DatabaseError(format!("Failed to remove schedule: {}", e)))?;

        Ok(())
    }

    /// Update schedule status
    pub async fn update_schedule_status(&self, id: &str, status: &str) -> Result<()> {
        sqlx::query(
            r#"
            UPDATE schedules SET status = ?2, updated_at = CURRENT_TIMESTAMP
            WHERE id = ?1
            "#,
        )
        .bind(id)
        .bind(status)
        .execute(&self.pool)
        .await
        .map_err(|e| {
            RspmError::DatabaseError(format!("Failed to update schedule status: {}", e))
        })?;

        Ok(())
    }

    /// Update schedule run info after execution
    pub async fn update_schedule_run(
        &self,
        id: &str,
        last_run: chrono::DateTime<chrono::Utc>,
        next_run: Option<chrono::DateTime<chrono::Utc>>,
        success: bool,
    ) -> Result<()> {
        let last_run_str = last_run.to_rfc3339();
        let next_run_str = next_run.map(|dt| dt.to_rfc3339());

        let query = if success {
            r#"
            UPDATE schedules SET
                last_run = ?2, next_run = ?3, run_count = run_count + 1,
                success_count = success_count + 1, updated_at = CURRENT_TIMESTAMP
            WHERE id = ?1
            "#
        } else {
            r#"
            UPDATE schedules SET
                last_run = ?2, next_run = ?3, run_count = run_count + 1,
                fail_count = fail_count + 1, updated_at = CURRENT_TIMESTAMP
            WHERE id = ?1
            "#
        };

        sqlx::query(query)
            .bind(id)
            .bind(&last_run_str)
            .bind(&next_run_str)
            .execute(&self.pool)
            .await
            .map_err(|e| {
                RspmError::DatabaseError(format!("Failed to update schedule run: {}", e))
            })?;

        Ok(())
    }

    /// Record a schedule execution
    pub async fn record_execution(&self, execution: &rspm_common::ScheduleExecution) -> Result<()> {
        let ended_at_str = execution.ended_at.map(|dt| dt.to_rfc3339());
        let status = match execution.status {
            rspm_common::ExecutionStatus::Running => "running",
            rspm_common::ExecutionStatus::Success => "success",
            rspm_common::ExecutionStatus::Failed => "failed",
            rspm_common::ExecutionStatus::Timeout => "timeout",
        };

        sqlx::query(
            r#"
            INSERT INTO schedule_executions (
                id, schedule_id, started_at, ended_at, status, output, error
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
            "#,
        )
        .bind(&execution.id)
        .bind(&execution.schedule_id)
        .bind(execution.started_at.to_rfc3339())
        .bind(&ended_at_str)
        .bind(status)
        .bind(&execution.output)
        .bind(&execution.error)
        .execute(&self.pool)
        .await
        .map_err(|e| RspmError::DatabaseError(format!("Failed to record execution: {}", e)))?;

        Ok(())
    }

    /// Get execution history for a schedule
    pub async fn get_executions(
        &self,
        schedule_id: &str,
        limit: i64,
    ) -> Result<Vec<rspm_common::ScheduleExecution>> {
        let rows = sqlx::query_as::<_, ExecutionRow>(
            r#"
            SELECT id, schedule_id, started_at, ended_at, status, output, error
            FROM schedule_executions
            WHERE schedule_id = ?
            ORDER BY started_at DESC
            LIMIT ?
            "#,
        )
        .bind(schedule_id)
        .bind(limit)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| RspmError::DatabaseError(format!("Failed to load executions: {}", e)))?;

        let mut executions = Vec::new();
        for row in rows {
            if let Ok(exec) = row.to_execution() {
                executions.push(exec);
            }
        }

        Ok(executions)
    }
}

/// Database row representation for a schedule
#[derive(sqlx::FromRow)]
struct ScheduleRow {
    id: String,
    name: String,
    process_name: Option<String>,
    schedule_type: String,
    schedule_value: String,
    action_type: String,
    action_command: Option<String>,
    action_args: Option<String>,
    enabled: i64,
    timezone: String,
    max_runs: i64,
    description: Option<String>,
    status: String,
    created_at: chrono::DateTime<chrono::Utc>,
    updated_at: chrono::DateTime<chrono::Utc>,
    last_run: Option<chrono::DateTime<chrono::Utc>>,
    next_run: Option<chrono::DateTime<chrono::Utc>>,
    run_count: i64,
    success_count: i64,
    fail_count: i64,
}

impl ScheduleRow {
    fn to_schedule_info(&self) -> Result<ScheduleInfo> {
        use rspm_common::*;

        let schedule = match self.schedule_type.as_str() {
            "cron" => ScheduleType::Cron(self.schedule_value.clone()),
            "interval" => ScheduleType::Interval(self.schedule_value.parse().unwrap_or(60)),
            "once" => {
                let dt = self
                    .schedule_value
                    .parse::<chrono::DateTime<chrono::Utc>>()
                    .map_err(|e| RspmError::ConfigParseError(format!("Invalid datetime: {}", e)))?;
                ScheduleType::Once(dt)
            }
            _ => ScheduleType::Interval(60),
        };

        let action = match self.action_type.as_str() {
            "start" => ScheduleAction::Start,
            "stop" => ScheduleAction::Stop,
            "restart" => ScheduleAction::Restart,
            "execute" => {
                let args = self
                    .action_args
                    .as_ref()
                    .and_then(|s| serde_json::from_str::<Vec<String>>(s).ok())
                    .unwrap_or_default();
                ScheduleAction::Execute {
                    command: self.action_command.clone().unwrap_or_default(),
                    args,
                }
            }
            _ => ScheduleAction::Start,
        };

        let status = match self.status.as_str() {
            "active" => ScheduleStatus::Active,
            "paused" => ScheduleStatus::Paused,
            "completed" => ScheduleStatus::Completed,
            s if s.starts_with("error") => ScheduleStatus::Error(s.to_string()),
            _ => ScheduleStatus::Active,
        };

        let config = ScheduleConfig {
            id: Some(self.id.clone()),
            name: self.name.clone(),
            process_name: self.process_name.clone(),
            schedule,
            action,
            enabled: self.enabled != 0,
            timezone: self.timezone.clone(),
            max_runs: self.max_runs as u32,
            description: self.description.clone(),
        };

        Ok(ScheduleInfo {
            id: self.id.clone(),
            config,
            status,
            created_at: self.created_at,
            updated_at: self.updated_at,
            last_run: self.last_run,
            next_run: self.next_run,
            run_count: self.run_count as u32,
            success_count: self.success_count as u32,
            fail_count: self.fail_count as u32,
        })
    }
}

/// Database row representation for an execution
#[derive(sqlx::FromRow)]
struct ExecutionRow {
    id: String,
    schedule_id: String,
    started_at: chrono::DateTime<chrono::Utc>,
    ended_at: Option<chrono::DateTime<chrono::Utc>>,
    status: String,
    output: Option<String>,
    error: Option<String>,
}

impl ExecutionRow {
    fn to_execution(&self) -> Result<rspm_common::ScheduleExecution> {
        use rspm_common::*;

        let status = match self.status.as_str() {
            "running" => ExecutionStatus::Running,
            "success" => ExecutionStatus::Success,
            "failed" => ExecutionStatus::Failed,
            "timeout" => ExecutionStatus::Timeout,
            _ => ExecutionStatus::Failed,
        };

        Ok(ScheduleExecution {
            id: self.id.clone(),
            schedule_id: self.schedule_id.clone(),
            started_at: self.started_at,
            ended_at: self.ended_at,
            status,
            output: self.output.clone(),
            error: self.error.clone(),
        })
    }
}

/// Process record with database ID
#[derive(Debug, Clone)]
pub struct ProcessRecord {
    pub id: i64,
    pub config: ProcessConfig,
}

/// Database row representation for a process
#[derive(sqlx::FromRow)]
struct ProcessRow {
    id: i64,
    name: String,
    command: String,
    args: String,
    env: String,
    cwd: Option<String>,
    instances: i64,
    autorestart: i64,
    max_restarts: i64,
    max_memory_mb: i64,
    watch: i64,
    watch_paths: String,
    log_file: Option<String>,
    error_file: Option<String>,
    log_max_size: i64,
    log_max_files: i64,
    server_type: i64,
}

impl ProcessRow {
    fn to_process_config(&self) -> Result<ProcessConfig> {
        use rspm_common::ServerType;

        let args: Vec<String> = serde_json::from_str(&self.args).map_err(|e| {
            RspmError::ConfigParseError(format!("Failed to deserialize args: {}", e))
        })?;
        let env: std::collections::HashMap<String, String> = serde_json::from_str(&self.env)
            .map_err(|e| {
                RspmError::ConfigParseError(format!("Failed to deserialize env: {}", e))
            })?;
        let watch_paths: Vec<String> = serde_json::from_str(&self.watch_paths).map_err(|e| {
            RspmError::ConfigParseError(format!("Failed to deserialize watch_paths: {}", e))
        })?;

        Ok(ProcessConfig {
            name: self.name.clone(),
            command: self.command.clone(),
            args,
            env,
            cwd: self.cwd.clone(),
            instances: self.instances as u32,
            autorestart: self.autorestart != 0,
            max_restarts: self.max_restarts as u32,
            max_memory_mb: self.max_memory_mb as u32,
            watch: self.watch != 0,
            watch_paths,
            log_file: self.log_file.clone(),
            error_file: self.error_file.clone(),
            log_max_size: self.log_max_size as u64,
            log_max_files: self.log_max_files as u32,
            server_type: if self.server_type == 1 {
                ServerType::StaticServer
            } else {
                ServerType::Process
            },
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rspm_common::ServerType;
    use std::collections::HashMap;
    use std::path::PathBuf;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn get_test_db_path(name: &str) -> PathBuf {
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_millis();
        // Use unique subdirectory for each test to avoid conflicts
        PathBuf::from(format!("/tmp/rspm_test_{}_{}/rspm.db", name, timestamp))
    }

    fn create_test_config(name: &str) -> ProcessConfig {
        let mut env = HashMap::new();
        env.insert("KEY1".to_string(), "value1".to_string());
        env.insert("KEY2".to_string(), "value2".to_string());

        ProcessConfig {
            name: name.to_string(),
            command: "/usr/bin/test".to_string(),
            args: vec!["arg1".to_string(), "arg2".to_string()],
            env,
            cwd: Some("/tmp".to_string()),
            instances: 2,
            autorestart: true,
            max_restarts: 10,
            max_memory_mb: 512,
            watch: false,
            watch_paths: vec!["/path/to/watch".to_string()],
            log_file: Some("/tmp/test.log".to_string()),
            error_file: Some("/tmp/test.err".to_string()),
            log_max_size: 10485760,
            log_max_files: 5,
            server_type: ServerType::Process,
        }
    }

    #[tokio::test]
    async fn test_state_store_create_and_load() {
        let temp_dir = tempfile::tempdir().unwrap();
        let db_path = temp_dir.path().join("test.db");

        // Create state store
        let store = StateStore::new(&db_path).await.unwrap();

        // Create and save a process config
        let config = create_test_config("test-app");
        store.save_process(&config).await.unwrap();

        // Load all processes
        let processes = store.load().await.unwrap();
        assert_eq!(processes.len(), 1);
        assert!(processes.contains_key("test-app"));

        // Verify loaded config matches original
        let loaded = &processes["test-app"];
        assert_eq!(loaded.name, "test-app");
        assert_eq!(loaded.command, "/usr/bin/test");
        assert_eq!(loaded.args, vec!["arg1", "arg2"]);
        assert_eq!(loaded.instances, 2);
        assert_eq!(loaded.max_restarts, 10);
        assert_eq!(loaded.max_memory_mb, 512);
        assert!(loaded.autorestart);

        // Cleanup (temp_dir automatically cleaned up when dropped)
    }

    #[tokio::test]
    async fn test_state_store_get() {
        let db_path = get_test_db_path("get");

        let store = StateStore::new(&db_path).await.unwrap();

        // Save a process
        let config = create_test_config("my-app");
        store.save_process(&config).await.unwrap();

        // Get the process
        let loaded = store.get("my-app").await.unwrap();
        assert!(loaded.is_some());
        let loaded = loaded.unwrap();
        assert_eq!(loaded.name, "my-app");

        // Get non-existent process
        let not_found = store.get("non-existent").await.unwrap();
        assert!(not_found.is_none());

        // Cleanup
        let _ = tokio::fs::remove_file(&db_path).await;
    }

    #[tokio::test]
    async fn test_state_store_update() {
        let db_path = get_test_db_path("update");

        let store = StateStore::new(&db_path).await.unwrap();

        // Save initial config
        let mut config = create_test_config("update-app");
        store.save_process(&config).await.unwrap();

        // Update config
        config.instances = 5;
        config.max_restarts = 20;
        store.save_process(&config).await.unwrap();

        // Verify update
        let loaded = store.get("update-app").await.unwrap().unwrap();
        assert_eq!(loaded.instances, 5);
        assert_eq!(loaded.max_restarts, 20);

        // Cleanup
        let _ = tokio::fs::remove_file(&db_path).await;
    }

    #[tokio::test]
    async fn test_state_store_remove() {
        let db_path = get_test_db_path("remove");

        let store = StateStore::new(&db_path).await.unwrap();

        // Save and then remove
        let config = create_test_config("remove-app");
        store.save_process(&config).await.unwrap();
        assert_eq!(store.load().await.unwrap().len(), 1);

        store.remove_process("remove-app").await.unwrap();
        assert_eq!(store.load().await.unwrap().len(), 0);

        // Verify it's gone
        let not_found = store.get("remove-app").await.unwrap();
        assert!(not_found.is_none());

        // Cleanup
        let _ = tokio::fs::remove_file(&db_path).await;
    }

    #[tokio::test]
    async fn test_state_store_multiple_processes() {
        let db_path = get_test_db_path("multi");

        let store = StateStore::new(&db_path).await.unwrap();

        // Save multiple processes
        let config1 = create_test_config("app1");
        let config2 = create_test_config("app2");
        let config3 = create_test_config("app3");

        store.save_process(&config1).await.unwrap();
        store.save_process(&config2).await.unwrap();
        store.save_process(&config3).await.unwrap();

        // Load all
        let processes = store.load().await.unwrap();
        assert_eq!(processes.len(), 3);
        assert!(processes.contains_key("app1"));
        assert!(processes.contains_key("app2"));
        assert!(processes.contains_key("app3"));

        // Cleanup
        let _ = tokio::fs::remove_file(&db_path).await;
    }

    #[tokio::test]
    async fn test_state_store_env_serialization() {
        let db_path = get_test_db_path("env");

        let store = StateStore::new(&db_path).await.unwrap();

        // Create config with complex env vars
        let mut config = create_test_config("env-test");
        config
            .env
            .insert("SPECIAL_KEY".to_string(), "special=value".to_string());
        config.env.insert("EMPTY_KEY".to_string(), "".to_string());

        store.save_process(&config).await.unwrap();

        // Verify env is preserved
        let loaded = store.get("env-test").await.unwrap().unwrap();
        assert_eq!(
            loaded.env.get("SPECIAL_KEY"),
            Some(&"special=value".to_string())
        );
        assert_eq!(loaded.env.get("EMPTY_KEY"), Some(&"".to_string()));
        assert_eq!(loaded.env.len(), 4); // 2 original + 2 new

        // Cleanup
        let _ = tokio::fs::remove_file(&db_path).await;
    }

    #[tokio::test]
    async fn test_state_store_id_operations() {
        let db_path = get_test_db_path("id_ops");

        let store = StateStore::new(&db_path).await.unwrap();

        // Save processes
        let config1 = create_test_config("id-app-1");
        let config2 = create_test_config("id-app-2");

        store.save_process(&config1).await.unwrap();
        store.save_process(&config2).await.unwrap();

        // Get all with IDs
        let processes_with_ids = store.get_all_with_ids().await.unwrap();
        assert_eq!(processes_with_ids.len(), 2);

        // Verify IDs are assigned (starting from 1)
        let (id1, _) = &processes_with_ids[0];
        let (id2, _) = &processes_with_ids[1];
        assert_eq!(*id1, 1);
        assert_eq!(*id2, 2);

        // Get by ID
        let loaded_by_id = store.get_by_id(1).await.unwrap();
        assert!(loaded_by_id.is_some());
        assert_eq!(loaded_by_id.unwrap().name, "id-app-1");

        // Get ID by name
        let found_id = store.get_id_by_name("id-app-2").await.unwrap();
        assert_eq!(found_id, Some(2));

        // Remove by ID
        store.remove_process_by_id(1).await.unwrap();

        // Verify removal
        let after_remove = store.get_by_id(1).await.unwrap();
        assert!(after_remove.is_none());

        let remaining = store.load().await.unwrap();
        assert_eq!(remaining.len(), 1);

        // Cleanup
        let _ = tokio::fs::remove_file(&db_path).await;
    }

    #[tokio::test]
    async fn test_state_store_id_persistence() {
        let db_path = get_test_db_path("id_persist");

        // Create store and save process
        {
            let store = StateStore::new(&db_path).await.unwrap();
            let config = create_test_config("persist-app");
            store.save_process(&config).await.unwrap();

            let id = store.get_id_by_name("persist-app").await.unwrap();
            assert_eq!(id, Some(1));
        }

        // Reopen store and verify ID is preserved
        {
            let store = StateStore::new(&db_path).await.unwrap();
            let id = store.get_id_by_name("persist-app").await.unwrap();
            assert_eq!(id, Some(1));

            // Add another process, should get ID 2
            let config2 = create_test_config("persist-app-2");
            store.save_process(&config2).await.unwrap();

            let id2 = store.get_id_by_name("persist-app-2").await.unwrap();
            assert_eq!(id2, Some(2));
        }

        // Cleanup
        let _ = tokio::fs::remove_file(&db_path).await;
    }
}
