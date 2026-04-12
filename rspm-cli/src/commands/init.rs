use rspm_common::{ProcessConfig, Result};
use std::collections::HashMap;
use std::fs;
use std::io::{self, Write};

/// Interactive initialization command to generate process configuration file
pub async fn init() -> Result<()> {
    println!("🚀 RSPM Interactive Configuration Generator\n");
    println!("This will guide you through creating a process configuration file.\n");

    // Collect process configurations
    let mut processes: Vec<ProcessConfig> = Vec::new();

    loop {
        println!("--- Process Configuration #{} ---\n", processes.len() + 1);

        let config = collect_process_config().await?;
        processes.push(config);

        // Ask if user wants to add another process
        print!("\n📦 Add another process? (y/N): ");
        io::stdout().flush().unwrap();

        let mut answer = String::new();
        io::stdin().read_line(&mut answer).unwrap();

        if !answer.trim().eq_ignore_ascii_case("y") {
            break;
        }
        println!();
    }

    // Ask for output format
    println!("\n📄 Select configuration format:");
    println!("  1) YAML (.yaml)");
    println!("  2) JSON (.json)");
    println!("  3) TOML (.toml)");
    print!("\nChoose format [1-3] (default: 1): ");
    io::stdout().flush().unwrap();

    let mut format_input = String::new();
    io::stdin().read_line(&mut format_input).unwrap();

    let format = match format_input.trim() {
        "2" => "json",
        "3" => "toml",
        _ => "yaml",
    };

    // Ask for output filename
    let default_filename = match format {
        "json" => "ecosystem.json",
        "toml" => "ecosystem.toml",
        _ => "ecosystem.yaml",
    };

    print!("\n💾 Output filename [{}]: ", default_filename);
    io::stdout().flush().unwrap();

    let mut filename_input = String::new();
    io::stdin().read_line(&mut filename_input).unwrap();

    let filename = if filename_input.trim().is_empty() {
        default_filename.to_string()
    } else {
        filename_input.trim().to_string()
    };

    // Write configuration file
    write_config_file(&processes, &filename, format)?;

    println!("\n✅ Configuration file created: {}", filename);
    println!("\n📝 Next steps:");
    println!("   • Review and edit the configuration file if needed");
    println!("   • Load and start processes: rspm load {}", filename);
    println!("   • Or start a single process: rspm start -f {}", filename);

    Ok(())
}

async fn collect_process_config() -> Result<ProcessConfig> {
    // Required: Process name
    print!("📛 Process name: ");
    io::stdout().flush().unwrap();
    let mut name = String::new();
    io::stdin().read_line(&mut name).unwrap();
    let name = name.trim().to_string();

    if name.is_empty() {
        return Err(rspm_common::RspmError::InvalidConfig(
            "Process name is required".to_string(),
        ));
    }

    // Required: Command
    print!("⚙️  Command to execute: ");
    io::stdout().flush().unwrap();
    let mut command = String::new();
    io::stdin().read_line(&mut command).unwrap();
    let command = command.trim().to_string();

    if command.is_empty() {
        return Err(rspm_common::RspmError::InvalidConfig(
            "Command is required".to_string(),
        ));
    }

    // Optional: Arguments
    print!("📝 Command arguments (space-separated, or press Enter to skip): ");
    io::stdout().flush().unwrap();
    let mut args_input = String::new();
    io::stdin().read_line(&mut args_input).unwrap();
    let args: Vec<String> = args_input
        .split_whitespace()
        .map(|s| s.to_string())
        .collect();

    // Optional: Working directory
    print!("📂 Working directory (or press Enter for current): ");
    io::stdout().flush().unwrap();
    let mut cwd_input = String::new();
    io::stdin().read_line(&mut cwd_input).unwrap();
    let cwd = if cwd_input.trim().is_empty() {
        None
    } else {
        Some(cwd_input.trim().to_string())
    };

    // Optional: Instances
    print!("🔢 Number of instances [1]: ");
    io::stdout().flush().unwrap();
    let mut instances_input = String::new();
    io::stdin().read_line(&mut instances_input).unwrap();
    let instances: u32 = instances_input.trim().parse().unwrap_or(1);

    // Optional: Auto restart
    let autorestart = ask_yes_no("🔄 Auto restart on crash?", true);

    // Optional: Max restarts
    print!("🔁 Max restart attempts before marking as errored [15]: ");
    io::stdout().flush().unwrap();
    let mut max_restarts_input = String::new();
    io::stdin().read_line(&mut max_restarts_input).unwrap();
    let max_restarts: u32 = max_restarts_input.trim().parse().unwrap_or(15);

    // Optional: Memory limit
    print!("💾 Memory limit in MB (0 for unlimited) [0]: ");
    io::stdout().flush().unwrap();
    let mut memory_input = String::new();
    io::stdin().read_line(&mut memory_input).unwrap();
    let max_memory_mb: u32 = memory_input.trim().parse().unwrap_or(0);

    // Optional: Watch mode
    let watch = ask_yes_no("👁️  Watch for file changes (dev mode)?", false);

    // Optional: Watch paths
    let watch_paths = if watch {
        print!("📍 Paths to watch (comma-separated, or press Enter for default): ");
        io::stdout().flush().unwrap();
        let mut paths_input = String::new();
        io::stdin().read_line(&mut paths_input).unwrap();

        if paths_input.trim().is_empty() {
            vec![".".to_string()]
        } else {
            paths_input
                .trim()
                .split(',')
                .map(|s| s.trim().to_string())
                .collect()
        }
    } else {
        Vec::new()
    };

    // Optional: Environment variables
    println!("\n🌍 Environment variables (format: KEY=VALUE, one per line, empty line to finish):");
    let mut env: HashMap<String, String> = HashMap::new();
    loop {
        print!("   ");
        io::stdout().flush().unwrap();
        let mut line = String::new();
        io::stdin().read_line(&mut line).unwrap();

        let line = line.trim();
        if line.is_empty() {
            break;
        }

        if let Some((key, value)) = line.split_once('=') {
            env.insert(key.trim().to_string(), value.trim().to_string());
        } else {
            println!("   ⚠️  Invalid format. Use KEY=VALUE");
        }
    }

    // Optional: Log max size
    print!("📏 Log max size in bytes (default: 10485760 = 10MB): ");
    io::stdout().flush().unwrap();
    let mut log_size_input = String::new();
    io::stdin().read_line(&mut log_size_input).unwrap();
    let log_max_size: u64 = log_size_input.trim().parse().unwrap_or(10 * 1024 * 1024);

    // Optional: Log max files
    print!("📚 Log max files to keep [5]: ");
    io::stdout().flush().unwrap();
    let mut log_files_input = String::new();
    io::stdin().read_line(&mut log_files_input).unwrap();
    let log_max_files: u32 = log_files_input.trim().parse().unwrap_or(5);

    Ok(ProcessConfig {
        name,
        command,
        args,
        env,
        cwd,
        instances,
        autorestart,
        max_restarts,
        max_memory_mb,
        watch,
        watch_paths,
        log_file: None,
        error_file: None,
        log_max_size,
        log_max_files,
        server_type: rspm_common::ServerType::Process,
    })
}

/// Helper function to ask yes/no questions
fn ask_yes_no(question: &str, default: bool) -> bool {
    let default_str = if default { "Y/n" } else { "y/N" };
    print!("{} ({}): ", question, default_str);
    io::stdout().flush().unwrap();

    let mut answer = String::new();
    io::stdin().read_line(&mut answer).unwrap();

    match answer.trim().to_lowercase().as_str() {
        "y" | "yes" => true,
        "n" | "no" => false,
        _ => default,
    }
}

/// Write configuration to file
fn write_config_file(processes: &[ProcessConfig], filename: &str, format: &str) -> Result<()> {
    let output = match format {
        "json" => {
            if processes.len() == 1 {
                serde_json::to_string_pretty(&processes[0])
                    .map_err(|e| rspm_common::RspmError::ConfigError(e.to_string()))?
            } else {
                serde_json::to_string_pretty(&serde_json::json!({
                    "processes": processes
                }))
                .map_err(|e| rspm_common::RspmError::ConfigError(e.to_string()))?
            }
        }
        "toml" => {
            if processes.len() == 1 {
                toml::to_string_pretty(&processes[0])
                    .map_err(|e| rspm_common::RspmError::ConfigError(e.to_string()))?
            } else {
                toml::to_string_pretty(&serde_json::json!({
                    "processes": processes
                }))
                .map_err(|e| rspm_common::RspmError::ConfigError(e.to_string()))?
            }
        }
        _ => {
            if processes.len() == 1 {
                serde_yaml::to_string(&processes[0])
                    .map_err(|e| rspm_common::RspmError::ConfigError(e.to_string()))?
            } else {
                serde_yaml::to_string(&serde_yaml::Value::Mapping({
                    let mut map = serde_yaml::Mapping::new();
                    map.insert(
                        serde_yaml::Value::String("processes".to_string()),
                        serde_yaml::to_value(processes)
                            .map_err(|e| rspm_common::RspmError::ConfigError(e.to_string()))?,
                    );
                    map
                }))
                .map_err(|e| rspm_common::RspmError::ConfigError(e.to_string()))?
            }
        }
    };

    fs::write(filename, output)?;

    Ok(())
}
