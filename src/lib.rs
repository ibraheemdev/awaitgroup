//! [![Documentation](https://img.shields.io/badge/docs-0.4.0-4d76ae?style=for-the-badge)](https://docs.rs/awaitgroup/0.4.0)
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
//! let wg = WaitGroup::new();
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
//! let wg = WaitGroup::new();
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

#[allow(clippy::new_without_default)]
impl WaitGroup {
    /// Creates a new `WaitGroup`.
    pub fn new() -> Self {
        Self {
            inner: Arc::new(Inner::new()),
        }
    }

    /// Register a new worker.
    pub fn worker(&self) -> Worker {
        self.inner.count.fetch_add(1, Ordering::Relaxed);
        Worker {
            inner: self.inner.clone(),
        }
    }

    /// Wait until all registered workers finish executing.
    pub async fn wait(&self) {
        WaitGroupFuture::new(&self.inner).await
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
}

impl Inner {
    pub fn new() -> Self {
        Self {
            count: AtomicUsize::new(0),
            waker: Mutex::new(None),
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
        self.inner.count.fetch_add(1, Ordering::Relaxed);
        Self {
            inner: self.inner.clone(),
        }
    }
}

impl Drop for Worker {
    fn drop(&mut self) {
        let count = self.inner.count.fetch_sub(1, Ordering::Relaxed);
        // We are the last worker
        if count == 1 {
            if let Some(waker) = self.inner.waker.lock().unwrap().take() {
                waker.wake();
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn test_wait_group() {
        let rt = tokio::runtime::Builder::new_current_thread()
            .build()
            .unwrap();

        rt.block_on(async {
            let wg = WaitGroup::new();

            for _ in 0..5 {
                let worker = wg.worker();

                tokio::spawn(async {
                    tokio::time::sleep(Duration::from_secs(5)).await;
                    worker.done();
                });
            }

            wg.wait().await;
        });
    }

    #[test]
    fn test_wait_group_reuse() {
        let rt = tokio::runtime::Builder::new_current_thread()
            .build()
            .unwrap();

        rt.block_on(async {
            let wg = WaitGroup::new();

            for _ in 0..5 {
                let worker = wg.worker();

                tokio::spawn(async {
                    tokio::time::sleep(Duration::from_secs(5)).await;
                    worker.done();
                });
            }

            wg.wait().await;

            let worker = wg.worker();

            tokio::spawn(async {
                tokio::time::sleep(Duration::from_secs(5)).await;
                worker.done();
            });

            wg.wait().await;
        });
    }

    #[test]
    fn test_worker_clone() {
        let rt = tokio::runtime::Builder::new_current_thread()
            .build()
            .unwrap();

        rt.block_on(async {
            let wg = WaitGroup::new();

            for _ in 0..5 {
                let worker = wg.worker();

                tokio::spawn(async {
                    let nested_worker = worker.clone();
                    tokio::spawn(async {
                        nested_worker.done();
                    });
                    worker.done();
                });
            }

            wg.wait().await;
        });
    }
}
