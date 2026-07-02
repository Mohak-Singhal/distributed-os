use chrono::Utc;
use dos_core::{NodeStatus, Platform};
use dos_heartbeat::{HeartbeatError, HeartbeatSampler};
use dos_protocol::message::HeartbeatPayload;
use sysinfo::System;

/// macOS implementation of [`HeartbeatSampler`].
///
/// Collects real CPU, memory, battery, and thermal metrics using `sysinfo`
/// and macOS command-line tools (`pmset`, `sysctl`).
pub struct MacSampler {
    version: String,
    sys: std::sync::Mutex<System>,
}

impl MacSampler {
    /// Create a sampler for the given agent version string.
    pub fn new(version: impl Into<String>) -> Self {
        Self {
            version: version.into(),
            sys: std::sync::Mutex::new(System::new_all()),
        }
    }

    fn get_thermal_state() -> String {
        let output = std::process::Command::new("pmset")
            .args(["-g", "therm"])
            .output()
            .ok()
            .and_then(|o| String::from_utf8(o.stdout).ok())
            .unwrap_or_default();
        if output.contains("CPU_Scheduler_Limit") {
            if let Some(line) = output.lines().find(|l| l.contains("CPU_Scheduler_Limit")) {
                let val = line.split('=').nth(1).unwrap_or("0").trim();
                let limit: f64 = val.parse().unwrap_or(0.0);
                if limit < 50.0 {
                    return "Critical".to_string();
                } else if limit < 70.0 {
                    return "Fair".to_string();
                }
            }
        }
        "Nominal".to_string()
    }

    fn get_cpu_temp() -> Option<f64> {
        let output = std::process::Command::new("sysctl")
            .args(["-n", "machdep.xcpm.cpu_thermal_level"])
            .output()
            .ok()
            .and_then(|o| String::from_utf8(o.stdout).ok())?;
        let level: f64 = output.trim().parse().ok()?;
        Some(level * 10.0 + 30.0)
    }

    fn get_fan_rpm() -> Option<f64> {
        let output = std::process::Command::new("istats")
            .args(["fan"])
            .output()
            .ok()
            .and_then(|o| String::from_utf8(o.stdout).ok())?;
        if let Some(line) = output.lines().find(|l| l.contains("Fan speed")) {
            let rpm_str = line.split(':').nth(1)?.trim().split_whitespace().next()?;
            return rpm_str.parse().ok();
        }
        None
    }

    fn get_battery_info() -> (Option<f64>, Option<f64>) {
        let output = std::process::Command::new("pmset")
            .args(["-g", "batt"])
            .output()
            .ok()
            .and_then(|o| String::from_utf8(o.stdout).ok())
            .unwrap_or_default();

        let pct = if let Some(line) = output.lines().find(|l| l.contains('%')) {
            if let Some(pct_str) = line.split('\t').nth(1) {
                pct_str.trim().trim_end_matches('%').parse().ok()
            } else if let Some(pct_str) = line.split(';').nth(1) {
                pct_str.trim().trim_end_matches('%').parse().ok()
            } else {
                None
            }
        } else {
            None
        };

        let temp_pct = output.lines()
            .find(|l| l.contains("temperature") || l.contains("Temp"))
            .and_then(|l| {
                l.split_whitespace()
                    .find_map(|w| w.trim_end_matches('C').trim_end_matches('°').parse::<f64>().ok())
            });

        (pct, temp_pct)
    }
}

#[async_trait::async_trait]
impl HeartbeatSampler for MacSampler {
    async fn sample(&self) -> Result<HeartbeatPayload, HeartbeatError> {
        let mut sys = self.sys.lock().map_err(|e| {
            HeartbeatError::SamplingFailed(e.to_string())
        })?;
        sys.refresh_cpu_usage();
        sys.refresh_memory();

        let cpu_usage = sys.global_cpu_usage() as f32;
        let total_mem = sys.total_memory();
        let used_mem = sys.used_memory();
        let memory_usage = if total_mem > 0 {
            (used_mem as f32 / total_mem as f32) * 100.0
        } else {
            0.0
        };

        let (battery_pct, _battery_temp) = Self::get_battery_info();
        let _thermal_state = Self::get_thermal_state();
        let _cpu_temp = Self::get_cpu_temp();
        let _fan_rpm = Self::get_fan_rpm();

        Ok(HeartbeatPayload {
            cpu_usage,
            memory_usage,
            battery_level: battery_pct.map(|p| p as u8),
            platform: Platform::Mac,
            version: self.version.clone(),
            status: NodeStatus::Online,
            capabilities: vec![],
            timestamp: Utc::now(),
        })
    }
}
