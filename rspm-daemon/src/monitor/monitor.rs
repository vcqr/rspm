use rspm_common::ProcessStats;
use std::collections::HashMap;
use std::sync::RwLock;
use sysinfo::{Pid, System};

/// System monitor for collecting process statistics
pub struct Monitor {
    system: RwLock<System>,
}

impl Default for Monitor {
    fn default() -> Self {
        Self::new()
    }
}

impl Monitor {
    pub fn new() -> Self {
        let mut system = System::new_all();
        system.refresh_all();

        Self {
            system: RwLock::new(system),
        }
    }

    /// Refresh system information
    pub fn refresh(&self) {
        let mut system = self.system.write().unwrap();
        system.refresh_all();
    }

    /// Get process statistics by PID
    pub fn get_process_stats(&self, pid: i32) -> Option<ProcessStats> {
        let mut system = self.system.write().unwrap();
        system.refresh_processes_specifics(
            sysinfo::ProcessesToUpdate::Some(&[Pid::from(pid as usize)]),
            true,
            sysinfo::ProcessRefreshKind::everything(),
        );
        let process = system.process(Pid::from(pid as usize))?;

        Some(ProcessStats {
            cpu_percent: process.cpu_usage() as f64,
            memory_bytes: process.memory(),
            fd_count: 0, // sysinfo doesn't provide this on all platforms
        })
    }

    /// Get all processes
    pub fn get_all_processes(&self) -> HashMap<i32, ProcessStats> {
        let system = self.system.read().unwrap();
        let mut result = HashMap::new();

        for (pid, process) in system.processes() {
            result.insert(
                pid.as_u32() as i32,
                ProcessStats {
                    cpu_percent: process.cpu_usage() as f64,
                    memory_bytes: process.memory(),
                    fd_count: 0,
                },
            );
        }

        result
    }

    /// Get total system memory
    pub fn get_total_memory(&self) -> u64 {
        let system = self.system.read().unwrap();
        system.total_memory()
    }

    /// Get available system memory
    pub fn get_available_memory(&self) -> u64 {
        let system = self.system.read().unwrap();
        system.available_memory()
    }

    /// Get total CPU usage
    pub fn get_cpu_usage(&self) -> f32 {
        let system = self.system.read().unwrap();
        system.global_cpu_usage()
    }
}
