use async_trait::async_trait;
use std::fs;
use dos_task_manager::providers::file::FileProvider;

pub struct DesktopFileProvider;

impl DesktopFileProvider {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl FileProvider for DesktopFileProvider {
    async fn read(&self, path: &str) -> Result<Vec<u8>, String> {
        fs::read(path).map_err(|e| format!("Failed to read file: {}", e))
    }

    async fn write(&self, path: &str, content: &[u8]) -> Result<(), String> {
        fs::write(path, content).map_err(|e| format!("Failed to write file: {}", e))
    }
}
