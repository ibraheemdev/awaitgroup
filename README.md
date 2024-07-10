# `awaitgroup`

[<img alt="crates.io" src="https://img.shields.io/crates/v/awaitgroup?style=for-the-badge" height="25">](https://crates.io/crates/awaitgroup)
[<img alt="github" src="https://img.shields.io/badge/github-awaitgroup-blue?style=for-the-badge" height="25">](https://github.com/ibraheemdev/awaitgroup)
[<img alt="docs.rs" src="https://img.shields.io/docsrs/awaitgroup?style=for-the-badge" height="25">](https://docs.rs/awaitgroup)

 A `WaitGroup` waits for a collection of asynchronous tasks to finish.
 
The main task can create new workers and pass them to each of the tasks it wants to wait for. Each of the tasks calls `done` when
it finishes executing, and the main task can call `wait` to block until all registered workers are done.

 ```rust
 use awaitgroup::WaitGroup;

 #[tokio::main]
 async fn main() {
    let mut wg = WaitGroup::new();

    for _ in 0..5 {
        // Create a new worker.
        let worker = wg.worker();

        tokio::spawn(async {
            // Do some work...

            // This task is done all of its work.
            worker.done();
        });
    }

    // Block until all other tasks have finished their work.
    wg.wait().await;
}
```

See [the documentation](https://docs.rs/awaitgroup) for more details.
