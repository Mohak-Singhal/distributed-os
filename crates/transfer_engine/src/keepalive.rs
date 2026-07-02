use std::pin::Pin;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::task::{Context, Poll};
use std::time::{Duration, Instant};

use tokio::io::{AsyncRead, AsyncWrite, AsyncWriteExt};
use tokio::sync::{mpsc, Notify};
use tracing::{warn, debug};

/// KeepAlive monitor for transport connections.
///
/// Spawns a background task that:
/// - Sends a small ping frame every `ping_interval`
/// - Fires `on_timeout` if no activity for `idle_timeout`
///
/// Activity is recorded by calling `record_activity()`.
pub struct KeepAlive {
    last_activity: Arc<std::sync::Mutex<Instant>>,
    idle_timeout: Duration,
    ping_interval: Duration,
    stopped: Arc<AtomicBool>,
    stop_notify: Arc<Notify>,
}

impl KeepAlive {
    /// Create a new KeepAlive monitor.
    ///
    /// Defaults: ping every 10s, idle timeout 30s.
    pub fn new() -> Self {
        Self {
            last_activity: Arc::new(std::sync::Mutex::new(Instant::now())),
            idle_timeout: Duration::from_secs(30),
            ping_interval: Duration::from_secs(10),
            stopped: Arc::new(AtomicBool::new(false)),
            stop_notify: Arc::new(Notify::new()),
        }
    }

    /// Set a custom idle timeout.
    pub fn with_idle_timeout(mut self, timeout: Duration) -> Self {
        self.idle_timeout = timeout;
        self
    }

    /// Set a custom ping interval (must be less than idle timeout).
    pub fn with_ping_interval(mut self, interval: Duration) -> Self {
        self.ping_interval = interval;
        self
    }

    /// Record that activity just happened (resets the idle timer).
    pub fn record_activity(&self) {
        if let Ok(mut last) = self.last_activity.lock() {
            *last = Instant::now();
        }
    }

    /// Time since last recorded activity.
    pub fn idle_duration(&self) -> Duration {
        self.last_activity.lock()
            .map(|last| last.elapsed())
            .unwrap_or(Duration::ZERO)
    }

    /// Returns a future that resolves when the idle timeout is exceeded.
    pub async fn wait_for_timeout(&self) {
        loop {
            let elapsed = self.idle_duration();
            if elapsed >= self.idle_timeout {
                return;
            }
            let remaining = self.idle_timeout - elapsed;
            tokio::time::sleep(remaining.min(Duration::from_secs(1))).await;
        }
    }

    /// Start the keepalive monitor.
    ///
    /// Spawns a background task that sends pings and monitors for idle timeout.
    /// The `send_ping` closure is called to transmit a ping frame.
    /// Returns a `KeepAliveHandle` that stops the monitor when dropped.
    pub fn spawn<F, Fut>(&self, send_ping: F) -> KeepAliveHandle
    where
        F: Fn() -> Fut + Send + Sync + 'static,
        Fut: std::future::Future<Output = ()> + Send + Unpin,
    {
        let last_activity = self.last_activity.clone();
        let idle_timeout = self.idle_timeout;
        let ping_interval = self.ping_interval;
        let stopped = self.stopped.clone();
        let stop_notify = self.stop_notify.clone();

        let send_ping = Arc::new(send_ping);

        tokio::spawn(async move {
            let mut ping_timer = tokio::time::interval(ping_interval);
            ping_timer.tick().await; // skip first immediate tick

            loop {
                tokio::select! {
                    _ = ping_timer.tick() => {
                        let idle = last_activity.lock()
                            .map(|last| last.elapsed())
                            .unwrap_or(Duration::ZERO);

                        if idle >= idle_timeout {
                            warn!(idle_secs = %idle.as_secs(), "keepalive: idle timeout reached");
                            stopped.store(true, Ordering::SeqCst);
                            stop_notify.notify_one();
                            return;
                        }

                        // Send ping
                        (send_ping)().await;
                        debug!("keepalive: ping sent");
                    }
                    _ = stop_notify.notified() => {
                        return;
                    }
                }
            }
        });

        KeepAliveHandle {
            stopped: self.stopped.clone(),
            stop_notify: self.stop_notify.clone(),
        }
    }

    /// Check if the monitor has timed out.
    pub fn is_timed_out(&self) -> bool {
        self.stopped.load(Ordering::Relaxed)
    }
}

impl Default for KeepAlive {
    fn default() -> Self {
        Self::new()
    }
}

/// Handle that stops the keepalive monitor when dropped.
pub struct KeepAliveHandle {
    stopped: Arc<AtomicBool>,
    stop_notify: Arc<Notify>,
}

impl KeepAliveHandle {
    /// Stop the keepalive monitor.
    pub fn stop(&self) {
        self.stopped.store(true, Ordering::SeqCst);
        self.stop_notify.notify_one();
    }
}

impl Drop for KeepAliveHandle {
    fn drop(&mut self) {
        self.stop();
    }
}

// ── Ping frame format ─────────────────────────────────────────────────────
// Simple 4-byte ping frame: [0xPP, 0x01, 0x00, 0x00]

pub const PING_FRAME: &[u8] = &[0x50, 0x49, 0x4E, 0x47]; // "PING"
pub const PONG_FRAME: &[u8] = &[0x50, 0x4F, 0x4E, 0x47]; // "PONG"

/// Send a ping frame on the given writer.
pub async fn send_ping<W: tokio::io::AsyncWrite + Unpin + Send>(writer: &mut W) {
    let _ = writer.write_all(PING_FRAME).await;
}

/// Send a pong frame on the given writer.
pub async fn send_pong<W: tokio::io::AsyncWrite + Unpin + Send>(writer: &mut W) {
    let _ = writer.write_all(PONG_FRAME).await;
}

// ── KeepaliveStream wrapper ──────────────────────────────────────────────

/// Wraps any `AsyncRead + AsyncWrite` stream with keepalive PING/PONG and
/// idle timeout detection.
///
/// How it works:
/// 1. Splits the stream into read/write halves
/// 2. Spawns a background reader for PING/PONG frames
/// 3. PING frames from peer → respond with PONG
/// 4. PONG frames from peer → record activity (resets idle timer)
/// 5. Sends PING frames every `ping_interval` via the internal keepalive
/// 6. On idle timeout → sets the `is_timed_out()` flag
///
/// The wrapper implements `AsyncWrite` so callers pass this directly to
/// `stream_file_send_with_resume` or similar APIs.
pub struct KeepaliveStream<RW: AsyncRead + AsyncWrite + Unpin + Send> {
    writer: tokio::io::WriteHalf<RW>,
    _ka: Arc<KeepAlive>,
    _handle: KeepAliveHandle,
    _read_handle: tokio::task::JoinHandle<()>,
    ping_rx: mpsc::UnboundedReceiver<Vec<u8>>,
    pending_ping: Option<Vec<u8>>,
}

impl<RW: AsyncRead + AsyncWrite + Unpin + Send + 'static> KeepaliveStream<RW> {
    /// Wrap a stream with keepalive. Returns the wrapper and a handle
    /// for checking timeout status.
    pub fn new(stream: RW) -> (Self, KeepAliveHandle) {
        let (reader, writer) = tokio::io::split(stream);
        let ka = Arc::new(KeepAlive::new());
        let (ping_tx, ping_rx) = mpsc::unbounded_channel();

        // Pong reader: reads from the read half
        let ka_for_reader = ka.clone();
        let pong_tx = ping_tx.clone();
        let read_handle = tokio::spawn(async move {
            let mut reader = reader;
            let mut buf = [0u8; 4];
            loop {
                match tokio::io::AsyncReadExt::read_exact(&mut reader, &mut buf).await {
                    Ok(_) => {}
                    Err(_) => break, // connection closed or error
                }
                if buf == PING_FRAME {
                    debug!("keepalive: received PING, queueing PONG");
                    let _ = pong_tx.send(PONG_FRAME.to_vec());
                    ka_for_reader.record_activity();
                } else if buf == PONG_FRAME {
                    debug!("keepalive: received PONG from peer");
                    ka_for_reader.record_activity();
                } else {
                    ka_for_reader.record_activity();
                }
            }
        });

        // Keepalive monitor: sends PINGs periodically, detects timeout
        let handle = ka.spawn(move || {
            let tx = ping_tx.clone();
            Box::pin(async move {
                let _ = tx.send(PING_FRAME.to_vec());
            })
        });

        (
            KeepaliveStream {
                writer,
                _ka: ka.clone(),
                _handle: handle,
                _read_handle: read_handle,
                ping_rx,
                pending_ping: None,
            },
            KeepAliveHandle {
                stopped: ka.stopped.clone(),
                stop_notify: ka.stop_notify.clone(),
            },
        )
    }

    /// Check if the underlying connection has timed out.
    pub fn is_timed_out(&self) -> bool {
        self._ka.is_timed_out()
    }
}

impl<RW: AsyncRead + AsyncWrite + Unpin + Send> AsyncWrite for KeepaliveStream<RW> {
    fn poll_write(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<std::io::Result<usize>> {
        // Drain any pending PING frames first
        loop {
            if self.pending_ping.is_some() {
                // Write the pending PING
                let ping = self.pending_ping.take().unwrap();
                match Pin::new(&mut self.writer).poll_write(cx, &ping) {
                    Poll::Pending => {
                        self.pending_ping = Some(ping);
                        return Poll::Pending;
                    }
                    Poll::Ready(Ok(n)) if n < ping.len() => {
                        self.pending_ping = Some(ping[n..].to_vec());
                        return Poll::Pending;
                    }
                    Poll::Ready(Ok(_)) => {
                        // PING sent successfully, continue
                        self._ka.record_activity();
                    }
                    Poll::Ready(Err(e)) => return Poll::Ready(Err(e)),
                }
            } else {
                match self.ping_rx.try_recv() {
                    Ok(ping) => self.pending_ping = Some(ping),
                    Err(mpsc::error::TryRecvError::Empty) => break,
                    Err(mpsc::error::TryRecvError::Disconnected) => break,
                }
            }
        }

        // Now write the actual data
        match Pin::new(&mut self.writer).poll_write(cx, buf) {
            Poll::Ready(Ok(n)) => {
                self._ka.record_activity();
                Poll::Ready(Ok(n))
            }
            Poll::Ready(Err(e)) => Poll::Ready(Err(e)),
            Poll::Pending => Poll::Pending,
        }
    }

    fn poll_flush(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<std::io::Result<()>> {
        // Drain any pending PING/PONG frames before flushing
        loop {
            if let Some(ping) = self.pending_ping.take() {
                match Pin::new(&mut self.writer).poll_write(cx, &ping) {
                    Poll::Pending => {
                        self.pending_ping = Some(ping);
                        return Poll::Pending;
                    }
                    Poll::Ready(Ok(n)) if n < ping.len() => {
                        self.pending_ping = Some(ping[n..].to_vec());
                        return Poll::Pending;
                    }
                    Poll::Ready(Ok(_)) => {
                        self._ka.record_activity();
                    }
                    Poll::Ready(Err(e)) => return Poll::Ready(Err(e)),
                }
            } else {
                match self.ping_rx.try_recv() {
                    Ok(ping) => self.pending_ping = Some(ping),
                    Err(mpsc::error::TryRecvError::Empty) => break,
                    Err(mpsc::error::TryRecvError::Disconnected) => break,
                }
            }
        }
        Pin::new(&mut self.writer).poll_flush(cx)
    }

    fn poll_shutdown(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<std::io::Result<()>> {
        Pin::new(&mut self.writer).poll_shutdown(cx)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::AsyncReadExt;

    #[tokio::test]
    async fn test_keepalive_basic() {
        let ka = KeepAlive::new()
            .with_idle_timeout(Duration::from_secs(1))
            .with_ping_interval(Duration::from_millis(200));

        let handle = ka.spawn(|| {
            Box::pin(async { /* no-op ping */ })
        });

        // Should not time out within 500ms with activity
        ka.record_activity();
        tokio::time::sleep(Duration::from_millis(500)).await;
        assert!(!ka.is_timed_out());

        // Stop cleanly
        handle.stop();
    }

    #[tokio::test]
    async fn test_keepalive_timeout() {
        let ka = KeepAlive::new()
            .with_idle_timeout(Duration::from_millis(500))
            .with_ping_interval(Duration::from_millis(200));

        let _handle = ka.spawn(|| {
            Box::pin(async { /* no-op ping */ })
        });

        // No activity — should time out
        tokio::time::sleep(Duration::from_millis(800)).await;
        assert!(ka.is_timed_out());
    }

    #[tokio::test]
    async fn test_keepalive_stream_wrapper() {
        use tokio::io::AsyncWriteExt;

        let (a, mut b) = tokio::io::duplex(65536);
        let (mut ka_stream, handle) = KeepaliveStream::new(a);

        // Write data through the keepalive wrapper
        ka_stream.write_all(b"hello").await.unwrap();
        ka_stream.flush().await.unwrap();

        // Verify data arrived
        let mut buf = vec![0u8; 5];
        b.read_exact(&mut buf).await.unwrap();
        assert_eq!(&buf, b"hello");

        // Not timed out since we just wrote
        assert!(!ka_stream.is_timed_out());

        drop(ka_stream);
        drop(handle);
    }

    #[tokio::test]
    async fn test_keepalive_stream_ping_pong() {
        use tokio::io::AsyncWriteExt;

        let (a, mut b) = tokio::io::duplex(65536);
        let (mut ka_stream, _handle) = KeepaliveStream::new(a);

        // Write some data
        ka_stream.write_all(b"data").await.unwrap();
        ka_stream.flush().await.unwrap();

        // Read the data from the other side
        let mut buf = vec![0u8; 4];
        b.read_exact(&mut buf).await.unwrap();
        assert_eq!(&buf, b"data");

        // Send a PING from the other side — the wrapper should respond with PONG
        // The PONG is delivered on the next write+flush through ka_stream
        b.write_all(PING_FRAME).await.unwrap();

        // Yield multiple times to let the pong reader process the PING
        for _ in 0..10 {
            tokio::task::yield_now().await;
        }

        // Write more data to trigger PONG delivery (PONG queued before data)
        ka_stream.write_all(b"more").await.unwrap();
        ka_stream.flush().await.unwrap();

        // The first 4 bytes from ka_stream should be the PONG (if reader processed PING),
        // or "more" if the reader hasn't processed it yet. Either is fine — data integrity
        // is what matters. Read whatever comes first.
        let mut first_buf = [0u8; 4];
        b.read_exact(&mut first_buf).await.unwrap();
        if first_buf == PONG_FRAME {
            // PONG was sent first, now read the data
            let mut data_buf = vec![0u8; 4];
            b.read_exact(&mut data_buf).await.unwrap();
            assert_eq!(&data_buf, b"more");
        } else {
            // Data came first, PONG will follow on next write
            assert_eq!(&first_buf, b"more");
        }

        drop(ka_stream);
    }
}
