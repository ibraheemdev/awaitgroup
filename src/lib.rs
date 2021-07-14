//! [![Documentation](https://img.shields.io/badge/docs-0.6.0-4d76ae?style=for-the-badge)](https://docs.rs/awaitgroup/0.6.0)
//! [![Version](https://img.shields.io/crates/v/awaitgroup?style=for-the-badge)](https://crates.io/crates/awaitgroup)
//! [![License](https://img.shields.io/crates/l/awaitgroup?style=for-the-badge)](https://crates.io/crates/awaitgroup)
//! [![Actions](https://img.shields.io/github/workflow/status/ibraheemdev/awaitgroup/Rust/master?style=for-the-badge)](https://github.com/ibraheemdev/awaitgroup/actions)
//!
//! An asynchronous implementation of a `WaitGroup`.
//!
//! A `WaitGroup` waits for a collection of tasks to finish. The main task can create new workers and
//! pass them to each of the tasks it wants to wait for. Then, each of the tasks calls `done` when
//! it finishes executing. The main task can call `wait` to block until all registered workers are done.
//!
//! # Examples
//!
//! ```rust
//! # fn main() {
//! # let rt = tokio::runtime::Builder::new_current_thread().build().unwrap();
//! # rt.block_on(async {
//! use awaitgroup::WaitGroup;
//!
//! let mut wg = WaitGroup::new();
//!
//! for _ in 0..5 {
//!     // Create a new worker.
//!     let worker = wg.worker();
//!
//!     tokio::spawn(async {
//!         // Do some work...
//!
//!         // This task is done all of its work.
//!         worker.done();
//!     });
//! }
//!
//! // Block until all other tasks have finished their work.
//! wg.wait().await;
//! # });
//! # }
//! ```
//!
//! A `WaitGroup` can be re-used and awaited multiple times.
//! ```rust
//! # use awaitgroup::WaitGroup;
//! # fn main() {
//! # let rt = tokio::runtime::Builder::new_current_thread().build().unwrap();
//! # rt.block_on(async {
//! let mut wg = WaitGroup::new();
//!
//! let worker = wg.worker();
//!
//! tokio::spawn(async {
//!     // Do work...
//!     worker.done();
//! });
//!
//! // Wait for tasks to finish
//! wg.wait().await;
//!
//! // Re-use wait group
//! let worker = wg.worker();
//!
//! tokio::spawn(async {
//!     // Do more work...
//!     worker.done();
//! });
//!
//! wg.wait().await;
//! # });
//! # }
//! ```
#![deny(missing_debug_implementations, rust_2018_idioms)]
use std::fmt;
use std::future::Future;
use std::pin::Pin;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::task::{Context, Poll, Waker};

/// Wait for a collection of tasks to finish execution.
///
/// Refer to the [crate level documentation](crate) for details.
pub struct WaitGroup {
    inner: Arc<Inner>,
}

impl Default for WaitGroup {
    fn default() -> Self {
        Self {
            inner: Arc::new(Inner::new()),
        }
    }
}

impl WaitGroup {
    /// Creates a new `WaitGroup`.
    pub fn new() -> Self {
        Self::default()
    }

    /// Register a new worker.
    pub fn worker(&self) -> Worker {
        self.inner.inc_count();
        Worker {
            inner: self.inner.clone(),
        }
    }

    /// No active registered workers. Used for testing.
    fn has_no_worker(&self) -> bool {
        let count = self.inner.count.load(Ordering::Relaxed);
        count == 0
    }

    /// Wait until all registered workers finish executing.
    pub async fn wait(&mut self) {
        WaitGroupFuture::new(&self.inner).await
    }

    /// Create a child `WaitGroup`.
    pub fn child(&self) -> Self {
        Self {
            inner: Arc::new(Inner::with_parent(&self.inner)),
        }
    }
}

struct WaitGroupFuture<'a> {
    inner: &'a Arc<Inner>,
}

impl<'a> WaitGroupFuture<'a> {
    fn new(inner: &'a Arc<Inner>) -> Self {
        Self { inner }
    }
}

impl Future for WaitGroupFuture<'_> {
    type Output = ();

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let waker = cx.waker().clone();
        *self.inner.waker.lock().unwrap() = Some(waker);

        match self.inner.count.load(Ordering::Relaxed) {
            0 => Poll::Ready(()),
            _ => Poll::Pending,
        }
    }
}

struct Inner {
    waker: Mutex<Option<Waker>>,
    count: AtomicUsize,
    parent: Option<Arc<Inner>>,
}

impl Inner {
    fn new() -> Self {
        Self {
            count: AtomicUsize::new(0),
            waker: Mutex::new(None),
            parent: None,
        }
    }

    fn with_parent(parent: &Arc<Inner>) -> Self {
        Self {
            count: AtomicUsize::new(0),
            waker: Mutex::new(None),
            parent: Some(parent.clone()),
        }
    }

    /// Increment the worker count of current `Inner` and its parent recursively.
    fn inc_count(&self) {
        self.count.fetch_add(1, Ordering::Relaxed);
        if let Some(parent) = &self.parent {
            parent.inc_count();
        }
    }

    /// Decrement the worker count of current `Inner` and its parent recursively.
    /// If the worker count of `Inner` reaches 0 during recursion, wake the waker in `Inner`.
    fn dec_count(&self) {
        let count = self.count.fetch_sub(1, Ordering::Relaxed);
        // last worker
        if count == 1 {
            if let Some(waker) = self.waker.lock().unwrap().take() {
                waker.wake();
            }
        }
        if let Some(parent) = &self.parent {
            parent.dec_count();
        }
    }
}

/// A worker registered in a `WaitGroup`.
///
/// Refer to the [crate level documentation](crate) for details.
pub struct Worker {
    inner: Arc<Inner>,
}

impl Worker {
    /// Notify the `WaitGroup` that this worker has finished execution.
    pub fn done(self) {
        drop(self)
    }
}

impl Clone for Worker {
    /// Cloning a worker increments the primary reference count and returns a new worker for use in
    /// another task.
    fn clone(&self) -> Self {
        self.inner.inc_count();
        Self {
            inner: self.inner.clone(),
        }
    }
}

impl Drop for Worker {
    fn drop(&mut self) {
        self.inner.dec_count();
    }
}

impl fmt::Debug for WaitGroup {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let count = self.inner.count.load(Ordering::Relaxed);
        f.debug_struct("WaitGroup").field("count", &count).finish()
    }
}

impl fmt::Debug for Worker {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let count = self.inner.count.load(Ordering::Relaxed);
        f.debug_struct("Worker").field("count", &count).finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{Duration, Instant};

    /// macro to assert that left is greater than right.
    macro_rules! assert_gt {
        ($left:expr, $right:expr) => ({
            let (left, right) = (&($left), &($right));
            if !(left > right) {
                panic!("assertion failed: `(left > right)`\n  left: `{:?}`,\n right: `{:?}`",
                       left, right);
            }
        });
        ($left:expr, $right:expr, ) => ({
            assert_gt!($left, $right);
        });
        ($left:expr, $right:expr, $($msg_args:tt)+) => ({
            let (left, right) = (&($left), &($right));
            if !(left > right) {
                panic!("assertion failed: `(left > right)`\n  left: `{:?}`,\n right: `{:?}`: {}",
                       left, right, format_args!($($msg_args)+));
            }
        })
    }

    /// macro to assert that the duration from `start` to now is longer than `elasped`.  
    macro_rules! assert_elasped_gt {
        ($start: expr, $elasped: expr) => ({
            assert_gt!(Instant::now() - $start, $elasped);
        });
        ($start: expr, $elasped: expr, ) => ({
            assert_elasped_gt!($start, $elapsed);
        });
        ($start: expr, $elasped: expr, $($msg_args:tt)+) => ({
            assert_elasped_gt!($start, $elasped, $($msg_args:tt)+);
        })
    }

    #[tokio::test]
    async fn test_wait_group() {
        let mut wg = WaitGroup::new();
        let start = Instant::now();

        for i in 0..5 {
            let worker = wg.worker();
            tokio::spawn(async move {
                tokio::time::sleep(Duration::from_millis(i * 100)).await;
                worker.done();
            });
        }

        wg.wait().await;
        assert_elasped_gt!(start, Duration::from_millis(400));
    }

    #[tokio::test]
    async fn test_wait_group_reuse() {
        let mut wg = WaitGroup::new();
        let start = Instant::now();

        for i in 0..5 {
            let worker = wg.worker();

            tokio::spawn(async move {
                tokio::time::sleep(Duration::from_millis(i * 100)).await;
                worker.done();
            });
        }

        wg.wait().await;
        assert_elasped_gt!(start, Duration::from_millis(400));

        let worker = wg.worker();
        let start = Instant::now();
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(500)).await;
            worker.done();
        });

        wg.wait().await;
        assert_elasped_gt!(start, Duration::from_millis(500));
    }

    #[tokio::test]
    async fn test_worker_clone() {
        let mut wg = WaitGroup::new();
        let start = Instant::now();

        for i in 0..5 {
            let worker = wg.worker();

            tokio::spawn(async move {
                let nested_worker = worker.clone();
                tokio::spawn(async move {
                    tokio::time::sleep(Duration::from_millis(i * 100)).await;
                    nested_worker.done();
                });
                worker.done();
            });
        }

        wg.wait().await;
        assert_elasped_gt!(start, Duration::from_millis(400));
    }

    #[tokio::test]
    async fn test_child_wait_group() {
        let mut wg = WaitGroup::new();
        let worker = wg.worker();

        let mut child_wg = wg.child();
        let start = Instant::now();
        for _ in 0..5 {
            let child_worker = child_wg.worker();

            tokio::spawn(async move {
                tokio::time::sleep(Duration::from_millis(200)).await;
                child_worker.done();
            });
        }

        child_wg.wait().await;
        assert_elasped_gt!(start, Duration::from_millis(200));
        assert_eq!(wg.has_no_worker(), false);

        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(100)).await;
            worker.done();
        });
        wg.wait().await;
        assert_elasped_gt!(start, Duration::from_millis(300));

        let child_worker = child_wg.worker();
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(100)).await;
            child_worker.done();
        });
        child_wg.wait().await;
        wg.wait().await;
        assert_elasped_gt!(start, Duration::from_millis(400));
    }
}
