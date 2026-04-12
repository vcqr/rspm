use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::fmt;

/// 定时任务调度类型
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum ScheduleType {
    /// Cron 表达式（6字段：秒 分 时 天 月 周）
    /// 例如："0 0 2 * * *" 每天凌晨2点执行
    /// 例如："0 */5 * * * *" 每5分钟执行
    /// 例如："0 0 0 * * 1" 每周一执行
    Cron(String),
    /// 间隔执行（秒）
    Interval(u64),
    /// 一次性执行（指定时间戳）
    Once(DateTime<Utc>),
}

impl fmt::Display for ScheduleType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ScheduleType::Cron(expr) => write!(f, "cron({})", expr),
            ScheduleType::Interval(secs) => write!(f, "interval({}s)", secs),
            ScheduleType::Once(dt) => write!(f, "once({})", dt.to_rfc3339()),
        }
    }
}

/// 定时任务操作类型
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum ScheduleAction {
    /// 启动进程
    Start,
    /// 停止进程
    Stop,
    /// 重启进程
    Restart,
    /// 执行自定义命令
    Execute { command: String, args: Vec<String> },
}

impl fmt::Display for ScheduleAction {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ScheduleAction::Start => write!(f, "start"),
            ScheduleAction::Stop => write!(f, "stop"),
            ScheduleAction::Restart => write!(f, "restart"),
            ScheduleAction::Execute { command, args } => {
                write!(f, "execute({} {})", command, args.join(" "))
            }
        }
    }
}

/// 定时任务状态
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum ScheduleStatus {
    /// 启用状态
    Active,
    /// 暂停状态
    Paused,
    /// 已完成（一次性任务）
    Completed,
    /// 错误状态
    Error(String),
}

impl fmt::Display for ScheduleStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ScheduleStatus::Active => write!(f, "active"),
            ScheduleStatus::Paused => write!(f, "paused"),
            ScheduleStatus::Completed => write!(f, "completed"),
            ScheduleStatus::Error(msg) => write!(f, "error({})", msg),
        }
    }
}

/// 定时任务配置
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScheduleConfig {
    /// 定时任务唯一ID（可选，自动生成）
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    /// 定时任务名称
    pub name: String,
    /// 关联的进程名称（如果操作类型是 start/stop/restart）
    #[serde(skip_serializing_if = "Option::is_none")]
    pub process_name: Option<String>,
    /// 调度类型
    #[serde(flatten)]
    pub schedule: ScheduleType,
    /// 操作类型
    pub action: ScheduleAction,
    /// 是否启用
    #[serde(default = "default_enabled")]
    pub enabled: bool,
    /// 时区（默认 UTC）
    #[serde(default = "default_timezone")]
    pub timezone: String,
    /// 最大执行次数（0表示无限）
    #[serde(default)]
    pub max_runs: u32,
    /// 任务描述
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}

fn default_enabled() -> bool {
    true
}

fn default_timezone() -> String {
    "UTC".to_string()
}

impl ScheduleConfig {
    /// 创建新的定时任务配置
    pub fn new(name: impl Into<String>, schedule: ScheduleType, action: ScheduleAction) -> Self {
        Self {
            id: None,
            name: name.into(),
            process_name: None,
            schedule,
            action,
            enabled: true,
            timezone: default_timezone(),
            max_runs: 0,
            description: None,
        }
    }

    /// 设置关联的进程名称
    pub fn process_name(mut self, name: impl Into<String>) -> Self {
        self.process_name = Some(name.into());
        self
    }

    /// 设置时区
    pub fn timezone(mut self, tz: impl Into<String>) -> Self {
        self.timezone = tz.into();
        self
    }

    /// 设置最大执行次数
    pub fn max_runs(mut self, runs: u32) -> Self {
        self.max_runs = runs;
        self
    }

    /// 设置描述
    pub fn description(mut self, desc: impl Into<String>) -> Self {
        self.description = Some(desc.into());
        self
    }

    /// 验证 cron 表达式是否有效（6字段格式）
    pub fn validate_cron(expr: &str) -> Result<(), String> {
        let parts: Vec<&str> = expr.split_whitespace().collect();
        if parts.len() != 6 {
            return Err(format!(
                "Cron expression must have 6 fields (second minute hour day month weekday), got {} fields: {}",
                parts.len(),
                expr
            ));
        }

        // 使用 croner 3.0 验证 6 字段表达式
        // croner 默认支持 6 字段：秒 分 时 天 月 周
        match expr.parse::<croner::Cron>() {
            Ok(cron) => {
                // 尝试获取下一次执行时间，如果成功则表达式有效
                match cron.find_next_occurrence(&Utc::now(), false) {
                    Ok(_) => Ok(()),
                    Err(e) => Err(format!("Invalid cron expression '{}': {}", expr, e)),
                }
            }
            Err(e) => Err(format!("Invalid cron expression '{}': {}", expr, e)),
        }
    }

    /// 获取下次执行时间
    pub fn next_run(&self, after: DateTime<Utc>) -> Option<DateTime<Utc>> {
        match &self.schedule {
            ScheduleType::Cron(expr) => {
                // croner 3.0 使用 find_next_occurrence 获取下一次执行时间
                match expr.parse::<croner::Cron>() {
                    Ok(cron) => cron.find_next_occurrence(&after, false).ok(),
                    Err(_) => None,
                }
            }
            ScheduleType::Interval(secs) => Some(after + chrono::Duration::seconds(*secs as i64)),
            ScheduleType::Once(dt) => {
                if *dt > after {
                    Some(*dt)
                } else {
                    None
                }
            }
        }
    }
}

/// 定时任务执行记录
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScheduleExecution {
    /// 执行ID
    pub id: String,
    /// 定时任务ID
    pub schedule_id: String,
    /// 执行开始时间
    pub started_at: DateTime<Utc>,
    /// 执行结束时间
    pub ended_at: Option<DateTime<Utc>>,
    /// 执行状态
    pub status: ExecutionStatus,
    /// 执行输出/结果
    pub output: Option<String>,
    /// 错误信息
    pub error: Option<String>,
}

/// 执行状态
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum ExecutionStatus {
    Running,
    Success,
    Failed,
    Timeout,
}

/// 定时任务信息（包含运行状态）
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScheduleInfo {
    /// 定时任务ID
    pub id: String,
    /// 配置信息
    pub config: ScheduleConfig,
    /// 当前状态
    pub status: ScheduleStatus,
    /// 创建时间
    pub created_at: DateTime<Utc>,
    /// 最后更新时间
    pub updated_at: DateTime<Utc>,
    /// 上次执行时间
    pub last_run: Option<DateTime<Utc>>,
    /// 下次执行时间
    pub next_run: Option<DateTime<Utc>>,
    /// 执行次数
    pub run_count: u32,
    /// 成功次数
    pub success_count: u32,
    /// 失败次数
    pub fail_count: u32,
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Timelike;

    #[test]
    fn test_cron_validation() {
        // 有效的 6 字段 cron
        assert!(ScheduleConfig::validate_cron("0 0 2 * * *").is_ok());
        assert!(ScheduleConfig::validate_cron("0 */5 * * * *").is_ok());
        assert!(ScheduleConfig::validate_cron("0 0 0 * * 1").is_ok());

        // 无效的 cron（5字段，缺少秒）
        assert!(ScheduleConfig::validate_cron("0 2 * * *").is_err());

        // 无效的 cron（7字段）
        assert!(ScheduleConfig::validate_cron("0 0 0 2 * * *").is_err());
    }

    #[test]
    fn test_schedule_type_display() {
        assert_eq!(
            ScheduleType::Cron("0 0 2 * * *".to_string()).to_string(),
            "cron(0 0 2 * * *)"
        );
        assert_eq!(ScheduleType::Interval(300).to_string(), "interval(300s)");
    }

    #[test]
    fn test_next_run_cron() {
        let config = ScheduleConfig::new(
            "test",
            ScheduleType::Cron("0 0 2 * * *".to_string()),
            ScheduleAction::Start,
        );

        let now = Utc::now();
        let next = config.next_run(now);
        assert!(next.is_some());
        // 下次执行应该是明天凌晨2点
        let next = next.unwrap();
        assert_eq!(next.hour(), 2);
        assert_eq!(next.minute(), 0);
        assert_eq!(next.second(), 0);
    }
}
