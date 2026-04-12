use std::path::Path;

use crate::{ConfigFile, ConfigFormat, ProcessConfig, Result, RspmError};

/// Load process configurations from a file
///
/// Automatically detects the format from file extension:
/// - `.json` -> JSON format
/// - `.yaml`, `.yml` -> YAML format
/// - `.toml` -> TOML format
///
/// Supports both single-process and multi-process configurations.
pub fn load_config(path: impl AsRef<Path>) -> Result<Vec<ProcessConfig>> {
    let path = path.as_ref();

    // Detect format from extension
    let format = ConfigFormat::from_path(path).ok_or_else(|| {
        RspmError::UnsupportedConfigFormat(
            path.extension()
                .and_then(|e| e.to_str())
                .unwrap_or("unknown")
                .to_string(),
        )
    })?;

    // Read file content
    let content = std::fs::read_to_string(path)
        .map_err(|e| RspmError::ConfigError(format!("Failed to read config file: {}", e)))?;

    // Parse content
    parse_config(&content, format)
}

/// Parse configuration content with specified format
pub fn parse_config(content: &str, format: ConfigFormat) -> Result<Vec<ProcessConfig>> {
    match format {
        ConfigFormat::Json => parse_json(content),
        ConfigFormat::Yaml => parse_yaml(content),
        ConfigFormat::Toml => parse_toml(content),
    }
}

/// Parse JSON configuration
fn parse_json(content: &str) -> Result<Vec<ProcessConfig>> {
    // Try to parse as multi-process config first
    if let Ok(config_file) = serde_json::from_str::<ConfigFile>(content)
        && !config_file.processes.is_empty()
    {
        return Ok(config_file.into_processes());
    }

    // Try to parse as single process config
    let config: ProcessConfig = serde_json::from_str(content)
        .map_err(|e| RspmError::ConfigParseError(format!("JSON parse error: {}", e)))?;

    Ok(vec![config])
}

/// Parse YAML configuration
fn parse_yaml(content: &str) -> Result<Vec<ProcessConfig>> {
    // Try to parse as multi-process config first
    if let Ok(config_file) = serde_yaml::from_str::<ConfigFile>(content)
        && !config_file.processes.is_empty()
    {
        return Ok(config_file.into_processes());
    }

    // Try to parse as single process config
    let config: ProcessConfig = serde_yaml::from_str(content)
        .map_err(|e| RspmError::ConfigParseError(format!("YAML parse error: {}", e)))?;

    Ok(vec![config])
}

/// Parse TOML configuration
fn parse_toml(content: &str) -> Result<Vec<ProcessConfig>> {
    // Try to parse as multi-process config first
    if let Ok(config_file) = toml::from_str::<ConfigFile>(content)
        && !config_file.processes.is_empty()
    {
        return Ok(config_file.into_processes());
    }

    // Try to parse as single process config
    let config: ProcessConfig = toml::from_str(content)
        .map_err(|e| RspmError::ConfigParseError(format!("TOML parse error: {}", e)))?;

    Ok(vec![config])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_json_single() {
        let content = r#"
        {
            "name": "test",
            "command": "/bin/test"
        }
        "#;
        let configs = parse_config(content, ConfigFormat::Json).unwrap();
        assert_eq!(configs.len(), 1);
        assert_eq!(configs[0].name, "test");
        assert_eq!(configs[0].command, "/bin/test");
    }

    #[test]
    fn test_parse_json_multi() {
        let content = r#"
        {
            "processes": [
                {"name": "web", "command": "/bin/web"},
                {"name": "worker", "command": "/bin/worker"}
            ]
        }
        "#;
        let configs = parse_config(content, ConfigFormat::Json).unwrap();
        assert_eq!(configs.len(), 2);
        assert_eq!(configs[0].name, "web");
        assert_eq!(configs[1].name, "worker");
    }

    #[test]
    fn test_parse_yaml_single() {
        let content = r#"
name: test
command: /bin/test
"#;
        let configs = parse_config(content, ConfigFormat::Yaml).unwrap();
        assert_eq!(configs.len(), 1);
        assert_eq!(configs[0].name, "test");
    }

    #[test]
    fn test_parse_yaml_multi() {
        let content = r#"
processes:
  - name: web
    command: /bin/web
  - name: worker
    command: /bin/worker
"#;
        let configs = parse_config(content, ConfigFormat::Yaml).unwrap();
        assert_eq!(configs.len(), 2);
    }

    #[test]
    fn test_parse_toml_single() {
        let content = r#"
name = "test"
command = "/bin/test"
"#;
        let configs = parse_config(content, ConfigFormat::Toml).unwrap();
        assert_eq!(configs.len(), 1);
        assert_eq!(configs[0].name, "test");
    }

    #[test]
    fn test_parse_toml_multi() {
        let content = r#"
[[processes]]
name = "web"
command = "/bin/web"

[[processes]]
name = "worker"
command = "/bin/worker"
"#;
        let configs = parse_config(content, ConfigFormat::Toml).unwrap();
        assert_eq!(configs.len(), 2);
    }
}
