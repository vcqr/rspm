use chrono::{DateTime, Utc};
use std::time::Duration;

/// Format duration to human readable string
pub fn format_duration(duration: Duration) -> String {
    let total_secs = duration.as_secs();

    if total_secs < 60 {
        format!("{}s", total_secs)
    } else if total_secs < 3600 {
        let mins = total_secs / 60;
        let secs = total_secs % 60;
        format!("{}m {}s", mins, secs)
    } else if total_secs < 86400 {
        let hours = total_secs / 3600;
        let mins = (total_secs % 3600) / 60;
        format!("{}h {}m", hours, mins)
    } else {
        let days = total_secs / 86400;
        let hours = (total_secs % 86400) / 3600;
        format!("{}d {}h", days, hours)
    }
}

/// Format bytes to human readable string
pub fn format_bytes(bytes: u64) -> String {
    const KB: u64 = 1024;
    const MB: u64 = KB * 1024;
    const GB: u64 = MB * 1024;

    if bytes < KB {
        format!("{}B", bytes)
    } else if bytes < MB {
        format!("{:.1}KB", bytes as f64 / KB as f64)
    } else if bytes < GB {
        format!("{:.1}MB", bytes as f64 / MB as f64)
    } else {
        format!("{:.1}GB", bytes as f64 / GB as f64)
    }
}

/// Generate a unique process ID
pub fn generate_id() -> String {
    uuid::Uuid::new_v4().to_string()
}

/// Generate process ID from name and instance number
pub fn generate_process_id(name: &str, instance: u32) -> String {
    if instance == 0 {
        name.to_string()
    } else {
        format!("{}_{}", name, instance)
    }
}

/// Get current timestamp
pub fn now() -> DateTime<Utc> {
    Utc::now()
}

/// Convert timestamp to human readable string
pub fn format_timestamp(dt: &DateTime<Utc>) -> String {
    dt.format("%Y-%m-%d %H:%M:%S").to_string()
}

/// Check if a process has been running stable for the given threshold
pub fn is_stable(started_at: Option<DateTime<Utc>>, threshold_ms: u64) -> bool {
    if let Some(started) = started_at {
        let elapsed = (Utc::now() - started).num_milliseconds() as u64;
        elapsed >= threshold_ms
    } else {
        false
    }
}
