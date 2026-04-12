use clap::{Parser, Subcommand};
use rspm_common::Result;

mod client;
mod commands;

#[derive(Parser, Debug)]
#[command(name = "rspm")]
#[command(about = "Rust Process Manager - A PM2-like process management tool", long_about = None)]
#[command(version)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Debug, Subcommand)]
enum Commands {
    /// Start a process
    Start {
        /// Process name
        #[arg(index = 1)]
        name: Option<String>,

        /// Command to run
        #[arg(short = 'C', long)]
        command: Option<String>,

        /// Number of instances
        #[arg(short = 'i', long, default_value = "1")]
        instances: u32,

        /// Working directory
        #[arg(short = 'w', long)]
        cwd: Option<String>,

        /// Environment variables (format: KEY=VALUE)
        #[arg(short = 'e', long)]
        env: Vec<String>,

        /// Config file path (supports .json/.yaml/.yml/.toml)
        #[arg(short = 'f', long)]
        config: Option<String>,

        /// Command and arguments (use -- to separate from name)
        #[arg(index = 2, last = true)]
        args: Vec<String>,
    },

    /// Stop a process
    Stop {
        /// Process ID or name
        #[arg(required = false)]
        id: Option<String>,

        /// Process ID (numeric) - use when name conflicts
        #[arg(long = "id")]
        id_flag: Option<String>,

        /// Process name - use when ID conflicts
        #[arg(long = "name")]
        name_flag: Option<String>,

        /// Force kill
        #[arg(short, long)]
        force: bool,
    },

    /// Restart a process
    Restart {
        /// Process ID or name
        #[arg(required = false)]
        id: Option<String>,

        /// Process ID (numeric) - use when name conflicts
        #[arg(long = "id")]
        id_flag: Option<String>,

        /// Process name - use when ID conflicts
        #[arg(long = "name")]
        name_flag: Option<String>,
    },

    /// Delete a process
    Delete {
        /// Process ID or name
        #[arg(required = false)]
        id: Option<String>,

        /// Process ID (numeric) - use when name conflicts
        #[arg(long = "id")]
        id_flag: Option<String>,

        /// Process name - use when ID conflicts
        #[arg(long = "name")]
        name_flag: Option<String>,

        /// Delete all processes
        #[arg(short, long)]
        all: bool,
    },

    /// List all processes
    List {
        /// Filter by name
        #[arg(short, long)]
        name: Option<String>,
    },

    /// Show process details
    Show {
        /// Process ID or name
        #[arg(required = false)]
        id: Option<String>,

        /// Process ID (numeric) - use when name conflicts
        #[arg(long = "id")]
        id_flag: Option<String>,

        /// Process name - use when ID conflicts
        #[arg(long = "name")]
        name_flag: Option<String>,
    },

    /// View process logs
    Logs {
        /// Process ID or name
        #[arg(required = false)]
        id: Option<String>,

        /// Process ID (numeric) - use when name conflicts
        #[arg(long = "id")]
        id_flag: Option<String>,

        /// Process name - use when ID conflicts
        #[arg(long = "name")]
        name_flag: Option<String>,

        /// Follow log output
        #[arg(short, long)]
        follow: bool,

        /// Number of lines to show
        #[arg(short = 'n', long, default_value = "100")]
        lines: u32,

        /// Show stderr only
        #[arg(long)]
        err: bool,
    },

    /// Scale a process to multiple instances
    Scale {
        /// Process ID or name
        #[arg(required = false)]
        id: Option<String>,

        /// Process ID (numeric) - use when name conflicts
        #[arg(long = "id")]
        id_flag: Option<String>,

        /// Process name - use when ID conflicts
        #[arg(long = "name")]
        name_flag: Option<String>,

        /// Number of instances
        instances: u32,
    },

    /// Stop all processes
    StopAll,

    /// Show daemon status
    Status,

    /// Start the daemon
    StartDaemon {
        /// Configuration file path (default: ~/.rspm/.env)
        #[arg(short = 'c', long)]
        config: Option<String>,
    },

    /// Stop the daemon
    StopDaemon,

    /// Load processes from config file
    Load {
        /// Config file path (supports .json/.yaml/.yml/.toml)
        file: String,

        /// Validate config without starting
        #[arg(long)]
        dry_run: bool,
    },

    /// Interactive initialization to generate config file
    Init,

    /// Schedule management commands
    Schedule {
        #[command(subcommand)]
        command: ScheduleCommands,
    },

    /// Start a static file server
    Serve {
        /// Server name
        #[arg(short, long)]
        name: Option<String>,

        /// Port to listen on (default: 8080)
        #[arg(short, long, default_value = "8080")]
        port: u16,

        /// Host to bind to (default: 127.0.0.1)
        #[arg(short = 'H', long, default_value = "127.0.0.1")]
        host: String,

        /// Directory to serve (default: current directory)
        #[arg(short, long)]
        dir: Option<String>,
    },

    /// Install rspmd as a system service (auto-start on boot)
    InstallService {
        /// Output the service file to stdout instead of installing
        #[arg(short, long)]
        output: bool,
    },

    /// Uninstall rspmd system service
    UninstallService,
}

#[derive(Debug, Subcommand)]
enum ScheduleCommands {
    /// Create a new schedule
    Create {
        /// Schedule name
        #[arg(short, long)]
        name: String,

        /// Process name to operate on
        #[arg(short, long)]
        process: Option<String>,

        /// Cron expression (6 fields: second minute hour day month weekday)
        #[arg(short, long, group = "schedule")]
        cron: Option<String>,

        /// Interval in seconds
        #[arg(short, long, group = "schedule")]
        interval: Option<u64>,

        /// One-time execution at specified time (ISO 8601 format)
        #[arg(short, long, group = "schedule")]
        once: Option<String>,

        /// Action to perform: start, stop, restart
        #[arg(short, long, default_value = "start")]
        action: String,

        /// Custom command to execute (for execute action)
        #[arg(short = 'C', long)]
        command: Option<String>,

        /// Command arguments (for execute action, comma-separated)
        #[arg(long, value_delimiter = ',')]
        args: Vec<String>,

        /// Command and arguments for execute action (use -- to separate)
        #[arg(index = 1, last = true)]
        trailing_args: Vec<String>,

        /// Timezone (default: UTC)
        #[arg(short, long, default_value = "UTC")]
        timezone: String,

        /// Maximum number of runs (0 = unlimited)
        #[arg(short = 'm', long, default_value = "0")]
        max_runs: u32,

        /// Description
        #[arg(short, long)]
        description: Option<String>,

        /// Disable the schedule (create in paused state)
        #[arg(long)]
        disabled: bool,
    },

    /// List all schedules
    List {
        /// Filter by name
        #[arg(short, long)]
        name: Option<String>,
    },

    /// Show schedule details
    Show {
        /// Schedule ID or name
        id: String,
    },

    /// Delete a schedule
    Delete {
        /// Schedule ID or name
        id: Option<String>,

        /// Delete all schedules
        #[arg(short, long)]
        all: bool,
    },

    /// Pause a schedule
    Pause {
        /// Schedule ID or name
        id: String,
    },

    /// Resume a paused schedule
    Resume {
        /// Schedule ID or name
        id: String,
    },

    /// Show execution history
    History {
        /// Schedule ID or name
        id: String,

        /// Number of entries to show
        #[arg(short, long, default_value = "20")]
        limit: u32,
    },
}

/// Helper function to get process identifier from positional arg or --id/--name arguments
fn get_process_id(
    positional: Option<String>,
    id_flag: Option<String>,
    name_flag: Option<String>,
) -> Result<String> {
    // Count how many sources are provided
    let mut count = 0;
    if positional.is_some() {
        count += 1;
    }
    if id_flag.is_some() {
        count += 1;
    }
    if name_flag.is_some() {
        count += 1;
    }

    if count > 1 {
        return Err(rspm_common::RspmError::InvalidConfig(
            "Cannot specify multiple process identifiers. Use either positional argument, --id, or --name (not multiple)".to_string()
        ));
    }

    if count == 0 {
        return Err(rspm_common::RspmError::InvalidConfig(
            "Must specify a process identifier: use positional argument, --id, or --name"
                .to_string(),
        ));
    }

    // Return whichever was provided
    if let Some(i) = id_flag {
        Ok(i)
    } else if let Some(n) = name_flag {
        Ok(n)
    } else {
        Ok(positional.unwrap())
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Commands::Start {
            name,
            command,
            instances,
            cwd,
            env,
            config,
            args,
        } => {
            commands::start(name, command, instances, cwd, env, config, args).await?;
        }
        Commands::Stop {
            id,
            id_flag,
            name_flag,
            force,
        } => {
            let target = get_process_id(id, id_flag, name_flag)?;
            commands::stop(&target, force).await?;
        }
        Commands::Restart {
            id,
            id_flag,
            name_flag,
        } => {
            let target = get_process_id(id, id_flag, name_flag)?;
            commands::restart(&target).await?;
        }
        Commands::Delete {
            id,
            id_flag,
            name_flag,
            all,
        } => {
            if all {
                commands::delete_all().await?;
            } else {
                let target = get_process_id(id, id_flag, name_flag)?;
                commands::delete(&target).await?;
            }
        }
        Commands::List { name } => {
            commands::list(name.as_deref()).await?;
        }
        Commands::Show {
            id,
            id_flag,
            name_flag,
        } => {
            let target = get_process_id(id, id_flag, name_flag)?;
            commands::show(&target).await?;
        }
        Commands::Logs {
            id,
            id_flag,
            name_flag,
            follow,
            lines,
            err,
        } => {
            let target = get_process_id(id, id_flag, name_flag)?;
            commands::logs(&target, follow, lines, err).await?;
        }
        Commands::Scale {
            id,
            id_flag,
            name_flag,
            instances,
        } => {
            let target = get_process_id(id, id_flag, name_flag)?;
            commands::scale(&target, instances).await?;
        }
        Commands::StopAll => {
            commands::stop_all().await?;
        }
        Commands::Status => {
            commands::status().await?;
        }
        Commands::StartDaemon { config } => {
            let config_path = config.map(std::path::PathBuf::from);
            commands::start_daemon(config_path).await?;
        }
        Commands::StopDaemon => {
            commands::stop_daemon().await?;
        }
        Commands::Load { file, dry_run } => {
            commands::load(&file, dry_run).await?;
        }
        Commands::Init => {
            commands::init().await?;
        }
        Commands::Schedule { command } => match command {
            ScheduleCommands::Create {
                name,
                process,
                cron,
                interval,
                once,
                action,
                command,
                args,
                trailing_args,
                timezone,
                max_runs,
                description,
                disabled,
            } => {
                commands::schedule::create(
                    name,
                    process,
                    cron,
                    interval,
                    once,
                    action,
                    command,
                    args,
                    trailing_args,
                    timezone,
                    max_runs,
                    description,
                    disabled,
                )
                .await?;
            }
            ScheduleCommands::List { name } => {
                commands::schedule::list(name).await?;
            }
            ScheduleCommands::Show { id } => {
                commands::schedule::show(&id).await?;
            }
            ScheduleCommands::Delete { id, all } => {
                if all {
                    commands::schedule::delete_all().await?;
                } else if let Some(schedule_id) = id {
                    commands::schedule::delete(&schedule_id).await?;
                } else {
                    return Err(rspm_common::RspmError::InvalidConfig(
                        "Either specify a schedule ID or use --all to delete all schedules"
                            .to_string(),
                    ));
                }
            }
            ScheduleCommands::Pause { id } => {
                commands::schedule::pause(&id).await?;
            }
            ScheduleCommands::Resume { id } => {
                commands::schedule::resume(&id).await?;
            }
            ScheduleCommands::History { id, limit } => {
                commands::schedule::history(&id, limit).await?;
            }
        },
        Commands::Serve {
            name,
            port,
            host,
            dir,
        } => {
            commands::serve(name, port, host, dir).await?;
        }
        Commands::InstallService { output } => {
            commands::install_service(output).await?;
        }
        Commands::UninstallService => {
            commands::uninstall_service().await?;
        }
    }

    Ok(())
}
