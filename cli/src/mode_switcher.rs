use crate::adaptive::classifier::{Bottleneck, BottleneckReport};
use crate::transfer_mode::TransferMode;

pub struct ModeSwitcher;

impl ModeSwitcher {
    pub fn determine_mode(
        report: &BottleneckReport,
        file_size: u64,
        zero_copy_failed: bool,
        current_mode: TransferMode,
    ) -> TransferMode {
        if zero_copy_failed {
            if current_mode == TransferMode::ZeroCopy {
                let event = serde_json::json!({
                    "mode_switch": "ZERO_COPY_DISABLED",
                    "reason": "syscall_failure"
                });
                println!("{}", serde_json::to_string(&event).unwrap());
            }
            return TransferMode::Buffered;
        }

        if file_size <= 4 * 1024 * 1024 {
            if current_mode == TransferMode::ZeroCopy {
                let event = serde_json::json!({
                    "mode_switch": "ZERO_COPY_DISABLED",
                    "reason": "small_file"
                });
                println!("{}", serde_json::to_string(&event).unwrap());
            }
            return TransferMode::Buffered;
        }

        match report.bottleneck {
            Bottleneck::KernelCopyOverhead => {
                if current_mode == TransferMode::Buffered {
                    let event = serde_json::json!({
                        "mode_switch": "ZERO_COPY_ENABLED",
                        "reason": "high_kernel_copy_overhead"
                    });
                    println!("{}", serde_json::to_string(&event).unwrap());
                }
                TransferMode::ZeroCopy
            }
            Bottleneck::Healthy => {
                // If it is Healthy, keep the current mode (no need to change)
                current_mode
            }
            _ => {
                if current_mode == TransferMode::ZeroCopy {
                    let event = serde_json::json!({
                        "mode_switch": "ZERO_COPY_DISABLED",
                        "reason": "bottleneck_changed"
                    });
                    println!("{}", serde_json::to_string(&event).unwrap());
                }
                TransferMode::Buffered
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::adaptive::classifier::{Bottleneck, BottleneckReport};
    use std::collections::HashMap;

    #[test]
    fn test_mode_switcher() {
        let report_kernel = BottleneckReport {
            bottleneck: Bottleneck::KernelCopyOverhead,
            confidence: 0.8,
            signals: vec![],
            scores: HashMap::new(),
            timestamp_ms: 0,
        };
        let report_cpu = BottleneckReport {
            bottleneck: Bottleneck::CpuBound,
            confidence: 0.8,
            signals: vec![],
            scores: HashMap::new(),
            timestamp_ms: 0,
        };

        // Large file, high copy overhead -> ZeroCopy
        assert_eq!(
            ModeSwitcher::determine_mode(&report_kernel, 10 * 1024 * 1024, false, TransferMode::Buffered),
            TransferMode::ZeroCopy
        );

        // Small file, high copy overhead -> Buffered
        assert_eq!(
            ModeSwitcher::determine_mode(&report_kernel, 2 * 1024 * 1024, false, TransferMode::Buffered),
            TransferMode::Buffered
        );

        // Large file, cpu bound -> Buffered
        assert_eq!(
            ModeSwitcher::determine_mode(&report_cpu, 10 * 1024 * 1024, false, TransferMode::ZeroCopy),
            TransferMode::Buffered
        );

        // Large file, high copy overhead, but failed -> Buffered
        assert_eq!(
            ModeSwitcher::determine_mode(&report_kernel, 10 * 1024 * 1024, true, TransferMode::ZeroCopy),
            TransferMode::Buffered
        );
    }
}
