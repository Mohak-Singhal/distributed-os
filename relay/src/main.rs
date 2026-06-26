//! Relay server — accepts WebSocket connections and routes messages.

mod handler;
mod registry;

use std::sync::Arc;

use tokio::net::TcpListener;
use tracing::info;
use tracing_subscriber::{fmt, EnvFilter};

use dos_common::constants::DEFAULT_RELAY_PORT;

use crate::registry::Registry;

fn main() -> anyhow::Result<()> {
    fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .init();

    tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .thread_name("dos-relay-worker")
        .build()?
        .block_on(run())
}

async fn run() -> anyhow::Result<()> {
    let addr = format!("0.0.0.0:{DEFAULT_RELAY_PORT}");
    let listener = TcpListener::bind(&addr).await?;
    let registry = Registry::new();

    println!("╔══════════════════════════════════════════╗");
    println!("║            Relay Started                 ║");
    println!("╚══════════════════════════════════════════╝");
    info!(address = %addr, "relay listening");

    loop {
        match listener.accept().await {
            Ok((stream, peer_addr)) => {
                info!(peer = %peer_addr, "incoming connection");
                let reg = Arc::clone(&registry);
                tokio::spawn(handler::handle_connection(stream, peer_addr, reg));
            }
            Err(e) => {
                tracing::error!(error = %e, "accept failed");
            }
        }
    }
}
