use std::sync::Arc;
use std::time::Duration;

use mdns_sd::{ServiceDaemon, ServiceEvent};
use tokio::sync::Mutex;
use tracing::{info, error};

#[derive(Debug, Clone)]
pub struct P2pNode {
    pub name: String,
    pub ip: String,
    pub port: u16,
    pub node_name: String,
    pub platform: String,
}

pub async fn discover_nodes(
    discovered: Arc<Mutex<Vec<P2pNode>>>,
    timeout_secs: u64,
) -> anyhow::Result<()> {
    let mdns = ServiceDaemon::new()?;
    let receiver = mdns.browse("_xync._tcp.local.")?;

    let d = discovered.clone();
    let handle = tokio::spawn(async move {
        loop {
            match receiver.recv_async().await {
                Ok(ServiceEvent::ServiceResolved(svc)) => {
                    let ip = svc
                        .get_addresses_v4()
                        .iter()
                        .next()
                        .map(|a| a.to_string())
                        .unwrap_or_default();
                    let node = P2pNode {
                        name: svc.get_fullname().to_string(),
                        ip,
                        port: svc.get_port(),
                        node_name: svc
                            .get_property_val_str("node_name")
                            .unwrap_or("unknown")
                            .to_string(),
                        platform: svc
                            .get_property_val_str("platform")
                            .unwrap_or("unknown")
                            .to_string(),
                    };
                    let mut list = d.lock().await;
                    if !list.iter().any(|n: &P2pNode| n.name == node.name) {
                        info!("Discovered: {} at {}:{}", node.node_name, node.ip, node.port);
                        list.push(node);
                    }
                }
                Ok(ServiceEvent::ServiceRemoved(_, fullname)) => {
                    let mut list = d.lock().await;
                    list.retain(|n| n.name != fullname);
                }
                Err(e) => {
                    error!(error = %e, "mDNS recv error");
                    break;
                }
                _ => {}
            }
        }
    });

    tokio::time::sleep(Duration::from_secs(timeout_secs)).await;
    handle.abort();
    mdns.shutdown()?;
    Ok(())
}
