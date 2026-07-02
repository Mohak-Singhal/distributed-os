use async_trait::async_trait;
use std::process::Command;
use dos_task_manager::providers::notifications::NotificationsProvider;

pub struct DesktopNotificationsProvider;

impl DesktopNotificationsProvider {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl NotificationsProvider for DesktopNotificationsProvider {
    async fn show(&self, title: &str, body: &str) -> Result<(), String> {
        // Escape quotes to prevent AppleScript injection
        let safe_title = title.replace('"', "'");
        let safe_body = body.replace('"', "'");

        // Try banner notification first (requires Notification Center permission)
        let banner_script = format!(
            r#"display notification "{}" with title "{}" sound name "default""#,
            safe_body, safe_title
        );
        let banner_result = Command::new("osascript")
            .arg("-e")
            .arg(&banner_script)
            .output();

        if let Ok(output) = &banner_result {
            if output.status.success() {
                return Ok(());
            }
        }

        // Fallback: use a dialog box — always visible, no permission needed
        let dialog_script = format!(
            r#"display dialog "{}" with title "{}" buttons {{"OK"}} default button "OK" with icon note"#,
            safe_body, safe_title
        );
        let dialog_output = Command::new("osascript")
            .arg("-e")
            .arg(&dialog_script)
            .output()
            .map_err(|e| format!("Failed to execute osascript: {}", e))?;

        if !dialog_output.status.success() {
            let err = String::from_utf8_lossy(&dialog_output.stderr);
            return Err(format!("osascript failed: {}", err));
        }

        Ok(())
    }
}
