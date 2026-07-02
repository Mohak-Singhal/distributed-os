use std::collections::VecDeque;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Priority {
    High = 0,
    Medium = 1,
    Low = 2,
}

#[derive(Debug, Clone)]
pub struct TransferJob {
    pub id: String,
    pub src_path: String,
    pub dst_path: String,
    pub size: u64,
    pub priority: Priority,
    pub retry_count: u8,
    pub max_retries: u8,
}

pub struct TransferScheduler {
    pub high: VecDeque<TransferJob>,
    pub medium: VecDeque<TransferJob>,
    pub low: VecDeque<TransferJob>,
    pub in_flight: Vec<TransferJob>,
    pub max_concurrent: u32,
}

impl TransferScheduler {
    pub fn new(max_concurrent: u32) -> Self {
        Self {
            high: VecDeque::new(),
            medium: VecDeque::new(),
            low: VecDeque::new(),
            in_flight: Vec::new(),
            max_concurrent,
        }
    }

    pub fn enqueue(&mut self, job: TransferJob) {
        match job.priority {
            Priority::High => self.high.push_back(job),
            Priority::Medium => self.medium.push_back(job),
            Priority::Low => self.low.push_back(job),
        }
    }

    pub fn dequeue(&mut self) -> Option<TransferJob> {
        if self.in_flight.len() >= self.max_concurrent as usize {
            return None;
        }
        let job = self.high.pop_front()
            .or_else(|| self.medium.pop_front())
            .or_else(|| self.low.pop_front())?;
        self.in_flight.push(job.clone());
        Some(job)
    }

    pub fn complete(&mut self, id: &str) {
        self.in_flight.retain(|j| j.id != id);
    }

    pub fn fail(&mut self, id: &str) {
        if let Some(idx) = self.in_flight.iter().position(|j| j.id == id) {
            let mut job = self.in_flight.remove(idx);
            if job.retry_count < job.max_retries {
                job.retry_count += 1;
                self.enqueue(job);
            }
        }
    }

    pub fn pending_count(&self) -> usize {
        self.high.len() + self.medium.len() + self.low.len()
    }
}
