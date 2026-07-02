//! Capability exchange and parameter negotiation between peers.
//!
//! The handshake happens over HTTP (`POST /api/handshake`) before any
//! file data is sent. Both sides exchange capabilities, then independently
//! compute the agreed transfer parameters via [`negotiate`].

use serde::{Deserialize, Serialize};

use crate::transport::selection::TransportSupport;
use crate::{TransferDirection, TransferOptions};

/// Full capability exchange payload sent during handshake.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct HandshakePayload {
    pub protocol_version: String,
    pub node_id: String,
    pub node_name: String,
    pub direction: TransferDirection,
    pub features: FeatureFlags,
    pub limits: CapabilityLimits,
    /// Supported transports (for transport selection).
    pub transport_support: Option<TransportSupport>,
}

/// Feature flags advertised by a peer.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FeatureFlags {
    pub parallel_upload: bool,
    pub parallel_download: bool,
    pub resume: bool,
    pub compression: bool,
    pub checksum: bool,
    pub zero_copy: bool,
    pub reliable: bool,
}

impl Default for FeatureFlags {
    fn default() -> Self {
        Self {
            parallel_upload: true,
            parallel_download: false,
            resume: true,
            compression: false,
            checksum: true,
            zero_copy: false,
            reliable: true,
        }
    }
}

/// Resource limits for transfer parameter negotiation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CapabilityLimits {
    pub max_chunk_size_bytes: usize,
    pub min_chunk_size_bytes: usize,
    pub max_parallel_streams: usize,
    pub max_throughput_mbps: Option<f64>,
    pub min_throughput_mbps: Option<f64>,
    pub max_send_buffer_kb: usize,
}

impl Default for CapabilityLimits {
    fn default() -> Self {
        Self {
            max_chunk_size_bytes: 2 * 1024 * 1024,
            min_chunk_size_bytes: 16 * 1024,
            max_parallel_streams: 8,
            max_throughput_mbps: None,
            min_throughput_mbps: Some(10.0),
            max_send_buffer_kb: 8192,
        }
    }
}

/// Negotiate mutually agreeable transfer options from local and remote
/// capabilities. Uses a conservative approach: pick the minimum of both
/// sides' maxima and the maximum of both sides' minima.
///
/// This is called independently on both sides after the handshake
/// exchange, so the resulting options are deterministic.
pub fn negotiate(
    local: &HandshakePayload,
    remote: &HandshakePayload,
) -> TransferOptions {
    let agreed_chunk = remote
        .limits
        .max_chunk_size_bytes
        .min(local.limits.max_chunk_size_bytes)
        .max(remote.limits.min_chunk_size_bytes.max(local.limits.min_chunk_size_bytes));

    let agreed_parallel = remote
        .limits
        .max_parallel_streams
        .min(local.limits.max_parallel_streams);

    let enable_parallel = local.features.parallel_upload && remote.features.parallel_upload;
    let use_reliable = local.features.reliable && remote.features.reliable;

    let limit = match (local.limits.max_throughput_mbps, remote.limits.max_throughput_mbps) {
        (Some(a), Some(b)) => Some(a.min(b)),
        (Some(a), None) => Some(a),
        (None, Some(b)) => Some(b),
        (None, None) => None,
    };

    TransferOptions {
        chunk_size: agreed_chunk,
        parallel_streams: agreed_parallel,
        parallel: enable_parallel && agreed_parallel > 1,
        checksum: local.features.checksum && remote.features.checksum,
        compression: local.features.compression && remote.features.compression,
        resume: local.features.resume && remote.features.resume,
        zero_copy: local.features.zero_copy && remote.features.zero_copy,
        reliable: use_reliable,
        throughput_limit_mbps: limit,
        send_buffer_kb: local.limits.max_send_buffer_kb.min(remote.limits.max_send_buffer_kb),
        recv_buffer_kb: local.limits.max_send_buffer_kb.min(remote.limits.max_send_buffer_kb),
        ..Default::default()
    }
}

/// Perform an HTTP handshake with a remote peer.
///
/// Sends our capabilities to `POST /api/handshake` and returns the
/// remote peer's capabilities on success.
///
/// If `resume_offset > 0`, includes a `X-Resume-Offset` header so the
/// receiver can skip already-transferred data.
pub async fn perform_handshake(
    host: &str,
    port: u16,
    our_payload: &HandshakePayload,
    resume_offset: u64,
) -> anyhow::Result<HandshakePayload> {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpStream;

    let mut stream = TcpStream::connect(format!("{}:{}", host, port)).await?;
    let body = serde_json::to_string(our_payload)?;

    let resume_header = if resume_offset > 0 {
        format!("X-Resume-Offset: {}\r\n", resume_offset)
    } else {
        String::new()
    };

    let request = format!(
        "POST /api/handshake HTTP/1.1\r\n\
         Host: {}:{}\r\n\
         Content-Type: application/json\r\n\
         Content-Length: {}\r\n\
         {}Connection: close\r\n\
         \r\n\
         {}",
        host, port, body.len(), resume_header, body
    );

    stream.write_all(request.as_bytes()).await?;

    let mut buf = Vec::new();
    stream.read_to_end(&mut buf).await?;
    let resp = String::from_utf8_lossy(&buf);

    // Parse the response to extract X-Accepted-Offset for resume confirmation
    let accepted_offset = resp.lines()
        .find(|l| l.to_lowercase().starts_with("x-accepted-offset:"))
        .and_then(|l| l.split(':').nth(1))
        .and_then(|s| s.trim().parse::<u64>().ok());

    if let Some(body_start) = resp.find("\r\n\r\n") {
        let body = &resp[body_start + 4..];
        if resp.contains("200 OK") {
            let remote: HandshakePayload = serde_json::from_str(body)
                .map_err(|e| {
                    anyhow::anyhow!(
                        "Handshake parse error: {} — body: {}",
                        e,
                        body.chars().take(200).collect::<String>()
                    )
                })?;

            // Log the resume negotiation
            if let Some(accepted) = accepted_offset {
                tracing::info!(
                    "Resume accepted: offset {} (requested {})",
                    accepted, resume_offset
                );
            }

            return Ok(remote);
        }
    }
    Err(anyhow::anyhow!(
        "Handshake failed: {}",
        resp.lines().next().unwrap_or("?")
    ))
}

/// Build a local handshake payload for the given direction and options.
pub fn build_payload(
    node_id: &str,
    node_name: &str,
    direction: TransferDirection,
    options: &TransferOptions,
) -> HandshakePayload {
    let features = FeatureFlags {
        parallel_upload: options.parallel,
        parallel_download: false,
        resume: options.resume,
        compression: options.compression,
        checksum: options.checksum,
        zero_copy: options.zero_copy,
        reliable: options.reliable,
    };

    let limits = CapabilityLimits {
        max_chunk_size_bytes: 2 * 1024 * 1024,
        min_chunk_size_bytes: 16 * 1024,
        max_parallel_streams: options.parallel_streams.max(1).min(8),
        max_throughput_mbps: options.throughput_limit_mbps,
        min_throughput_mbps: Some(10.0),
        max_send_buffer_kb: options.send_buffer_kb.max(options.recv_buffer_kb),
    };

    HandshakePayload {
        protocol_version: env!("CARGO_PKG_VERSION").to_string(),
        node_id: node_id.to_string(),
        node_name: node_name.to_string(),
        direction,
        features,
        limits,
        transport_support: Some(crate::transport::selection::TransportSupport::default()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_negotiate_conservative() {
        let local = HandshakePayload {
            protocol_version: "1.0".into(),
            node_id: "local".into(),
            node_name: "MacBook".into(),
            direction: TransferDirection::Upload,
            features: FeatureFlags {
                parallel_upload: true,
                ..Default::default()
            },
            limits: CapabilityLimits {
                max_chunk_size_bytes: 1_048_576,
                max_parallel_streams: 4,
                ..Default::default()
            },
            ..Default::default()
        };

        let remote = HandshakePayload {
            protocol_version: "1.0".into(),
            node_id: "remote".into(),
            node_name: "Phone".into(),
            direction: TransferDirection::Download,
            features: FeatureFlags {
                parallel_upload: true,
                ..Default::default()
            },
            limits: CapabilityLimits {
                max_chunk_size_bytes: 512_000,
                max_parallel_streams: 2,
                ..Default::default()
            },
            ..Default::default()
        };

        let opts = negotiate(&local, &remote);
        assert_eq!(opts.chunk_size, 512_000);
        assert_eq!(opts.parallel_streams, 2);
        assert!(opts.parallel);
    }

    #[test]
    fn test_negotiate_no_parallel_if_not_supported() {
        let local = HandshakePayload {
            features: FeatureFlags {
                parallel_upload: true,
                ..Default::default()
            },
            limits: CapabilityLimits {
                max_parallel_streams: 8,
                ..Default::default()
            },
            ..Default::default()
        };

        let remote = HandshakePayload {
            features: FeatureFlags {
                parallel_upload: false,
                ..Default::default()
            },
            limits: CapabilityLimits {
                max_parallel_streams: 1,
                ..Default::default()
            },
            ..Default::default()
        };

        let opts = negotiate(&local, &remote);
        assert!(!opts.parallel);
        assert_eq!(opts.parallel_streams, 1);
    }
}
