//! Worker pool with a bounded queue for CPU-bound planning.
//!
//! Planning never runs on the async runtime. At most `threads` jobs run at once and at most
//! `queue_capacity` more wait; beyond that the pool sheds load with [`Overloaded`] and the
//! client's retry takes care of the request (D48).
//!
//! How (D71): an admission counter (`in_pool`, queued plus running) rejects a job at once when
//! the pool is full. Admitted jobs wait for a permit from a fair (FIFO) semaphore with `threads`
//! permits, then run on tokio's blocking pool while holding it.
//!
//! Cancellation and panics (D72): a waiting job whose caller is dropped leaves the queue at once.
//! A running job cannot be stopped, so it keeps its slot and counts as running until it really
//! ends; [`WorkerPool::load`] then always matches the CPU in use. A panicking job frees its slot
//! and the panic resumes in the caller.
//!
//! Owner: E (planner and concurrency).

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use tokio::sync::{OwnedSemaphorePermit, Semaphore};

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

/// Cheap to clone: clones share one pool.
#[derive(Debug, Clone)]
pub struct WorkerPool {
    inner: Arc<Inner>,
}

#[derive(Debug)]
struct Inner {
    threads: usize,
    queue_capacity: usize,
    /// One permit per worker thread; fair, so waiting jobs start in arrival order.
    permits: Arc<Semaphore>,
    /// Jobs admitted and not finished: queued plus running. The admission limit applies to it.
    in_pool: AtomicUsize,
    /// Jobs holding a permit whose closure has not returned yet.
    running: AtomicUsize,
}

impl WorkerPool {
    pub fn new(threads: usize, queue_capacity: usize) -> Self {
        let threads = threads.max(1);
        Self {
            inner: Arc::new(Inner {
                threads,
                queue_capacity,
                permits: Arc::new(Semaphore::new(threads)),
                in_pool: AtomicUsize::new(0),
                running: AtomicUsize::new(0),
            }),
        }
    }

    /// Run a CPU-bound job off the async runtime.
    ///
    /// Returns `Overloaded` at once, without waiting, when `threads` jobs are running and
    /// `queue_capacity` more are waiting.
    pub async fn run<T, F>(&self, job: F) -> Result<T, Overloaded>
    where
        T: Send + 'static,
        F: FnOnce() -> T + Send + 'static,
    {
        let slot = Slot::admit(&self.inner)?;
        // Dropping this future while it waits here drops `slot`, which leaves the queue.
        let permit = self
            .inner
            .permits
            .clone()
            .acquire_owned()
            .await
            // The semaphore is never closed; treat it like a full pool if that ever changes.
            .map_err(|_| Overloaded)?;
        let running = Running::start(slot, permit);
        // `running` moves into the job, so the slot is held until the job really ends, even if
        // the caller is dropped meanwhile, and is released during unwinding if the job panics.
        let task = tokio::task::spawn_blocking(move || {
            let _running = running;
            job()
        });
        match task.await {
            Ok(out) => Ok(out),
            Err(e) if e.is_panic() => std::panic::resume_unwind(e.into_panic()),
            // Cancelled: the runtime is shutting down.
            Err(_) => Err(Overloaded),
        }
    }

    pub fn load(&self) -> PoolLoad {
        // Read `running` first: a job that ends between the two reads then shows as still
        // running for this one sample instead of as phantom queued work.
        let running = self.inner.running.load(Ordering::SeqCst);
        let in_pool = self.inner.in_pool.load(Ordering::SeqCst);
        PoolLoad {
            queued: in_pool.saturating_sub(running),
            running,
            threads: self.inner.threads,
        }
    }
}

/// A place in the pool, queued or running. Frees itself when dropped.
#[derive(Debug)]
struct Slot {
    inner: Arc<Inner>,
}

impl Slot {
    fn admit(inner: &Arc<Inner>) -> Result<Self, Overloaded> {
        let limit = inner.threads + inner.queue_capacity;
        inner
            .in_pool
            .fetch_update(Ordering::SeqCst, Ordering::SeqCst, |n| {
                (n < limit).then_some(n + 1)
            })
            .map_err(|_| Overloaded)?;
        Ok(Self {
            inner: inner.clone(),
        })
    }
}

impl Drop for Slot {
    fn drop(&mut self) {
        self.inner.in_pool.fetch_sub(1, Ordering::SeqCst);
    }
}

/// A job that holds a worker permit. Counts as running until dropped.
#[derive(Debug)]
struct Running {
    // Dropped after `Drop::drop` runs, in this order: the permit, then the slot.
    _permit: OwnedSemaphorePermit,
    slot: Slot,
}

impl Running {
    fn start(slot: Slot, permit: OwnedSemaphorePermit) -> Self {
        slot.inner.running.fetch_add(1, Ordering::SeqCst);
        Self {
            _permit: permit,
            slot,
        }
    }
}

impl Drop for Running {
    fn drop(&mut self) {
        self.slot.inner.running.fetch_sub(1, Ordering::SeqCst);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Condvar, Mutex};
    use std::time::{Duration, Instant};

    /// Holds jobs on their worker thread until opened. Opens itself when dropped, so a failing
    /// assertion never leaves a blocking thread stuck and the runtime hanging at shutdown.
    struct Gate(Arc<(Mutex<bool>, Condvar)>);

    impl Gate {
        fn new() -> Self {
            Self(Arc::new((Mutex::new(false), Condvar::new())))
        }

        /// A closure for a job: blocks until the gate opens.
        fn job(&self) -> impl FnOnce() + Send + 'static {
            let shared = self.0.clone();
            move || {
                let (open, cv) = &*shared;
                let mut guard = open.lock().unwrap_or_else(|p| p.into_inner());
                while !*guard {
                    guard = cv.wait(guard).unwrap_or_else(|p| p.into_inner());
                }
            }
        }

        fn open(&self) {
            let (open, cv) = &*self.0;
            *open.lock().unwrap_or_else(|p| p.into_inner()) = true;
            cv.notify_all();
        }
    }

    impl Drop for Gate {
        fn drop(&mut self) {
            self.open();
        }
    }

    async fn wait_for_load(pool: &WorkerPool, queued: usize, running: usize) {
        let timeout = Duration::from_secs(5);
        let deadline = Instant::now() + timeout;
        loop {
            let load = pool.load();
            if load.queued == queued && load.running == running {
                return;
            }
            assert!(
                Instant::now() < deadline,
                "timed out after {timeout:?} waiting for queued={queued} running={running}, \
                 last load {load:?}"
            );
            tokio::time::sleep(Duration::from_millis(2)).await;
        }
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn limits_hold_and_a_full_pool_sheds_at_once() {
        let pool = WorkerPool::new(2, 2);
        let gate = Gate::new();
        let jobs: Vec<_> = (0..4)
            .map(|_| {
                let (pool, job) = (pool.clone(), gate.job());
                tokio::spawn(async move { pool.run(job).await })
            })
            .collect();
        wait_for_load(&pool, 2, 2).await;
        assert_eq!(pool.load().threads, 2);

        // Full: the fifth job is refused without waiting for a worker.
        let fifth = tokio::time::timeout(Duration::from_secs(1), pool.run(|| ())).await;
        assert_eq!(fifth, Ok(Err(Overloaded)));
        assert_eq!(pool.load().queued, 2, "a refused job takes no slot");

        gate.open();
        for job in jobs {
            assert_eq!(job.await.unwrap(), Ok(()));
        }
        wait_for_load(&pool, 0, 0).await;
        assert_eq!(pool.run(|| 7).await, Ok(7), "slots are reusable");
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn zero_queue_capacity_runs_only_the_threads() {
        let pool = WorkerPool::new(1, 0);
        let gate = Gate::new();
        let job = {
            let (pool, job) = (pool.clone(), gate.job());
            tokio::spawn(async move { pool.run(job).await })
        };
        wait_for_load(&pool, 0, 1).await;
        assert_eq!(pool.run(|| ()).await, Err(Overloaded));

        gate.open();
        assert_eq!(job.await.unwrap(), Ok(()));
        wait_for_load(&pool, 0, 0).await;
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn a_dropped_waiter_leaves_the_queue_but_a_running_job_keeps_its_slot() {
        let pool = WorkerPool::new(1, 1);
        let gate = Gate::new();
        let running = {
            let (pool, job) = (pool.clone(), gate.job());
            tokio::spawn(async move { pool.run(job).await })
        };
        wait_for_load(&pool, 0, 1).await;
        let waiting = {
            let (pool, job) = (pool.clone(), gate.job());
            tokio::spawn(async move { pool.run(job).await })
        };
        wait_for_load(&pool, 1, 1).await;

        waiting.abort();
        assert!(waiting.await.unwrap_err().is_cancelled());
        wait_for_load(&pool, 0, 1).await;

        // The caller goes away, but its job is still on the CPU and keeps counting.
        running.abort();
        assert!(running.await.unwrap_err().is_cancelled());
        assert_eq!(pool.load().running, 1);
        let refused = {
            let (pool, job) = (pool.clone(), gate.job());
            tokio::spawn(async move { pool.run(job).await })
        };
        wait_for_load(&pool, 1, 1).await;
        assert_eq!(
            pool.run(|| ()).await,
            Err(Overloaded),
            "the slot is still held"
        );

        gate.open();
        assert_eq!(refused.await.unwrap(), Ok(()));
        wait_for_load(&pool, 0, 0).await;
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn a_panicking_job_frees_its_slot_and_panics_in_the_caller() {
        let pool = WorkerPool::new(1, 0);
        let caller = {
            let pool = pool.clone();
            tokio::spawn(async move { pool.run(|| -> u32 { panic!("planner bug") }).await })
        };
        assert!(caller.await.unwrap_err().is_panic());
        wait_for_load(&pool, 0, 0).await;
        assert_eq!(pool.run(|| 1).await, Ok(1));
    }
}
