use crate::client::create_client;
use rspm_common::{ProcessConfig, Result, RspmError, load_config};

pub async fn start(
    name: Option<String>,
    command: Option<String>,
    instances: u32,
    cwd: Option<String>,
    env: Vec<String>,
    config_file: Option<String>,
    args: Vec<String>,
) -> Result<()> {
    let mut client = create_client().await?;

    // Load config from file if provided
    let mut base_config = if let Some(ref path) = config_file {
        let configs = load_config(path)?;
        if configs.len() > 1 {
            return Err(RspmError::InvalidConfig(
                "Config file contains multiple processes. Use 'rspm load' command instead."
                    .to_string(),
            ));
        }
        configs.into_iter().next().unwrap_or_default()
    } else {
        ProcessConfig::default()
    };

    // Parse args: [name] -- [command] [args...]
    // If name is not provided via --name, first arg is the name
    // Remaining args are command and its arguments
    let (cli_name, cmd_and_args): (Option<String>, Vec<String>) = if name.is_some() {
        // Name provided via --name, all args are command + args
        (name, args)
    } else {
        // Name not provided via --name
        if args.is_empty() {
            if base_config.name.is_empty() {
                return Err(RspmError::InvalidConfig(
                    "No command specified. Use 'rspm start <name> -- <command>' or 'rspm start --name <name> -- <command>'.".to_string(),
                ));
            }
            (None, Vec::new())
        } else {
            // First arg is name, rest are command + args
            (Some(args[0].clone()), args[1..].to_vec())
        }
    };

    // Command and args
    let (cmd, cmd_args) = match command {
        Some(cmd) => (Some(cmd), cmd_and_args),
        None => {
            if cmd_and_args.is_empty() {
                if base_config.command.is_empty() {
                    return Err(RspmError::InvalidConfig(
                        "No command specified. Use 'rspm start <name> -- <command>' or provide -c/--command.".to_string(),
                    ));
                }
                (None, Vec::new())
            } else {
                (Some(cmd_and_args[0].clone()), cmd_and_args[1..].to_vec())
            }
        }
    };

    if let Some(c) = cmd {
        base_config.command = c;
    }
    if !cmd_args.is_empty() {
        base_config.args = cmd_args;
    }

    // Name
    if let Some(n) = cli_name {
        base_config.name = n;
    } else if base_config.name.is_empty() {
        // Generate name from command
        base_config.name = std::path::Path::new(&base_config.command)
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("process")
            .to_string();
    }

    // Instances
    if instances != 1 || config_file.is_none() {
        base_config.instances = instances;
    }

    // Working directory
    if let Some(c) = cwd {
        base_config.cwd = Some(c);
    } else if base_config.cwd.is_none() {
        // Use current working directory as default
        base_config.cwd = std::env::current_dir()
            .ok()
            .map(|p| p.to_string_lossy().to_string());
    }

    // Environment variables
    if !env.is_empty() {
        for e in env {
            let parts: Vec<&str> = e.splitn(2, '=').collect();
            if parts.len() == 2 {
                base_config
                    .env
                    .insert(parts[0].to_string(), parts[1].to_string());
            }
        }
    }

    println!("Starting process '{}'...", base_config.name);

    match client.start_process(base_config).await {
        Ok(id) => {
            println!(
                "{}",
                colored::Colorize::green(format!("Process started with ID: {}", id).as_str())
            );
        }
        Err(e) => {
            eprintln!(
                "{}",
                colored::Colorize::red(format!("Failed to start process: {}", e).as_str())
            );
            return Err(e);
        }
    }

    Ok(())
}
