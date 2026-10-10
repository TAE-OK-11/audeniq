use std::{future::Future, time::Duration};

// LISTEN/NOTIFY wakes new jobs promptly. Polling remains a fallback for
// disconnected listeners, scheduled retries, and expired leases.
pub(super) const MAX_IDLE: Duration = Duration::from_secs(5);

pub(super) async fn wait_for_work(wakeup: impl Future<Output = ()>, idle: Duration) {
    tokio::select! {
        _ = wakeup => {}
        _ = tokio::time::sleep(idle) => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::sync::Notify;

    #[tokio::test]
    async fn notification_during_claim_is_observed_before_waiting() {
        let wakeup = Notify::new();
        // Production creates this future before its capacity/DB awaits.
        let notified = wakeup.notified();
        wakeup.notify_waiters();
        tokio::time::timeout(Duration::from_secs(1), wait_for_work(notified, MAX_IDLE))
            .await
            .expect("notification must not be lost between claim and wait");
    }

    #[tokio::test]
    async fn all_workers_of_the_queue_observe_notification() {
        let wakeup = Notify::new();
        let first = wakeup.notified();
        let second = wakeup.notified();
        wakeup.notify_waiters();
        tokio::time::timeout(Duration::from_secs(1), async {
            tokio::join!(
                wait_for_work(first, MAX_IDLE),
                wait_for_work(second, MAX_IDLE)
            );
        })
        .await
        .expect("both queue workers must wake");
    }

    #[tokio::test]
    async fn disconnected_listener_still_polls() {
        let wakeup = Notify::new();
        let started = tokio::time::Instant::now();
        let idle = Duration::from_millis(5);
        tokio::time::timeout(
            Duration::from_secs(1),
            wait_for_work(wakeup.notified(), idle),
        )
        .await
        .expect("poll fallback must finish without a notification");
        assert!(started.elapsed() >= idle);
    }
}
