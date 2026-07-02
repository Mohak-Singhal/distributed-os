use async_trait::async_trait;
use dos_task_manager::providers::clipboard::ClipboardProvider;
use jni::objects::{GlobalRef, JString, JValue};
use jni::JavaVM;
use tokio::task;

use std::sync::Arc;

pub struct AndroidClipboardProvider {
    jvm: Arc<JavaVM>,
    callback: GlobalRef,
}

impl AndroidClipboardProvider {
    pub fn new(jvm: Arc<JavaVM>, callback: GlobalRef) -> Self {
        Self { jvm, callback }
    }
}

#[async_trait]
impl ClipboardProvider for AndroidClipboardProvider {
    async fn get_text(&self) -> Result<String, String> {
        let jvm = self.jvm.clone();
        let callback = self.callback.clone();

        task::spawn_blocking(move || {
            let mut env = jvm.attach_current_thread().map_err(|e| e.to_string())?;
            
            let result = env.call_method(
                callback.as_obj(),
                "getClipboard",
                "()Ljava/lang/String;",
                &[]
            ).map_err(|e| e.to_string())?;
            
            let j_str = result.l().map_err(|e| e.to_string())?;
            let string: String = env.get_string(&JString::from(j_str))
                .map_err(|e| e.to_string())?
                .into();
                
            Ok(string)
        })
        .await
        .map_err(|e| format!("Join error: {}", e))?
    }

    async fn set_text(&self, text: &str) -> Result<(), String> {
        let jvm = self.jvm.clone();
        let callback = self.callback.clone();
        let text = text.to_string();

        task::spawn_blocking(move || {
            let mut env = jvm.attach_current_thread().map_err(|e| e.to_string())?;
            
            let j_string = env.new_string(text).map_err(|e| e.to_string())?;
            
            env.call_method(
                callback.as_obj(),
                "setClipboard",
                "(Ljava/lang/String;)V",
                &[JValue::from(&j_string)]
            ).map_err(|e| e.to_string())?;
            
            Ok(())
        })
        .await
        .map_err(|e| format!("Join error: {}", e))?
    }
}
