use std::future::Future;
use std::sync::OnceLock;

use tokio::runtime::{Builder, Runtime};

fn service_runtime() -> &'static Runtime {
    static RUNTIME: OnceLock<Runtime> = OnceLock::new();

    RUNTIME.get_or_init(|| {
        Builder::new_multi_thread()
            .worker_threads(2)
            .enable_all()
            .build()
            .expect("failed to build wallet service runtime")
    })
}

pub(crate) fn block_on<F: Future>(future: F) -> F::Output {
    service_runtime().block_on(future)
}

#[cfg(test)]
mod tests {
    use std::sync::mpsc;
    use std::time::Duration;

    use super::*;

    #[test]
    fn detached_tasks_outlive_the_dispatching_runtime() {
        let (completed_tx, completed_rx) = mpsc::channel();

        {
            let caller_runtime = Builder::new_current_thread()
                .enable_all()
                .build()
                .expect("caller runtime should build");
            caller_runtime.block_on(async move {
                let _task = service_runtime().spawn(async move {
                    tokio::time::sleep(Duration::from_millis(25)).await;
                    completed_tx
                        .send(())
                        .expect("test receiver should remain available");
                });
            });
        }

        completed_rx
            .recv_timeout(Duration::from_secs(2))
            .expect("detached task should survive after its caller runtime is dropped");
    }
}
