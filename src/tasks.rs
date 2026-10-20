use std::future::Future;
use std::time::Duration;

use tokio_util::sync::CancellationToken;
use tokio_util::task::TaskTracker;

#[derive(Clone, Default)]
pub struct Tasks {
    token: CancellationToken,
    tracker: TaskTracker,
}

impl Tasks {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn token(&self) -> CancellationToken {
        self.token.clone()
    }

    pub(crate) fn spawn<F>(&self, f: F)
    where
        F: Future<Output = ()> + Send + 'static,
    {
        self.tracker.spawn(f);
    }

    pub(crate) fn spawn_until_cancelled<F>(&self, f: F)
    where
        F: Future<Output = ()> + Send + 'static,
    {
        let token = self.token.clone();
        self.tracker.spawn(async move {
            tokio::select! {
                () = token.cancelled() => {}
                () = f => {}
            }
        });
    }

    #[doc(hidden)]
    pub async fn drain(&self) {
        self.tracker.close();
        self.tracker.wait().await;
        self.tracker.reopen();
    }

    pub(crate) async fn shutdown(&self, grace: Duration) {
        self.token.cancel();
        self.tracker.close();
        if tokio::time::timeout(grace, self.tracker.wait())
            .await
            .is_err()
        {
            tracing::warn!(
                "{} background task(s) still running after {}s; exiting anyway",
                self.tracker.len(),
                grace.as_secs()
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn shutdown_cancels_and_waits_for_tracked_tasks() {
        let tasks = Tasks::new();
        let (tx, rx) = tokio::sync::oneshot::channel();
        let token = tasks.token();
        tasks.spawn(async move {
            token.cancelled().await;
            let _ = tx.send(());
        });
        tasks.spawn_until_cancelled(std::future::pending());
        tokio::time::timeout(
            Duration::from_secs(5),
            tasks.shutdown(Duration::from_secs(5)),
        )
        .await
        .unwrap();
        assert!(rx.await.is_ok());
        assert!(tasks.tracker.is_empty());
    }

    #[tokio::test]
    async fn drain_waits_without_cancelling_and_keeps_accepting_work() {
        let tasks = Tasks::new();
        let (tx, rx) = tokio::sync::oneshot::channel();
        tasks.spawn(async move {
            tokio::time::sleep(Duration::from_millis(20)).await;
            let _ = tx.send(());
        });
        tasks.drain().await;
        assert!(rx.await.is_ok());
        assert!(!tasks.token().is_cancelled());

        let (tx, rx) = tokio::sync::oneshot::channel();
        tasks.spawn(async move {
            let _ = tx.send(());
        });
        tasks.drain().await;
        assert!(rx.await.is_ok());
    }
}
