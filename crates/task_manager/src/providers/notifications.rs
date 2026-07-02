use async_trait::async_trait;

/// Provides platform-specific notification delivery.
#[async_trait]
pub trait NotificationsProvider: Send + Sync {
    /// Show a notification with the given title and body.
    async fn show(&self, title: &str, body: &str) -> Result<(), String>;
}
