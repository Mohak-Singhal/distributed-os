//! mDNS / Bonjour discovery backend.
//!
//! Discovers peers advertising `_xync._tcp` on the local network.
//! Implements the [`Discoverer`] trait from [`crate::method`].

use std::collections::HashMap;
use std::sync::Arc;

use mdns_sd::{ServiceDaemon, ServiceEvent};
use tokio::sync::Mutex;

use dos_core::{Capability, Platform};
use dos_protocol::ids::NodeId;

use crate::{DiscoveryError, DiscoveredNode, DiscoveryMethod, Discoverer};

/// mDNS service type advertised by all xync peers.
pub const SERVICE_TYPE: &str = "_xync._tcp.local.";

/// A peer discovered via mDNS with raw properties extracted from TXT records.
#[derive(Debug, Clone)]
pub struct MdnsPeer {
    /// Full service name.
    pub fullname: String,
    /// IPv4 address of the peer.
    pub host: String,
    /// Port the peer is listening on.
    pub port: u16,
    /// Human-readable node name.
    pub node_name: String,
    /// Platform string from TXT record.
    pub platform: String,
    /// Version string from TXT record.
    pub version: String,
}

/// mDNS-based discovery backend.
///
/// Spawns a background task that browses `_xync._tcp` and calls the
/// `on_discovered` callback for each newly resolved service.
pub struct MdnsDiscoverer {
    daemon: Arc<Mutex<Option<ServiceDaemon>>>,
}

impl MdnsDiscoverer {
    /// Create a new mDNS discoverer (not yet started).
    pub fn new() -> Self {
        Self {
            daemon: Arc::new(Mutex::new(None)),
        }
    }

    /// Advertise this node on mDNS so others can discover us.
    /// Returns a handle — drop it or call `stop_advertising` to unregister.
    pub fn advertise(
        port: u16,
        node_name: &str,
        platform: &str,
        version: &str,
    ) -> anyhow::Result<ServiceDaemon> {
        let mdns = ServiceDaemon::new()?;
        let hostname = format!(
            "pdos-{}-{}",
            platform,
            uuid::Uuid::new_v4().to_string().chars().take(4).collect::<String>()
        );

        let mut properties = HashMap::new();
        properties.insert("platform".to_string(), platform.to_string());
        properties.insert("node_name".to_string(), node_name.to_string());
        properties.insert("version".to_string(), version.to_string());

        let service_info = mdns_sd::ServiceInfo::new(
            SERVICE_TYPE,
            node_name,
            &format!("{}.local.", hostname),
            "0.0.0.0",
            port,
            properties,
        )?;

        mdns.register(service_info)?;
        tracing::info!(name = %node_name, port = %port, "advertised on _xync._tcp");
        Ok(mdns)
    }
}

#[async_trait::async_trait]
impl Discoverer for MdnsDiscoverer {
    async fn start(
        &self,
        on_discovered: Box<dyn Fn(DiscoveredNode) + Send + Sync>,
    ) -> Result<(), DiscoveryError> {
        let mut guard = self.daemon.lock().await;
        if guard.is_some() {
            return Err(DiscoveryError::AlreadyRunning);
        }

        let daemon = ServiceDaemon::new()
            .map_err(|e| DiscoveryError::MdnsInit(e.to_string()))?;
        let receiver = daemon.browse(SERVICE_TYPE)
            .map_err(|e| DiscoveryError::MdnsInit(e.to_string()))?;

        *guard = Some(daemon);
        drop(guard);

        let on_discovered = Arc::new(on_discovered);
        tokio::spawn(async move {
            loop {
                match receiver.recv_async().await {
                    Ok(ServiceEvent::ServiceResolved(svc)) => {
                        let ip = svc.get_addresses_v4().iter()
                            .next()
                            .map(|a| a.to_string())
                            .unwrap_or_default();

                        let peer = MdnsPeer {
                            fullname: svc.get_fullname().to_string(),
                            host: ip,
                            port: svc.get_port(),
                            node_name: svc.get_property_val_str("node_name")
                                .unwrap_or("unknown").to_string(),
                            platform: svc.get_property_val_str("platform")
                                .unwrap_or("unknown").to_string(),
                            version: svc.get_property_val_str("version")
                                .unwrap_or("0.0.0").to_string(),
                        };

                        let platform_str = peer.platform.clone();
                        let platform = match platform_str.as_str() {
                            "android" => Platform::Android,
                            "mac" | "macos" => Platform::Mac,
                            "linux" => Platform::Linux,
                            "windows" => Platform::Windows,
                            "ios" => Platform::Unknown("ios".into()),
                            _ => Platform::Unknown(peer.platform.clone()),
                        };

                        // Use a stable node_id derived from peer address + name
                        // so the same peer always gets the same NodeId across rediscoveries.
                        let stable_id = format!("{}:{}@{}", peer.host, peer.port, peer.node_name);
                        let node_id = NodeId(uuid::Uuid::new_v5(
                            &uuid::Uuid::NAMESPACE_DNS,
                            stable_id.as_bytes(),
                        ));

                        let node = DiscoveredNode {
                            node_id,
                            name: peer.node_name.clone(),
                            platform,
                            capabilities: vec![Capability::FileTransfer],
                            method: DiscoveryMethod::Mdns,
                            address: format!("{}:{}", peer.host, peer.port),
                        };

                        tracing::info!(
                            name = %node.name,
                            addr = %node.address,
                            platform = ?node.platform,
                            "discovered peer via mDNS"
                        );
                        on_discovered(node);
                    }
                    Ok(ServiceEvent::ServiceFound(_, _)) => {}
                    Ok(ServiceEvent::ServiceRemoved(_, _)) => {}
                    Err(e) => {
                        tracing::error!(error = %e, "mDNS recv error");
                        break;
                    }
                    _ => {}
                }
            }
        });

        Ok(())
    }

    async fn stop(&self) {
        let mut guard = self.daemon.lock().await;
        if let Some(d) = guard.take() {
            let _ = d.shutdown();
        }
    }
}
