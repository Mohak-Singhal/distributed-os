use std::collections::VecDeque;

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct TransferRecord {
    pub interface_label: String,
    pub rtt_ms: f64,
    pub file_size: u64,
    pub file_count: u32,
    pub strategy: String,
    pub streams: u32,
    pub buffer_kb: u32,
    pub achieved_mbps: f64,
    pub duration_secs: f64,
    pub success: bool,
}

pub struct LearningDb {
    records: VecDeque<TransferRecord>,
    max_records: usize,
}

impl LearningDb {
    pub fn new(max_records: usize) -> Self {
        let mut records = VecDeque::with_capacity(max_records);
        // Load from disk
        let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".to_string());
        let path = format!("{}/.pdos/transfer_learning.json", home);
        if let Ok(data) = std::fs::read_to_string(&path) {
            if let Ok(loaded) = serde_json::from_str::<Vec<TransferRecord>>(&data) {
                for r in loaded.into_iter().take(max_records) {
                    records.push_back(r);
                }
            }
        }
        Self { records, max_records }
    }

    pub fn record(&mut self, record: TransferRecord) {
        if self.records.len() >= self.max_records {
            self.records.pop_front();
        }
        self.records.push_back(record);
        self.save();
    }

    pub fn best_strategy(&self, interface_label: &str, file_size: u64, file_count: u32) -> Option<&str> {
        let candidates: Vec<&TransferRecord> = self.records.iter()
            .filter(|r| r.interface_label == interface_label && r.file_count == file_count && r.success)
            .collect();

        // Find the closest file size match
        candidates.into_iter()
            .min_by_key(|r| (r.file_size as i64 - file_size as i64).unsigned_abs())
            .map(|r| r.strategy.as_str())
    }

    fn save(&self) {
        let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".to_string());
        let dir = format!("{}/.pdos", home);
        std::fs::create_dir_all(&dir).ok();
        let path = format!("{}/transfer_learning.json", dir);
        if let Ok(data) = serde_json::to_string(&self.records) {
            let _ = std::fs::write(&path, data);
        }
    }
}
