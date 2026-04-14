---
name: process-scheduler-rspm
description: A powerful process and task scheduler manager that helps you manage applications and keep them running 24/7 with an intuitive CLI interface
---

## Overview

RSPM (Process Scheduler Manager) is a comprehensive CLI tool for managing processes, scheduling tasks, and serving static files. It provides robust process management with features like auto-restart, load balancing, log management, and cron-based scheduling.

> **Note:** Executables are located in the current user home directory `./bin/` directory:
>
> - `rspm` - CLI client
> - `rspmd` - Daemon server
>
> Ensure the `./bin/` directory is in your system PATH, or reference the full path when running commands.

## Core Capabilities

### 1. Process Management

Start, stop, restart, delete, and monitor processes with support for multiple instances and environment variables.

### 2. Daemon Management

Run RSPM as a background daemon for persistent process management across system reboots.

### 3. Task Scheduling

Create and manage scheduled tasks using cron expressions, intervals, or one-time execution.

### 4. Static File Serving

Quickly serve static files with configurable port, host, and directory options.

### 5. Configuration Management

Generate, validate, and load configurations from YAML, JSON, or TOML files.

## Quick Start

```bash
# Start the daemon
rspm start-daemon

# Start a process
rspm start my-app -- node server.js

# List all processes
rspm list

# View logs
rspm logs my-app --follow

# Stop the daemon
rspm stop-daemon
```

## Command Reference

### Process Commands

| Command                      | Description                  | Example                            |
| ---------------------------- | ---------------------------- | ---------------------------------- |
| `rspm start <name> -- <cmd>` | Start a new process          | `rspm start api -- node server.js` |
| `rspm stop <identifier>`     | Stop a process by name or ID | `rspm stop my-app`                 |
| `rspm restart <identifier>`  | Restart a process            | `rspm restart 1`                   |
| `rspm delete <identifier>`   | Delete a process             | `rspm delete my-app`               |
| `rspm delete --all`          | Delete all processes         | `rspm delete --all`                |
| `rspm list [name]`           | List all processes           | `rspm list api`                    |
| `rspm show <identifier>`     | Show process details         | `rspm show my-app`                 |
| `rspm logs <id> [opts]`      | View process logs            | `rspm logs my-app -f -n 100`       |
| `rspm scale <id> <n>`        | Scale process instances      | `rspm scale api 4`                 |

### Start Options

| Option         | Short | Description           |
| -------------- | ----- | --------------------- |
| `--name`       | `-n`  | Process name          |
| `--command`    | `-c`  | Command to execute    |
| `--instances`  | `-i`  | Number of instances   |
| `--cwd`        | `-w`  | Working directory     |
| `--config`     | `-f`  | Config file path      |
| `-e KEY=VALUE` |       | Environment variables |

### Log Options

| Option     | Short | Description              |
| ---------- | ----- | ------------------------ |
| `--follow` | `-f`  | Follow logs in real-time |
| `--lines`  | `-n`  | Number of lines to show  |
| `--stderr` |       | Show only stderr output  |

### Daemon Commands

| Command                      | Description                |
| ---------------------------- | -------------------------- |
| `rspm start-daemon [config]` | Start the daemon           |
| `rspm stop-daemon`           | Stop the daemon            |
| `rspm status`                | Show daemon status         |
| `rspm stop-all`              | Stop all managed processes |

### Configuration Commands

| Command               | Description                  | Example                           |
| --------------------- | ---------------------------- | --------------------------------- |
| `rspm init`           | Interactive config generator | `rspm init`                       |
| `rspm load <file>`    | Load config file             | `rspm load ecosystem.yaml`        |
| `rspm load -d <file>` | Dry-run config validation    | `rspm load config.yaml --dry-run` |

### Static Server

| Option        | Short | Description     | Default           |
| ------------- | ----- | --------------- | ----------------- |
| `--name`      | `-n`  | Server name     | auto-generated    |
| `--port`      | `-p`  | Port number     | 8080              |
| `--host`      | `-H`  | Listen address  | 127.0.0.1         |
| `--directory` | `-d`  | Serve directory | current directory |

```bash
# Basic usage
rspm serve

# Full configuration
rspm serve -n mysite -H 0.0.0.0 -p 3000 -d ./dist
```

### Schedule Commands

| Command                          | Description            | Example                                    |
| -------------------------------- | ---------------------- | ------------------------------------------ |
| `rspm schedule create -n <name>` | Create scheduled task  | See below                                  |
| `rspm schedule list`             | List all schedules     | `rspm schedule list`                       |
| `rspm schedule show <id>`        | Show schedule details  | `rspm schedule show my-task`               |
| `rspm schedule pause <id>`       | Pause a schedule       | `rspm schedule pause my-task`              |
| `rspm schedule resume <id>`      | Resume a schedule      | `rspm schedule resume my-task`             |
| `rspm schedule delete <id>`      | Delete a schedule      | `rspm schedule delete my-task`             |
| `rspm schedule delete --all`     | Delete all schedules   | `rspm schedule delete --all`               |
| `rspm schedule history <id>`     | View execution history | `rspm schedule history my-task --limit 50` |
| `rspm schedule logs <id>`        | View execution logs    | `rspm schedule logs my-task --lines 50`    |

#### Schedule Create Options

| Option          | Short | Description                                                |
| --------------- | ----- | ---------------------------------------------------------- |
| `--name`        | `-n`  | Schedule name (required)                                   |
| `--cron`        |       | Cron expression (6 fields: sec min hour day month weekday) |
| `--interval`    |       | Interval in seconds                                        |
| `--once`        |       | One-time execution (ISO 8601 format)                       |
| `--action`      | `-a`  | Action: start/stop/restart/execute                         |
| `--process`     | `-p`  | Target process name                                        |
| `--command`     | `-C`  | Custom command for execute action                          |
| `--args`        |       | Command arguments (comma-separated)                        |
| `--timezone`    | `-t`  | Timezone (e.g., Asia/Shanghai)                             |
| `--max-runs`    | `-m`  | Max executions (0=infinite)                                |
| `--description` | `-d`  | Task description                                           |
| `--disabled`    |       | Create in disabled state                                   |

#### Schedule Examples

```bash
# Daily restart at 2 AM
rspm schedule create daily-restart \
  --cron "0 0 2 * * *" \
  --process my-app \
  --action restart

# Health check every 5 minutes
rspm schedule create health-check \
  --cron "0 */5 * * * *" \
  --action execute \
  -- echo "Health check"

# One-time task
rspm schedule create cleanup \
  --once "2026-04-01T10:00:00Z" \
  --action execute \
  -- /bin/bash -c "rm -rf /tmp/*.log"

# Every 30 seconds
rspm schedule create ping \
  --interval 30 \
  --action execute \
  -- curl -s http://localhost:3000/health
```

## Common Workflows

### Workflow 1: Node.js Application

```bash
# Start application with environment variables
rspm start api -- node server.js \
  -e NODE_ENV=production \
  -e PORT=3000

# Monitor logs in real-time
rspm logs api --follow

# Restart after code changes
rspm restart api

# Scale to 4 instances
rspm scale api 4
```

### Workflow 2: Configuration-Driven Management

```bash
# Generate config interactively
rspm init

# Validate before loading
rspm load ecosystem.yaml --dry-run

# Load and start all processes
rspm load ecosystem.yaml

# View all processes
rspm list
```

### Workflow 3: Automated Maintenance

```bash
# Daily restart at 2 AM
rspm schedule create nightly-restart \
  --cron "0 0 2 * * *" \
  --process web-app \
  --action restart

# Weekly backup
rspm schedule create weekly-backup \
  --cron "0 0 3 * * 0" \
  --action execute \
  -- /bin/bash -c "tar -czf backup.tar.gz ./data"

# View schedule history
rspm schedule history nightly-restart --limit 20
```

### Workflow 4: Complete Lifecycle

```bash
# 1. Start daemon
rspm start-daemon

# 2. Check daemon status
rspm status

# 3. Start processes
rspm start web -- node server.js -e NODE_ENV=production
rspm start api -i 4 -- node api.js

# 4. Monitor
rspm list
rspm logs web -f

# 5. Create maintenance schedule
rspm schedule create daily-restart \
  --cron "0 0 2 * * *" \
  --process web \
  --action restart

# 6. Stop specific process
rspm stop web

# 7. Delete process
rspm delete api

# 8. Stop all processes
rspm stop-all

# 9. Stop daemon
rspm stop-daemon
```

## Cron Expression Format

| Field   | Range | Description                      |
| ------- | ----- | -------------------------------- |
| Second  | 0-59  | Seconds                          |
| Minute  | 0-59  | Minutes                          |
| Hour    | 0-23  | Hours                            |
| Day     | 1-31  | Day of month                     |
| Month   | 1-12  | Month                            |
| Weekday | 0-7   | Day of week (0 and 7 are Sunday) |

### Common Patterns

| Expression       | Description              |
| ---------------- | ------------------------ |
| `0 0 2 * * *`    | Daily at 2:00 AM         |
| `0 */5 * * * *`  | Every 5 minutes          |
| `0 0 0 * * 1`    | Every Monday at midnight |
| `0 30 9 * * *`   | Daily at 9:30 AM         |
| `0 0 12 * * 1-5` | Weekdays at noon         |

## Best Practices

1. **Use daemon mode** for persistent process management
2. **Validate configs** with `--dry-run` before loading
3. **Set environment variables** via `-e` flags or config files
4. **Monitor logs** with `--follow` for real-time debugging
5. **Use schedules** for automated maintenance tasks
6. **Set memory limits** to prevent resource exhaustion
7. **Configure auto-restart** for high availability
8. **Use multiple instances** for load balancing

## Tips

- Process identifiers can be names or IDs interchangeably
- Use `--force` with `stop` for immediate termination (SIGKILL)
- Scale up/down dynamically without restarting processes
- Combine `serve` with other commands for full-stack management
- Schedule tasks support timezones for global deployments
- Config files support YAML, JSON, and TOML formats
