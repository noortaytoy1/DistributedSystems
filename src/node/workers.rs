//! Worker pool with a bounded queue for CPU-bound planning.
//!
//! Planning never runs on the async runtime. At most `threads` jobs should run at once and at
//! most `queue_capacity` more wait; beyond that the pool sheds load with [`Overloaded`] and the
//! client's retry takes care of the request.
//!
//! Owner: E (planner and concurrency). Template: jobs run on tokio's blocking pool, unbounded.

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("worker pool is full")]
pub struct Overloaded;

/// Queue and thread usage, sampled for the load score.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PoolLoad {
    pub queued: usize,
    pub running: usize,
    pub threads: usize,
}

#[derive(Debug, Clone)]
pub struct WorkerPool {
    threads: usize,
    queue_capacity: usize,
}

impl WorkerPool {
    pub fn new(threads: usize, queue_capacity: usize) -> Self {
        Self {
            threads: threads.max(1),
            queue_capacity,
        }
    }

    /// Run a CPU-bound job off the async runtime.
    pub async fn run<T, F>(&self, job: F) -> Result<T, Overloaded>
    where
        T: Send + 'static,
        F: FnOnce() -> T + Send + 'static,
    {
        // TODO(E): at most `self.threads` jobs running and `self.queue_capacity` waiting;
        // return `Overloaded` at once when full.
        match tokio::task::spawn_blocking(job).await {
            Ok(out) => Ok(out),
            Err(e) if e.is_panic() => std::panic::resume_unwind(e.into_panic()),
            Err(_) => Err(Overloaded),
        }
    }

    pub fn load(&self) -> PoolLoad {
        // TODO(E): report real queue and thread usage.
        let _ = self.queue_capacity;
        PoolLoad {
            queued: 0,
            running: 0,
            threads: self.threads,
        }
    }
}
