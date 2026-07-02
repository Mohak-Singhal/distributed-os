use async_trait::async_trait;
use dos_task_manager::providers::notifications::NotificationsProvider;
use jni::objects::{GlobalRef, JValue};
use jni::JavaVM;
use tokio::task;

use std::sync::Arc;

pub struct AndroidNotificationsProvider {
    jvm: Arc<JavaVM>,
    callback: GlobalRef,
}

impl AndroidNotificationsProvider {
    pub fn new(jvm: Arc<JavaVM>, callback: GlobalRef) -> Self {
        Self { jvm, callback }
    }
}

#[async_trait]
impl NotificationsProvider for AndroidNotificationsProvider {
    async fn show(&self, title: &str, body: &str) -> Result<(), String> {
        let jvm = self.jvm.clone();
        let callback = self.callback.clone();
        let title = title.to_string();
        let body = body.to_string();

        task::spawn_blocking(move || {
            let mut env = jvm.attach_current_thread().map_err(|e| e.to_string())?;
            
            let j_title = env.new_string(title).map_err(|e| e.to_string())?;
            let j_body = env.new_string(body).map_err(|e| e.to_string())?;
            
            env.call_method(
                callback.as_obj(),
                "showNotification",
                "(Ljava/lang/String;Ljava/lang/String;)V",
                &[JValue::from(&j_title), JValue::from(&j_body)]
            ).map_err(|e| e.to_string())?;
            
            Ok(())
        })
        .await
        .map_err(|e| format!("Join error: {}", e))?
    }
}
