use std::path::PathBuf;
use crate::{TransferRequest, RemoteTarget, TransferDirection, TransferOptions};

/// Start a transfer using the unified transfer engine
/// 
/// This replaces the fragmented implementations from:
/// - http_transfer.rs
/// - transport.rs/udp_transport.rs/quic_transport.rs
/// - zero_copy.rs
/// 
/// Now everything goes through the single core engine.
pub async fn start_transfer_with_engine(
    source: &str,
    destination: &str,
    options: TransferOptions,
) -> Result<(), Box<dyn std::error::Error>> {
    let source_path = PathBuf::from(source);
    let destination_target = parse_destination(destination);
    let transfer_direction = determine_direction(&source_path, &destination_target);
    
    let request = TransferRequest {
        source: source_path,
        destination: destination_target,
        direction: transfer_direction,
        options,
    };

    let mut engine = transfer_engine::TransferCoordinator::new();
    let handle = engine.start_transfer(request).await?;
    
    println!("Transfer started: {} -> {} (ID: {})", 
             source, destination, handle.id);
    
    // Wait for completion (for CLI we'll use simple blocking)
    // In production, you'd use async progress callbacks
    loop {
        let status = engine.coordinator.get_handle_status(&handle.id).await?;
        
        if status.state == transfer_engine::TransferState::Completed {
            println!("Transfer completed: {}", handle.id);
            return Ok(());
        } else if status.state == transfer_engine::TransferState::Failed {
            return Err(format!("Transfer failed: {}", status.error.unwrap_or_default()).into());
        }
        
        // Small delay to avoid busy waiting
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    }
}

fn parse_destination(destination: &str) -> RemoteTarget {
    let parts: Vec<&str> = destination.split('/').collect();
    
    match parts[0] {
        "http" | "https" => {
            let after_protocol = destination.replace("http://", "").replace("https://", "");
            let url_parts: Vec<&str> = after_protocol.split('/').collect();
            let host_port = url_parts[0];
            let host_port_parts: Vec<&str> = host_port.split(':').collect();
            let host = host_port_parts[0];
            let port = host_port_parts.get(1).and_then(|p| p.parse::<u16>().ok()).unwrap_or(8080);
            let path = if url_parts.len() > 1 {
                format!("/{}", url_parts[1..].join("/"))
            } else {
                "/".to_string()
            };
            RemoteTarget::Http {
                host: host.to_string(),
                port,
                path: Some(path),
            }
        }
        _ => {
            // Assume TCP for anything else
            let parts: Vec<&str> = destination.split('/').collect();
            let host_port = parts[0];
            let host_port_parts: Vec<&str> = host_port.split(':').collect();
            let host = host_port_parts[0];
            let port = host_port_parts.get(1).and_then(|p| p.parse::<u16>().ok()).unwrap_or(8080);
            RemoteTarget::Tcp {
                host: host.to_string(),
                port,
                path: if parts.len() > 1 { Some(format!("/{}", parts[1..].join("/"))) } else { None },
            }
        }
    }
}

fn determine_direction(source: &PathBuf, destination: &RemoteTarget) -> TransferDirection {
    // Default to upload if source exists and is a file/directory
    if source.exists() {
        TransferDirection::Upload
    } else {
        TransferDirection::Download
    }
}
