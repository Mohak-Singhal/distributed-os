use async_trait::async_trait;

/// Provides platform-specific terminal execution.
#[async_trait]
pub trait TerminalProvider: Send + Sync {
    /// Execute a command and return its standard output (and standard error if combined).
    async fn execute(&self, command: &str, args: &[String]) -> Result<String, String>;
}
