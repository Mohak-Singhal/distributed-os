/// Buffer pool that reuses allocations to minimize GC / allocator churn.
pub struct BufferPool {
    buffers: std::sync::Mutex<Vec<Vec<u8>>>,
    chunk_size: usize,
    max_pooled: usize,
}

impl BufferPool {
    pub fn new(chunk_size: usize, max_pooled: usize) -> Self {
        Self {
            buffers: std::sync::Mutex::new(Vec::with_capacity(max_pooled)),
            chunk_size,
            max_pooled,
        }
    }

    pub fn acquire(&self) -> Vec<u8> {
        let mut pool = self.buffers.lock().unwrap();
        pool.pop().unwrap_or_else(|| vec![0u8; self.chunk_size])
    }

    pub fn release(&self, mut buf: Vec<u8>) {
        buf.clear();
        let mut pool = self.buffers.lock().unwrap();
        if pool.len() < self.max_pooled {
            pool.push(buf);
        }
    }
}
