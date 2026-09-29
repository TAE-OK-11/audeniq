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
use tracing::Instrument;

#[global_allocator]
static GLOBAL: mimalloc::MiMalloc = mimalloc::MiMalloc;

const QUEUES: [&str; 6] = [
    "interactive",
    "qc",
    "rights",
    "distribution",
    "finance",
    "delivery",
];

/// How often each queue loop sweeps expired leases back to QUEUED. Leases are
/// at least 30 s (JOB_LEASE_SECONDS), so this adds at most a few seconds to
/// the recovery of a crashed worker's job.
const RECLAIM_EVERY: Duration = Duration::from_secs(5);

fn env_usize(name: &str, default: usize, min: usize, max: usize) -> anyhow::Result<usize> {
    let value = match std::env::var(name) {
        Ok(raw) => raw.parse::<usize>()?,
        Err(std::env::VarError::NotPresent) => default,
        Err(error) => return Err(error.into()),
    };
    anyhow::ensure!((min..=max).contains(&value), "{name} must be {min}..={max}");
    Ok(value)
}

/// One wakeup per queue: a NOTIFY (payload = queue name) wakes only that
/// queue's workers instead of every idle worker in the process.
type Wakeups = Arc<std::collections::HashMap<&'static str, Arc<Notify>>>;

async fn listen_for_jobs(database_url: String, wakeups: Wakeups) {
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
                            Ok(n) => match wakeups.get(n.payload()) {
                                Some(w) => w.notify_waiters(),
                                None => wakeups.values().for_each(|w| w.notify_waiters()),
                            },
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
    /// Ran past the kind's `timeout_secs` (job_policies): the handler was
    /// dropped, which closes its partner connections and rolls back its
    /// open transaction, and the job is retried.
    TimedOut,
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
    // No policy timeout: the heartbeat below keeps the lease alive for as
    // long as the handler runs (CPU analyzers bound themselves).
    let limit = job
        .timeout_secs
        .map(|s| Duration::from_secs(s.max(1) as u64))
        .unwrap_or(Duration::MAX);
    let deadline = tokio::time::sleep(limit.min(Duration::from_secs(100 * 365 * 86_400)));
    tokio::pin!(deadline);
    loop {
        tokio::select! {
            result = &mut execution => return RunResult::Finished(result),
            _ = &mut deadline => return RunResult::TimedOut,
            _ = ticker.tick() => {
                if let Err(error) = operations::heartbeat(pool, job, lease_seconds).await {
                    tracing::warn!(job_id=%job.id, %error, "job lease lost; cancelling stale handler");
                    return RunResult::LeaseLost;
                }
            }
        }
    }
}

/// Tracing span for one job run: the correlation fields every log line of
/// the run carries.
fn job_span(job: &operations::Job, queue: &'static str) -> tracing::Span {
    let span = tracing::info_span!(
        "job",
        job_id = %job.id,
        kind = %job.kind,
        queue,
        attempt = job.attempts,
        release_id = tracing::field::Empty,
    );
    if let Some(release) = job.release_id {
        span.record("release_id", tracing::field::display(release));
    }
    span
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
        (max_in_flight + 3).clamp(4, 32),
        2,
        64,
    )?;
    let lease_seconds = env_usize("JOB_LEASE_SECONDS", 300, 30, 3600)? as i32;
    operations::set_job_lease_seconds(lease_seconds);
    let database_url = std::env::var("DATABASE_URL")?;
    let pool = database::connect(&database_url, database_max as u32).await?;
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
    let wakeups: Wakeups = Arc::new(QUEUES.map(|q| (q, Arc::new(Notify::new()))).into());
    let mut tasks = tokio::task::JoinSet::new();

    {
        // LISTEN needs a session: behind PgBouncer (transaction pooling) it
        // must use a direct connection, or notifications silently never
        // arrive and the worker falls back to polling.
        let listen_url = std::env::var("DATABASE_LISTEN_URL").unwrap_or(database_url);
        tasks.spawn(listen_for_jobs(listen_url, wakeups.clone()));
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
            let wakeup = wakeups[queue].clone();
            let shutdown = shutdown.clone();
            let in_flight = in_flight.clone();
            let name = format!("{}:{queue}:{n}", uuid::Uuid::new_v4());
            tasks.spawn(async move {
                let mut idle = Duration::from_millis(50);
                // Expired-lease sweeps run on a timer, not on every poll: an
                // idle poll is then a single round trip.
                let mut last_reclaim: Option<std::time::Instant> = None;
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
                    let reclaim = last_reclaim.is_none_or(|t| t.elapsed() >= RECLAIM_EVERY);
                    let claimed =
                        operations::claim_with(&pool, queue, &name, lease_seconds, reclaim).await;
                    if reclaim && claimed.is_ok() {
                        last_reclaim = Some(std::time::Instant::now());
                    }
                    match claimed {
                        Ok(Some(job)) => {
                            idle = Duration::from_millis(50);
                            in_flight
                                .lock()
                                .expect("in-flight registry")
                                .insert(job.id, job.token);
                            // Every log line of this job (handlers, DSP calls,
                            // analyzers) carries these fields: one release_id
                            // search finds the whole run (migration 0064).
                            let span = job_span(&job, queue);
                            let started = std::time::Instant::now();
                            let outcome = execute_with_heartbeat(&pool, &store, &job, lease_seconds)
                                .instrument(span.clone())
                                .await;
                            in_flight.lock().expect("in-flight registry").remove(&job.id);
                            let _entered = span.enter();
                            let elapsed_ms = started.elapsed().as_millis() as u64;
                            match &outcome {
                                RunResult::Finished(Ok(())) => {
                                    tracing::info!(elapsed_ms, "job finished")
                                }
                                RunResult::LeaseLost => {
                                    tracing::warn!(elapsed_ms, "job lease lost")
                                }
                                RunResult::Finished(Err(error)) => {
                                    tracing::warn!(elapsed_ms, %error, "job handler error")
                                }
                                RunResult::TimedOut => {
                                    tracing::warn!(elapsed_ms, "job timed out")
                                }
                            }
                            drop(_entered);
                            // Retry, or dead-letter and surface on the release
                            // (pipeline kinds): a failed last attempt must not
                            // leave the release in a running state.
                            let failed = match outcome {
                                RunResult::Finished(Ok(())) | RunResult::LeaseLost => None,
                                RunResult::Finished(Err(error)) => {
                                    let short: String =
                                        format!("{error:?}").chars().take(500).collect();
                                    Some((
                                        format!("INTERNAL_HANDLER_ERROR:{short}"),
                                        operations::is_permanent(&error),
                                    ))
                                }
                                RunResult::TimedOut => Some(("JOB_TIMEOUT".to_string(), false)),
                            };
                            if let Some((code, permanent)) = failed
                                && let Err(fail_error) =
                                    operations::fail_handler(&pool, &job, &code, permanent).await
                            {
                                tracing::warn!(job_id=%job.id, queue, code, %fail_error, "recording the job failure failed");
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
