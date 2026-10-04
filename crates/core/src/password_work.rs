use tokio::sync::OwnedSemaphorePermit;

/// Keep the CPU/RAM slot inside the blocking task: cancelling its async
/// caller cannot stop a started Argon2 operation. Returning the permit lets
/// password changes reuse their slot for verification and hashing.
pub(crate) async fn run<T: Send + 'static>(
    permit: OwnedSemaphorePermit,
    work: impl FnOnce() -> T + Send + 'static,
) -> Result<(OwnedSemaphorePermit, T), tokio::task::JoinError> {
    tokio::task::spawn_blocking(move || {
        let result = work();
        (permit, result)
    })
    .await
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{sync::Arc, time::Duration};
    use tokio::sync::{Semaphore, oneshot};

    #[tokio::test]
    async fn cancelled_caller_keeps_slot_until_blocking_work_finishes() {
        let slots = Arc::new(Semaphore::new(1));
        let permit = slots.clone().acquire_owned().await.unwrap();
        let (started_tx, started_rx) = oneshot::channel();
        let (finish_tx, finish_rx) = std::sync::mpsc::channel();
        let task = tokio::spawn(run(permit, move || {
            started_tx.send(()).unwrap();
            finish_rx.recv_timeout(Duration::from_secs(5)).unwrap();
        }));
        started_rx.await.unwrap();
        task.abort();
        assert!(task.await.unwrap_err().is_cancelled());
        let still_held = slots.clone().try_acquire_owned().is_err();
        finish_tx.send(()).unwrap();
        assert!(
            still_held,
            "cancelled caller released a running task's slot"
        );
        let permit = tokio::time::timeout(Duration::from_secs(5), slots.acquire())
            .await
            .expect("finished work releases its slot")
            .unwrap();
        drop(permit);
    }

    #[tokio::test]
    async fn consecutive_operations_reuse_one_slot() {
        let slots = Arc::new(Semaphore::new(1));
        let permit = slots.clone().acquire_owned().await.unwrap();
        let (permit, verified) = run(permit, || true).await.unwrap();
        assert!(verified);
        assert!(slots.clone().try_acquire_owned().is_err());
        let (permit, hashed) = run(permit, || "hash").await.unwrap();
        assert_eq!(hashed, "hash");
        assert_eq!(slots.available_permits(), 0);
        drop(permit);
        assert_eq!(slots.available_permits(), 1);
    }

    #[tokio::test]
    async fn panicking_work_releases_slot() {
        let slots = Arc::new(Semaphore::new(1));
        let permit = slots.clone().acquire_owned().await.unwrap();
        assert!(
            run(permit, || panic!("failed work"))
                .await
                .unwrap_err()
                .is_panic()
        );
        assert_eq!(slots.available_permits(), 1);
    }
}
