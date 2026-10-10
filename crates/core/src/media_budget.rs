//! Share media capacity across jobs, not separately for every album.
use std::sync::{Arc, OnceLock};
use tokio::sync::{OwnedSemaphorePermit, Semaphore};

fn bounded_parallelism(raw: Option<&str>, cores: usize) -> usize {
    raw.and_then(|s| s.parse::<usize>().ok())
        .filter(|n| *n > 0)
        .unwrap_or(cores)
        .clamp(1, 8)
}

struct Budget {
    capacity: usize,
    slots: Arc<Semaphore>,
}

fn limiter() -> &'static Budget {
    static LIMITER: OnceLock<Budget> = OnceLock::new();
    LIMITER.get_or_init(|| {
        let cores = std::thread::available_parallelism()
            .map(usize::from)
            .unwrap_or(1);
        let capacity = bounded_parallelism(
            std::env::var("AUDENIQ_QC_ASSET_PARALLELISM")
                .ok()
                .as_deref(),
            cores,
        );
        Budget {
            capacity,
            slots: Arc::new(Semaphore::new(capacity)),
        }
    })
}

pub(crate) fn parallelism() -> usize {
    limiter().capacity
}

pub(crate) async fn acquire() -> Result<OwnedSemaphorePermit, &'static str> {
    limiter()
        .slots
        .clone()
        .acquire_owned()
        .await
        .map_err(|_| "media capacity unavailable")
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn parallelism_keeps_cpu_capacity_with_a_hard_upper_bound() {
        assert_eq!(bounded_parallelism(None, 1), 1);
        assert_eq!(bounded_parallelism(None, 4), 4);
        assert_eq!(bounded_parallelism(Some("2"), 4), 2);
        assert_eq!(bounded_parallelism(Some("1000000"), 4), 8);
        assert_eq!(bounded_parallelism(Some("0"), 4), 4);
    }

    #[tokio::test]
    async fn aborting_a_started_blocking_task_keeps_its_capacity_until_exit() {
        let slots = Arc::new(Semaphore::new(1));
        let permit = slots.clone().acquire_owned().await.unwrap();
        let (started_tx, started_rx) = tokio::sync::oneshot::channel();
        let (exit_tx, exit_rx) = std::sync::mpsc::channel();
        let task = tokio::task::spawn_blocking(move || {
            let _capacity = permit;
            started_tx.send(()).unwrap();
            exit_rx.recv().unwrap();
        });
        started_rx.await.unwrap();
        task.abort();
        assert!(slots.try_acquire().is_err());
        exit_tx.send(()).unwrap();
        let _ = task.await;
        assert_eq!(slots.available_permits(), 1);
    }
}
