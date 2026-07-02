//! Clipboard provider abstraction.

use async_trait::async_trait;

/// Abstract interface for clipboard access across platforms.
#[async_trait]
pub trait ClipboardProvider: Send + Sync + 'static {
    /// Retrieve text from the clipboard.
    async fn get_text(&self) -> Result<String, String>;

    /// Set the clipboard text.
    async fn set_text(&self, text: &str) -> Result<(), String>;
}
