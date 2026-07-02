use async_trait::async_trait;
use std::process::Command;
use dos_task_manager::providers::terminal::TerminalProvider;

pub struct DesktopTerminalProvider;

impl DesktopTerminalProvider {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl TerminalProvider for DesktopTerminalProvider {
    async fn execute(&self, command: &str, args: &[String]) -> Result<String, String> {
        let output = Command::new(command)
            .args(args)
            .output()
            .map_err(|e| format!("Failed to execute command: {}", e))?;
            
        let mut result = String::from_utf8_lossy(&output.stdout).into_owned();
        if !output.stderr.is_empty() {
            if !result.is_empty() {
                result.push('\n');
            }
            result.push_str(&String::from_utf8_lossy(&output.stderr));
        }
        
        Ok(result)
    }
}
