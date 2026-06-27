#![cfg(target_os = "android")]
#![allow(non_snake_case)]

use std::sync::{Arc, Mutex};
use jni::objects::{JClass, JObject, JString, JValue};
use jni::{JNIEnv, JavaVM};
use tokio::sync::mpsc;
use tracing::{error, info};
use lazy_static::lazy_static;

use dos_common::config::Config;
use dos_core::Platform;
use dos_crypto::NodeIdentity;
use dos_storage::{Database, SqliteSettingsRepository};
use dos_runtime::{Agent, AgentEvent};

lazy_static! {
    static ref SHUTDOWN_TX: Mutex<Option<mpsc::Sender<()>>> = Mutex::new(None);
}

#[no_mangle]
pub extern "system" fn Java_com_dos_agent_Core_startAgent<'local>(
    mut env: JNIEnv<'local>,
    _class: JClass<'local>,
    config_path: JString<'local>,
    callback: JObject<'local>,
) {
    tracing_subscriber::fmt().with_env_filter("info").try_init().ok();
    info!("DOS Android Agent native library loaded! Starting runtime...");

    let jvm = env.get_java_vm().expect("Failed to get JavaVM");
    let callback_global = env.new_global_ref(callback).expect("Failed to create GlobalRef");
    
    let path_str: String = env.get_string(&config_path).expect("Invalid string").into();
    let config_dir = std::path::Path::new(&path_str).parent().unwrap_or(std::path::Path::new(".")).to_string_lossy().to_string();
    let db_path = format!("{}/dos.db", config_dir);

    let (shutdown_tx, mut shutdown_rx) = mpsc::channel(1);
    *SHUTDOWN_TX.lock().unwrap() = Some(shutdown_tx);

    std::thread::spawn(move || {
        if let Ok(rt) = tokio::runtime::Builder::new_multi_thread().enable_all().thread_name("dos-android-worker").build() {
            rt.block_on(async move {
                // Initialize Config
                let config = Config::load(&path_str).unwrap_or_else(|_| Config {
                    node_name: "Android Phone".to_string(),
                    relay_url: "ws://127.0.0.1:7890".to_string(), // Fallback
                    node_port: 7891,
                    db_path: db_path.clone(),
                    log_level: "info".to_string(),
                });

                // Initialize Database and Identity
                let db = Database::open(&config.db_path).await.expect("Failed to open database");
                let settings = SqliteSettingsRepository::new(db);
                let identity = load_or_generate_identity(&settings).await;

                let (event_tx, mut event_rx) = mpsc::unbounded_channel::<AgentEvent>();

                // Spawn Event dispatcher task to JVM
                let jvm_clone = jvm.clone();
                let callback_ref = callback_global.clone();
                tokio::spawn(async move {
                    while let Some(event) = event_rx.recv().await {
                        if let Ok(mut local_env) = jvm_clone.attach_current_thread() {
                            if let Ok(json) = serde_json::to_string(&event) {
                                let j_string = local_env.new_string(json).unwrap();
                                let _ = local_env.call_method(
                                    callback_ref.as_obj(),
                                    "onStateChanged",
                                    "(Ljava/lang/String;)V",
                                    &[JValue::from(&j_string)]
                                );
                            }
                        }
                    }
                });

                let agent = Agent {
                    identity,
                    config,
                    platform: Platform::Unknown("android".to_string()),
                    event_tx: Some(event_tx),
                };

                // Run agent until shutdown
                tokio::select! {
                    _ = agent.run() => {
                        error!("Agent run loop exited unexpectedly.");
                    }
                    _ = shutdown_rx.recv() => {
                        info!("Shutdown signal received, exiting Agent loop.");
                    }
                }
            });
        }
    });
}

#[no_mangle]
pub extern "system" fn Java_com_dos_agent_Core_stopAgent<'local>(
    _env: JNIEnv<'local>,
    _class: JClass<'local>,
) {
    info!("Stopping DOS Android Agent...");
    if let Some(tx) = SHUTDOWN_TX.lock().unwrap().take() {
        let _ = tx.try_send(());
    }
}

async fn load_or_generate_identity(settings: &SqliteSettingsRepository) -> NodeIdentity {
    const IDENTITY_KEY: &str = "node_identity_signing_key";
    use dos_storage::SettingsRepository;

    if let Ok(Some(hex_str)) = settings.get(IDENTITY_KEY).await {
        if let Ok(bytes) = hex::decode(&hex_str) {
            if bytes.len() == 32 {
                let mut key = [0u8; 32];
                key.copy_from_slice(&bytes);
                if let Ok(identity) = NodeIdentity::from_bytes(&key) {
                    info!(node_id = %identity.node_id, "identity loaded from database");
                    return identity;
                }
            }
        }
    }

    let identity = NodeIdentity::generate();
    let hex_key = hex::encode(identity.to_signing_key_bytes());
    if let Err(e) = settings.set(IDENTITY_KEY, &hex_key).await {
        error!(error = %e, "failed to save identity to database");
    } else {
        info!(node_id = %identity.node_id, "identity generated and saved to database");
    }
    identity
}
