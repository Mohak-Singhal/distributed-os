use arboard::Clipboard;
use async_trait::async_trait;
use dos_task_manager::providers::clipboard::ClipboardProvider;
use tokio::task;

pub struct DesktopClipboardProvider;

impl DesktopClipboardProvider {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl ClipboardProvider for DesktopClipboardProvider {
    async fn get_text(&self) -> Result<String, String> {
        // arboard must be called on a blocking thread because it interacts with native GUI APIs
        task::spawn_blocking(|| {
            let mut clipboard = Clipboard::new().map_err(|e| e.to_string())?;
            clipboard.get_text().map_err(|e| e.to_string())
        })
        .await
        .map_err(|e| format!("Join error: {}", e))?
    }

    async fn set_text(&self, text: &str) -> Result<(), String> {
        let text = text.to_string();
        task::spawn_blocking(move || {
            let mut clipboard = Clipboard::new().map_err(|e| e.to_string())?;
            clipboard.set_text(text).map_err(|e| e.to_string())
        })
        .await
        .map_err(|e| format!("Join error: {}", e))?
    }
}
