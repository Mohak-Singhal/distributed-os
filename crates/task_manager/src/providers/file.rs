use async_trait::async_trait;

/// Provides platform-specific file reading and writing operations.
#[async_trait]
pub trait FileProvider: Send + Sync {
    /// Read a file's contents from the given path.
    async fn read(&self, path: &str) -> Result<Vec<u8>, String>;
    
    /// Write contents to the given file path.
    async fn write(&self, path: &str, content: &[u8]) -> Result<(), String>;
}
