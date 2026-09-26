use audeniq_core::{database, operations, storage};
use sqlx::postgres::PgListener;
use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};
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
    let value = match std::env::var(name) {
        Ok(raw) => raw.parse::<usize>()?,
        Err(std::env::VarError::NotPresent) => default,
        Err(error) => return Err(error.into()),
    };
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
    heartbeat_pool: &sqlx::PgPool,
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
                match operations::heartbeat(heartbeat_pool, job, lease_seconds).await {
                    Ok(()) => {}
                    // Fenced out: another worker owns the job now.
                    Err(audeniq_core::error::Error::LeaseLost) => {
                        tracing::warn!(job_id=%job.id, kind=%job.kind, "job lease lost; cancelling stale handler");
                        return RunResult::LeaseLost;
                    }
                    // Database briefly unreachable: the lease is still valid
                    // (heartbeats run every lease/3), so keep working and
                    // retry on the next tick instead of throwing away a
                    // multi-minute QC analysis. If the lease really expires,
                    // the next heartbeat is fenced out (Conflict) above.
                    Err(error) => {
                        tracing::warn!(job_id=%job.id, %error, "job heartbeat failed; retrying next tick");
                    }
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

    // SIGXFSZ (file-size rlimit hit while writing a temp file) would kill the
    // whole worker by default (sandbox round 2). Handling it turns the write
    // into an EFBIG error, which fails/retries just that job. ENOSPC is
    // already an ordinary write error at job level.
    let mut xfsz =
        tokio::signal::unix::signal(tokio::signal::unix::SignalKind::from_raw(libc::SIGXFSZ))?;
    tokio::spawn(async move {
        while xfsz.recv().await.is_some() {
            tracing::warn!(
                "SIGXFSZ: a temp file hit the file-size limit; the job write fails with EFBIG"
            );
        }
    });

    let cpu_count = std::thread::available_parallelism()
        .map(usize::from)
        .unwrap_or(1);
    let max_in_flight = env_usize("WORKER_MAX_IN_FLIGHT", (cpu_count * 2).clamp(2, 8), 1, 64)?;
    let database_max = env_usize(
        "DATABASE_MAX_CONNECTIONS",
        // A Stage 1 job runs several asset analyses at once, each using
        // connections of its own: size for that, not just one per job.
        (max_in_flight + cpu_count * 2 + 3).clamp(4, 48),
        2,
        64,
    )?;
    let lease_seconds = env_usize("JOB_LEASE_SECONDS", 300, 30, 3600)? as i32;
    operations::set_job_lease_seconds(lease_seconds);
    let database_url = std::env::var("DATABASE_URL")?;
    let pool = database::connect(&database_url, database_max as u32).await?;
    // Lease heartbeats get their own connections: a Stage 1 run analysing
    // hundreds of assets can keep the main pool busy for longer than the
    // 5 s acquire timeout, and a starved heartbeat used to lose the lease
    // of the very job that was working.
    let heartbeat_pool = database::connect(&database_url, 2).await?;
    let store: Arc<dyn storage::ObjectStore> =
        storage::store_from_env(std::env::var("ALLOW_HTTP_STORAGE").as_deref() == Ok("true"))?;
    let drain_seconds = env_usize("WORKER_DRAIN_SECONDS", 30, 0, 3600)? as u64;
    let limiter = Arc::new(Semaphore::new(max_in_flight));
    // Set on SIGTERM/SIGINT: stop claiming, let in-flight jobs finish.
    let shutdown = Arc::new(AtomicBool::new(false));
    // Claimed jobs currently executing: (job id, lock token). On shutdown,
    // anything still here after the drain window is released immediately.
    let in_flight: Arc<std::sync::Mutex<std::collections::HashMap<uuid::Uuid, uuid::Uuid>>> =
        Arc::default();
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
            let heartbeat_pool = heartbeat_pool.clone();
            let store = store.clone();
            let limiter = limiter.clone();
            let wakeup = wakeup.clone();
            let shutdown = shutdown.clone();
            let in_flight = in_flight.clone();
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
                    if shutdown.load(Ordering::SeqCst) {
                        drop(permit);
                        return;
                    }
                    match operations::claim(&pool, queue, &name, lease_seconds).await {
                        Ok(Some(job)) => {
                            idle = Duration::from_millis(50);
                            in_flight
                                .lock()
                                .expect("in-flight registry")
                                .insert(job.id, job.token);
                            let outcome =
                                execute_with_heartbeat(&pool, &heartbeat_pool, &store, &job, lease_seconds)
                                    .await;
                            in_flight.lock().expect("in-flight registry").remove(&job.id);
                            match outcome {
                                RunResult::Finished(Ok(())) | RunResult::LeaseLost => {}
                                // Already logged with its reason and the
                                // result it could not record; calling fail()
                                // would only be fenced out again.
                                RunResult::Finished(Err(
                                    audeniq_core::error::Error::LeaseLost,
                                )) => {}
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

    let mut sigterm = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())?;
    tokio::select! {
        _ = tokio::signal::ctrl_c() => {},
        _ = sigterm.recv() => {},
        _ = tasks.join_next() => anyhow::bail!("worker task unexpectedly exited"),
    }
    // Graceful drain (sandbox round 2: SIGTERM killed in-flight jobs, which
    // then sat until their lease expired, up to JOB_LEASE_SECONDS per deploy).
    // Stop claiming, wait for every in-flight job to release its capacity
    // permit, then exit. Jobs still running after the drain window are
    // cancelled and released below (no attempt consumed).
    shutdown.store(true, Ordering::SeqCst);
    tracing::info!(drain_seconds, "shutdown requested: draining in-flight jobs");
    let drained = tokio::time::timeout(
        Duration::from_secs(drain_seconds),
        limiter.acquire_many(max_in_flight as u32),
    )
    .await;
    tasks.abort_all();
    while tasks.join_next().await.is_some() {}
    if drained.is_err() {
        // Stop the unfinished handlers first (above), then hand their jobs
        // back without burning an attempt so another worker picks them up
        // now rather than after JOB_LEASE_SECONDS.
        let pending: Vec<(uuid::Uuid, uuid::Uuid)> = in_flight
            .lock()
            .expect("in-flight registry")
            .drain()
            .collect();
        for (id, token) in pending {
            match operations::release_lease(&pool, id, token).await {
                Ok(true) => tracing::info!(job_id=%id, "released unfinished job on shutdown"),
                Ok(false) => {}
                Err(error) => tracing::warn!(job_id=%id, %error, "could not release job lease"),
            }
        }
    }
    Ok(())
}
