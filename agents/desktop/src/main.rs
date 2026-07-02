//! Desktop Agent — macOS, Windows, Linux.



use tracing::info;
use tracing_subscriber::{fmt, EnvFilter};

use dos_common::config::Config;
use dos_core::Platform;
use dos_crypto::NodeIdentity;
use dos_storage::{Database, SettingsRepository, SqliteSettingsRepository};

use dos_runtime::Agent;

pub mod providers;

fn main() -> anyhow::Result<()> {
    fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .init();

    tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .thread_name("dos-desktop-worker")
        .build()?
        .block_on(run())
}

async fn run() -> anyhow::Result<()> {
    let config = Config::load("dos-config.toml")?;
    let db = Database::open(&config.db_path).await.expect("Failed to open database");
    let settings = SqliteSettingsRepository::new(db.clone());
    let identity = load_or_generate_identity(&settings).await;

    println!("╔══════════════════════════════════════════╗");
    println!("║        Desktop Agent Starting...         ║");
    println!("╚══════════════════════════════════════════╝");
    info!(
        node_id  = %identity.node_id,
        relay    = %config.relay_url,
        platform = %detect_platform(),
        "agent initialised"
    );

    let clipboard = std::sync::Arc::new(providers::clipboard::DesktopClipboardProvider::new());
    let notifications = std::sync::Arc::new(providers::notifications::DesktopNotificationsProvider::new());
    let terminal = std::sync::Arc::new(providers::terminal::DesktopTerminalProvider::new());
    let file = std::sync::Arc::new(providers::file::DesktopFileProvider::new());
    
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

    let agent = Agent {
        identity,
        config,
        platform: detect_platform(),
        event_tx: None,
        registry,
    };

    agent.run().await
}

async fn load_or_generate_identity(settings: &SqliteSettingsRepository) -> NodeIdentity {
    const IDENTITY_KEY: &str = "node_identity_signing_key";

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
        tracing::error!(error = %e, "failed to save identity to database");
    } else {
        info!(node_id = %identity.node_id, "identity generated and saved to database");
    }
    identity
}

fn detect_platform() -> Platform {
    #[cfg(target_os = "macos")]
    return Platform::Mac;
    #[cfg(target_os = "windows")]
    return Platform::Windows;
    #[cfg(target_os = "linux")]
    return Platform::Linux;
    #[allow(unreachable_code)]
    Platform::Unknown("unsupported".to_string())
}
