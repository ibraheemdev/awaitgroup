//! An asynchronous implementation of a `WaitGroup`.
//!
//! A `WaitGroup` waits for a collection of tasks to finish. The main task can create new workers and
//! pass them to each of the tasks it wants to wait for. Then, each of the tasks calls `done` when
//! finished. The main task can call `await` to block until all other tasks have finished.
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
use std::sync::{Arc, Mutex};
use std::task::{Context, Poll, Waker};

/// Wait for a collection of tasks to finish execution.
///
/// Refer to the [crate level documentation](crate) for details.
pub struct WaitGroup {
    inner: Arc<WaitGroupInner>,
}

#[allow(clippy::new_without_default)]
impl WaitGroup {
    /// Creates a new `WaitGroup`.
    pub fn new() -> Self {
        Self {
            inner: Arc::new(WaitGroupInner::new()),
        }
    }

    /// Register a new worker.
    pub fn worker(&self) -> Worker {
        Worker {
            inner: self.inner.clone(),
        }
    }

    /// Wait until all registered workers finish executing.
    pub fn wait(&self) -> WaitGroupFuture<'_> {
        WaitGroupFuture { inner: &self.inner }
    }
}

#[doc(hidden)]
pub struct WaitGroupFuture<'a> {
    inner: &'a Arc<WaitGroupInner>,
}

impl Future for WaitGroupFuture<'_> {
    type Output = ();

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let waker = cx.waker().clone();
        *self.inner.waker.lock().unwrap() = Some(waker);

        match Arc::strong_count(&self.inner) {
            1 => Poll::Ready(()),
            _ => Poll::Pending,
        }
    }
}

struct WaitGroupInner {
    waker: Mutex<Option<Waker>>,
}

impl WaitGroupInner {
    pub fn new() -> Self {
        Self {
            waker: Mutex::new(None),
        }
    }
}

/// A worker registered in a `WaitGroup`.
///
/// Refer to the [crate level documentation](crate) for details.
pub struct Worker {
    inner: Arc<WaitGroupInner>,
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
        Self {
            inner: self.inner.clone(),
        }
    }
}

impl Drop for Worker {
    fn drop(&mut self) {
        // The only references left are ours and the waitgroup's.
        if Arc::strong_count(&self.inner) == 2 {
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
}
