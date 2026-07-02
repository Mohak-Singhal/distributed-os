use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use tokio::sync::Semaphore;

/// Bounded in-flight byte window to cap memory per connection.
///
/// Before writing a chunk, the caller acquires permits for its size.
/// After the write completes (or is acked), permits are released.
/// This prevents unbounded buffering in the transport layer.
///
/// Default max: 4 MB per connection.
pub struct FlowWindow {
    semaphore: Arc<Semaphore>,
    max_bytes: u64,
    in_flight: AtomicU64,
}

impl FlowWindow {
    /// Create a new flow window with the given maximum in-flight bytes.
    pub fn new(max_bytes: u64) -> Self {
        let max = max_bytes.max(65536); // at least 64KB
        Self {
            semaphore: Arc::new(Semaphore::new(max as usize)),
            max_bytes: max,
            in_flight: AtomicU64::new(0),
        }
    }

    /// Acquire permits for `size` bytes. Awaits until capacity is available.
    pub async fn acquire(&self, size: u64) -> WindowPermit<'_> {
        let acquired = size.min(self.max_bytes) as u32;
        let permit = self.semaphore.acquire_many(acquired).await.unwrap();
        self.in_flight.fetch_add(acquired as u64, Ordering::Relaxed);
        WindowPermit {
            _permit: permit,
            window: self,
            size: acquired as u64,
        }
    }

    /// Current number of in-flight bytes.
    pub fn in_flight_bytes(&self) -> u64 {
        self.in_flight.load(Ordering::Relaxed)
    }

    /// Maximum allowed in-flight bytes.
    pub fn max_bytes(&self) -> u64 {
        self.max_bytes
    }

    fn release(&self, size: u64) {
        self.in_flight.fetch_sub(size, Ordering::Relaxed);
    }
}

impl Default for FlowWindow {
    fn default() -> Self {
        Self::new(4 * 1024 * 1024) // 4 MB default
    }
}

/// A permit that releases bytes from the flow window when dropped.
pub struct WindowPermit<'a> {
    _permit: tokio::sync::SemaphorePermit<'a>,
    window: &'a FlowWindow,
    size: u64,
}

impl Drop for WindowPermit<'_> {
    fn drop(&mut self) {
        self.window.release(self.size);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_flow_window_basic() {
        let w = FlowWindow::new(65536);
        assert_eq!(w.max_bytes(), 65536);
        assert_eq!(w.in_flight_bytes(), 0);

        let p = w.acquire(512).await;
        assert_eq!(w.in_flight_bytes(), 512);
        drop(p);
        assert_eq!(w.in_flight_bytes(), 0);
    }

    #[tokio::test]
    async fn test_flow_window_backpressure() {
        let w = FlowWindow::new(65536);
        let p1 = w.acquire(65536).await;
        assert_eq!(w.in_flight_bytes(), 65536);

        // This would block if awaited — just verify the semaphore is exhausted
        assert_eq!(w.semaphore.available_permits(), 0);
        drop(p1);
        assert_eq!(w.semaphore.available_permits(), 65536);
    }
}
