use audeniq_core::{database, operations, storage};
use sqlx::postgres::PgListener;
use std::{sync::Arc, time::Duration};
use tokio::sync::{Notify, Semaphore};

const QUEUES: [&str; 6] = [
    "interactive",
    "qc",
    "rights",
    "distribution",
    "finance",
    "delivery",
];

fn env_usize(name: &str, default: usize, min: usize, max: usize) -> anyhow::Result<usize> {
    let value = std::env::var(name)
        .map(|raw| raw.parse::<usize>())
        .transpose()?
        .unwrap_or(default);
    anyhow::ensure!((min..=max).contains(&value), "{name} must be {min}..={max}");
    Ok(value)
}

async fn listen_for_jobs(database_url: String, wakeup: Arc<Notify>) {
    let mut retry = Duration::from_secs(1);
    loop {
        match PgListener::connect(&database_url).await {
            Ok(mut listener) => {
                if let Err(error) = listener.listen("audeniq_jobs").await {
                    tracing::warn!(%error, "job notification LISTEN failed");
                } else {
                    retry = Duration::from_secs(1);
                    loop {
                        match listener.recv().await {
                            Ok(_) => wakeup.notify_waiters(),
                            Err(error) => {
                                tracing::warn!(%error, "job notification connection lost");
                                break;
                            }
                        }
                    }
                }
            }
            Err(error) => tracing::warn!(%error, "job notification connection unavailable"),
        }
        tokio::time::sleep(retry).await;
        retry = (retry * 2).min(Duration::from_secs(30));
    }
}

enum RunResult {
    Finished(audeniq_core::error::Result<()>),
    LeaseLost,
}

async fn execute_with_heartbeat(
    pool: &sqlx::PgPool,
    store: &Arc<dyn storage::ObjectStore>,
    job: &operations::Job,
    lease_seconds: i32,
) -> RunResult {
    let heartbeat_seconds = (lease_seconds / 3).max(1) as u64;
    let mut ticker = tokio::time::interval(Duration::from_secs(heartbeat_seconds));
    ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    ticker.tick().await;

    let execution = operations::execute(pool, store, job);
    tokio::pin!(execution);
    loop {
        tokio::select! {
            result = &mut execution => return RunResult::Finished(result),
            _ = ticker.tick() => {
                if let Err(error) = operations::heartbeat(pool, job, lease_seconds).await {
                    tracing::warn!(job_id=%job.id, %error, "job lease lost; cancelling stale handler");
                    return RunResult::LeaseLost;
                }
            }
        }
    }
}

async fn wait_for_work(wakeup: &Notify, idle: Duration) {
    tokio::select! {
        _ = wakeup.notified() => {}
        _ = tokio::time::sleep(idle) => {}
    }
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .json()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();

    let cpu_count = std::thread::available_parallelism()
        .map(usize::from)
        .unwrap_or(1);
    let max_in_flight = env_usize("WORKER_MAX_IN_FLIGHT", (cpu_count * 2).clamp(2, 8), 1, 64)?;
    let database_max = env_usize(
        "DATABASE_MAX_CONNECTIONS",
        (max_in_flight + 3).clamp(4, 32),
        2,
        64,
    )?;
    let lease_seconds = env_usize("JOB_LEASE_SECONDS", 300, 30, 3600)? as i32;
    let database_url = std::env::var("DATABASE_URL")?;
    let pool = database::connect(&database_url, database_max as u32).await?;
    let store: Arc<dyn storage::ObjectStore> =
        storage::store_from_env(std::env::var("ALLOW_HTTP_STORAGE").as_deref() == Ok("true"))?;
    let limiter = Arc::new(Semaphore::new(max_in_flight));
    let wakeup = Arc::new(Notify::new());
    let mut tasks = tokio::task::JoinSet::new();

    {
        let wakeup = wakeup.clone();
        tasks.spawn(listen_for_jobs(database_url, wakeup));
    }

    let mut configured_workers = 0usize;
    for queue in QUEUES {
        let key = format!("QUEUE_{}_CONCURRENCY", queue.to_uppercase());
        let count = env_usize(&key, 1, 0, 32)?;
        configured_workers += count;
        for n in 0..count {
            let pool = pool.clone();
            let store = store.clone();
            let limiter = limiter.clone();
            let wakeup = wakeup.clone();
            let name = format!("{}:{queue}:{n}", uuid::Uuid::new_v4());
            tasks.spawn(async move {
                let mut idle = Duration::from_millis(50);
                loop {
                    // Capacity is acquired before claiming, so a queued album
                    // never burns its lease while waiting for CPU or memory.
                    let permit = limiter
                        .clone()
                        .acquire_owned()
                        .await
                        .expect("worker semaphore must remain open");
                    match operations::claim(&pool, queue, &name, lease_seconds).await {
                        Ok(Some(job)) => {
                            idle = Duration::from_millis(50);
                            match execute_with_heartbeat(&pool, &store, &job, lease_seconds).await {
                                RunResult::Finished(Ok(())) | RunResult::LeaseLost => {}
                                RunResult::Finished(Err(error)) => {
                                    let short: String =
                                        format!("{error:?}").chars().take(500).collect();
                                    if let Err(fail_error) = operations::fail(
                                        &pool,
                                        &job,
                                        false,
                                        &format!("INTERNAL_HANDLER_ERROR:{short}"),
                                    )
                                    .await
                                    {
                                        tracing::warn!(job_id=%job.id, queue, %error, %fail_error, "job handler failed");
                                    }
                                }
                            }
                            drop(permit);
                        }
                        Ok(None) => {
                            drop(permit);
                            wait_for_work(&wakeup, idle).await;
                            idle = (idle * 2).min(Duration::from_secs(2));
                        }
                        Err(error) => {
                            drop(permit);
                            tracing::warn!(queue, %error, "queue polling failed");
                            wait_for_work(&wakeup, Duration::from_secs(3)).await;
                        }
                    }
                }
            });
        }
    }
    anyhow::ensure!(
        configured_workers > 0,
        "at least one queue worker is required"
    );

    {
        let pool = pool.clone();
        tasks.spawn(async move {
            loop {
                tokio::time::sleep(Duration::from_secs(900)).await;
                let bucket = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|d| d.as_secs() / 900)
                    .unwrap_or(0);
                let mut tx = match pool.begin().await {
                    Ok(tx) => tx,
                    Err(error) => {
                        tracing::warn!(%error, "reconcile scheduler: db unavailable");
                        continue;
                    }
                };
                let pending: bool = sqlx::query_scalar(
                    "SELECT EXISTS(SELECT 1 FROM operations.jobs WHERE queue='delivery' AND kind='delivery.reconcile' AND status IN ('QUEUED','RUNNING'))",
                )
                .fetch_one(&mut *tx)
                .await
                .unwrap_or(true);
                if !pending {
                    let key = format!("delivery.reconcile:sched:{bucket}");
                    if let Err(error) = operations::enqueue(
                        &mut tx,
                        "delivery",
                        "delivery.reconcile",
                        &serde_json::json!({}),
                        &key,
                        None,
                    )
                    .await
                    {
                        tracing::warn!(%error, "reconcile scheduler: enqueue failed");
                    }
                }
                if let Err(error) = tx.commit().await {
                    tracing::warn!(%error, "reconcile scheduler: commit failed");
                }
            }
        });
    }

    tokio::select! {
        _ = tokio::signal::ctrl_c() => {},
        _ = tasks.join_next() => anyhow::bail!("worker task unexpectedly exited"),
    }
    tasks.abort_all();
    while tasks.join_next().await.is_some() {}
    Ok(())
}
