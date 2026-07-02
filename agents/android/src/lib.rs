#![allow(non_snake_case)]
#![recursion_limit = "256"]


use std::sync::{Arc, Mutex};
use jni::objects::{JClass, JObject, JString, JValue};
use jni::JNIEnv;
use tokio::sync::mpsc;
use tracing::{error, info};
use lazy_static::lazy_static;

/// Custom panic hook: log to Android logcat but DON'T abort.
/// This prevents Rust panics from killing the Android process.
fn install_safe_panic_hook() {
    use std::sync::Once;
    static HOOK_INSTALLED: Once = Once::new();
    HOOK_INSTALLED.call_once(|| {
        let default_hook = std::panic::take_hook();
        std::panic::set_hook(Box::new(move |info| {
            // Log to logcat via direct NDK call — always works, no init needed
            #[cfg(target_os = "android")]
            unsafe {
                let msg = std::ffi::CString::new(format!("{}", info)).unwrap_or_default();
                let tag = std::ffi::CString::new("DOS-Rust-Panic").unwrap();
                android_log_sys::__android_log_write(
                    android_log_sys::LogPriority::ERROR as i32,
                    tag.as_ptr(),
                    msg.as_ptr(),
                );
            }
            // Don't abort — let the thread die gracefully.
            // The catch_unwind in our JNI entry points will handle recovery.
            default_hook(info);
        }));
    });
}

use dos_common::config::Config;
use dos_core::Platform;
use dos_crypto::NodeIdentity;
use dos_storage::{Database, SqliteSettingsRepository};
use dos_runtime::{Agent, AgentEvent};

pub mod providers;
pub mod file_server;

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
    install_safe_panic_hook();
    android_logger::init_once(
        android_logger::Config::default()
            .with_max_level(log::LevelFilter::Info)
            .with_tag("DOS-Rust"),
    );
    info!("DOS Android Agent native library loaded! Starting runtime...");

    let jvm = match env.get_java_vm() {
        Ok(jvm) => Arc::new(jvm),
        Err(e) => {
            error!("Failed to get JavaVM: {:?}", e);
            return;
        }
    };
    let callback_global = match env.new_global_ref(callback) {
        Ok(r) => r,
        Err(e) => {
            error!("Failed to create GlobalRef: {:?}", e);
            return;
        }
    };
    
    let path_str: String = match env.get_string(&config_path) {
        Ok(s) => s.into(),
        Err(e) => {
            error!("Invalid config path string: {:?}", e);
            return;
        }
    };
    let config_dir = std::path::Path::new(&path_str).parent().unwrap_or(std::path::Path::new(".")).to_string_lossy().to_string();
    let db_path = format!("{}/dos.db", config_dir);

    let (shutdown_tx, mut shutdown_rx) = mpsc::channel(1);
    *SHUTDOWN_TX.lock().unwrap() = Some(shutdown_tx);

    std::thread::spawn(move || {
        let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            if let Ok(rt) = tokio::runtime::Builder::new_multi_thread().enable_all().thread_name("dos-android-worker").build() {
                let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    rt.block_on(async move {
                        // Initialize Config
                        let config = Config::load(&path_str).unwrap_or_else(|_| Config {
                            node_name: "Android Phone".to_string(),
                            relay_url: "p2p".to_string(), // P2P mode by default
                            node_port: 7891,
                            db_path: db_path.clone(),
                            log_level: "info".to_string(),
                        });

                        let mut config = config;
                        // ALWAYS override db_path to the absolute app private directory
                        config.db_path = db_path.clone();

                        // Override relay_url to "p2p" if it was "discover" or empty
                        if config.relay_url.is_empty() || config.relay_url == "discover" {
                            config.relay_url = "p2p".to_string();
                        }

                        // Initialize Database and Identity
                        let db = match Database::open(&config.db_path).await {
                            Ok(db) => db,
                            Err(e) => {
                                error!("Failed to open database: {:?}", e);
                                return;
                            }
                        };
                        let settings = SqliteSettingsRepository::new(db);
                        let identity = load_or_generate_identity(&settings).await;

                        let (event_tx, mut event_rx) = mpsc::unbounded_channel::<AgentEvent>();

                        // Spawn Event dispatcher task to JVM
                        let callback_ref = callback_global.clone();
                        let callback_for_clipboard = callback_global.clone();
                        let jvm_for_clipboard = jvm.clone();
                        
                        let callback_for_notifications = callback_global.clone();
                        let jvm_for_notifications = jvm.clone();
                        
                        let clipboard = Arc::new(providers::clipboard::AndroidClipboardProvider::new(jvm_for_clipboard, callback_for_clipboard));
                        let notifications = Arc::new(providers::notifications::AndroidNotificationsProvider::new(jvm_for_notifications, callback_for_notifications));
                        let terminal = Arc::new(providers::terminal::AndroidTerminalProvider::new());
                        let file = Arc::new(providers::file::AndroidFileProvider::new());
                        
                        let mut registry = dos_task_manager::TaskRegistry::new();
                        registry.register("ping", |req| {
                            Ok(Box::new(dos_task_manager::PingTask::with_id(req.task_id.0)))
                        });
                        registry.register("clipboard", move |req| {
                            Ok(Box::new(dos_task_manager::ClipboardTask::new(&req, clipboard.clone())?))
                        });
                        registry.register("notifications", move |req| {
                            Ok(Box::new(dos_task_manager::NotificationsTask::new(&req, notifications.clone())?))
                        });
                        registry.register("terminal", move |req| {
                            Ok(Box::new(dos_task_manager::TerminalTask::new(&req, terminal.clone())?))
                        });
                        registry.register("file_transfer", move |req| {
                            Ok(Box::new(dos_task_manager::FileTask::new(&req, file.clone())?))
                        });

                        let jvm_for_events = jvm.clone();
                        tokio::spawn(async move {
                            while let Some(event) = event_rx.recv().await {
                                if let Ok(mut local_env) = jvm_for_events.attach_current_thread() {
                                    if let Ok(json) = serde_json::to_string(&event) {
                                        if let Ok(j_string) = local_env.new_string(json) {
                                            let _ = local_env.call_method(
                                                callback_ref.as_obj(),
                                                "onStateChanged",
                                                "(Ljava/lang/String;)V",
                                                &[JValue::from(&j_string)]
                                            );
                                        }
                                    }
                                }
                            }
                        });

                        let agent = Agent {
                            identity,
                            config,
                            platform: Platform::Android,
                            event_tx: Some(event_tx),
                            registry,
                        };

                        // Run in P2P server mode (accept incoming connections)
                        tokio::select! {
                            _ = agent.serve() => {
                                error!("Agent serve loop exited unexpectedly.");
                            }
                            _ = shutdown_rx.recv() => {
                                info!("Shutdown signal received, exiting Agent loop.");
                            }
                        }
                    });
                }));
            }
        }));
    });
}

#[no_mangle]
pub extern "system" fn Java_com_dos_agent_Core_stopAgent<'local>(
    _env: JNIEnv<'local>,
    _class: JClass<'local>,
) {
    info!("Stopping DOS Android Agent...");
    if let Ok(mut guard) = SHUTDOWN_TX.lock() {
        if let Some(tx) = guard.take() {
            let _ = tx.try_send(());
        }
    } else {
        error!("SHUTDOWN_TX mutex poisoned, cannot stop agent cleanly");
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

#[no_mangle]
pub extern "system" fn Java_com_dos_agent_NativeTransferEngine_startServer<'local>(
    mut env: JNIEnv<'local>,
    _class: JClass<'local>,
    port: jni::sys::jint,
    download_dir: JString<'local>,
) {
    install_safe_panic_hook();
    let path_str: String = match env.get_string(&download_dir) {
        Ok(s) => s.into(),
        Err(e) => {
            error!("Invalid download_dir string: {:?}", e);
            return;
        }
    };
    info!("Starting NativeTransferEngine on port {}", port);
    
    // Start it in a background tokio thread
    std::thread::spawn(move || {
        let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            if let Ok(rt) = tokio::runtime::Builder::new_multi_thread().enable_all().thread_name("dos-native-file").build() {
                let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    rt.block_on(async move {
                        file_server::start_server(port as u16, path_str).await;
                    });
                }));
            }
        }));
    });
}
