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

#[path = "worker/adaptive.rs"]
mod adaptive;
#[path = "worker/poll.rs"]
mod poll;
#[path = "worker/unwind.rs"]
mod unwind;

#[global_allocator]
static GLOBAL: audeniq_core::allocator::Allocator = audeniq_core::allocator::Allocator;
/// jemalloc options, read at start-up (crate::allocator). `oversize_threshold:0`:
/// blocks above 8 MiB (Argon2's 19 MiB per password hash) stay in the normal
/// arenas and their decay instead of going back to the OS on every free.
/// `background_thread:true`: purging runs on jemalloc's own threads, not on
/// the request that frees (docs/BACKEND_RESOURCE_PERFORMANCE.md, Allocator).
#[allow(non_upper_case_globals)]
#[unsafe(export_name = "_rjem_malloc_conf")]
pub static malloc_conf: &[u8] = b"oversize_threshold:0,background_thread:true\0";

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
        // Compose passes unset optional values as empty strings.
        Ok(raw) if raw.trim().is_empty() => default,
        Ok(raw) => raw.trim().parse::<usize>()?,
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
    /// The handler panicked: this job fails, the worker keeps running.
    Panicked(String),
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
    // The lease as last confirmed: a transient heartbeat error (pool wait,
    // network blip, lock timeout) is retried on the next tick while the lease
    // is still ours; only a refused renewal or an expired lease cancels.
    let mut lease_until =
        std::time::Instant::now() + Duration::from_secs(lease_seconds.max(1) as u64);
    loop {
        tokio::select! {
            result = &mut execution => return RunResult::Finished(result),
            _ = ticker.tick() => {
                match operations::heartbeat(pool, job, lease_seconds).await {
                    Ok(()) => {
                        lease_until = std::time::Instant::now()
                            + Duration::from_secs(lease_seconds.max(1) as u64);
                    }
                    Err(audeniq_core::error::Error::Conflict) => {
                        tracing::warn!(job_id=%job.id, "job lease lost; cancelling stale handler");
                        return RunResult::LeaseLost;
                    }
                    Err(error) if std::time::Instant::now() + Duration::from_secs(heartbeat_seconds) >= lease_until => {
                        tracing::warn!(job_id=%job.id, %error, "job lease cannot be renewed before it expires; cancelling");
                        return RunResult::LeaseLost;
                    }
                    Err(error) => {
                        tracing::warn!(job_id=%job.id, %error, "job heartbeat failed; retrying on the next tick");
                    }
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

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    audeniq_core::config::antivirus_enabled()?;
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
    let mut scratch = vec![std::env::temp_dir()];
    scratch.extend(std::env::var_os("AUDENIQ_DELIVERY_TMP").map(std::path::PathBuf::from));
    scratch.dedup();
    audeniq_core::temp_sweep::spawn(scratch);
    let store: Arc<dyn storage::ObjectStore> =
        storage::store_from_env(std::env::var("ALLOW_HTTP_STORAGE").as_deref() == Ok("true"))?;
    let drain_seconds = env_usize("WORKER_DRAIN_SECONDS", 30, 0, 3600)? as u64;
    // A DSP send cut off by a deploy leaves an IN_FLIGHT attempt that only
    // reconciliation (often manual) can settle: while one is running, the
    // drain waits this long instead (compose stop_grace_period must exceed it).
    let delivery_drain_seconds = env_usize("WORKER_DELIVERY_DRAIN_SECONDS", 600, 0, 3600)? as u64;
    let sending = Arc::new(std::sync::atomic::AtomicUsize::new(0));
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
        let floor = env_usize(&key, 1, 0, 32)?;
        // Adaptive ceiling (worker/adaptive.rs). QC analyses one track per
        // job, so a burst of uploads is where extra loops pay off: by default
        // it may grow to 2 per CPU (one track downloading while another
        // decodes on a small server; side by side on a bigger one). Other
        // queues stay fixed unless configured.
        let default_max = if queue == "qc" && floor > 0 {
            (cpu_count * 2).clamp(2, 8).max(floor)
        } else {
            floor
        };
        let max_key = format!("QUEUE_{}_MAX_CONCURRENCY", queue.to_uppercase());
        let count = env_usize(&max_key, default_max, floor, 32)?;
        configured_workers += floor;
        let gate = (count > floor).then(|| Arc::new(adaptive::Gate::new(floor, count)));
        if let Some(gate) = &gate {
            tracing::info!(
                queue,
                min = floor,
                max = count,
                "adaptive queue concurrency"
            );
            tasks.spawn(adaptive::control(pool.clone(), queue, gate.clone()));
        }
        for n in 0..count {
            let gate = gate.clone();
            let pool = pool.clone();
            let store = store.clone();
            let limiter = limiter.clone();
            let wakeup = wakeups[queue].clone();
            let shutdown = shutdown.clone();
            let in_flight = in_flight.clone();
            let sending = sending.clone();
            let name = format!("{}:{queue}:{n}", uuid::Uuid::new_v4());
            tasks.spawn(async move {
                let mut idle = Duration::from_millis(50);
                // Expired-lease sweeps run on a timer, not on every poll: an
                // idle poll is then a single round trip.
                let mut last_reclaim: Option<std::time::Instant> = None;
                loop {
                    // Above the adaptive target: claim nothing until it grows.
                    if let Some(gate) = gate.as_ref().filter(|g| n >= g.target()) {
                        if shutdown.load(Ordering::SeqCst) {
                            return;
                        }
                        let grown = gate.grow.notified();
                        tokio::pin!(grown);
                        grown.as_mut().enable();
                        if n >= gate.target() {
                            poll::wait_for_work(grown, adaptive::TICK).await;
                        }
                        continue;
                    }
                    // notify_waiters reaches even an unpolled Notified future.
                    // Subscribe before awaiting capacity/SQL so an enqueue
                    // during an empty claim cannot be lost before sleeping.
                    let notified = wakeup.notified();
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
                            let _running = gate.as_ref().map(|g| g.running());
                            idle = Duration::from_millis(50);
                            in_flight
                                .lock()
                                .expect("in-flight registry")
                                .insert(job.id, job.token);
                            if queue == "delivery" {
                                sending.fetch_add(1, Ordering::SeqCst);
                            }
                            // Every log line of this job (handlers, DSP calls,
                            // analyzers) carries these fields: one release_id
                            // search finds the whole run (migration 0064).
                            let span = job_span(&job, queue);
                            let started = std::time::Instant::now();
                            let outcome = unwind::catch(
                                execute_with_heartbeat(&pool, &store, &job, lease_seconds)
                                    .instrument(span.clone()),
                            )
                            .await
                            .unwrap_or_else(RunResult::Panicked);
                            in_flight.lock().expect("in-flight registry").remove(&job.id);
                            if queue == "delivery" {
                                sending.fetch_sub(1, Ordering::SeqCst);
                            }
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
                                RunResult::Panicked(message) => {
                                    tracing::error!(elapsed_ms, %message, "job handler panicked")
                                }
                            }
                            drop(_entered);
                            let failure = match outcome {
                                RunResult::Finished(Ok(())) | RunResult::LeaseLost => None,
                                RunResult::Finished(Err(error)) => Some(format!(
                                    "INTERNAL_HANDLER_ERROR:{}",
                                    format!("{error:?}").chars().take(500).collect::<String>()
                                )),
                                // Retried with backoff like any handler error,
                                // and dead-lettered after max_attempts.
                                RunResult::Panicked(message) => Some(format!(
                                    "HANDLER_PANIC:{}",
                                    message.chars().take(500).collect::<String>()
                                )),
                            };
                            match failure {
                                None => {}
                                Some(code) => {
                                    if let Err(fail_error) =
                                        operations::fail(&pool, &job, false, &code).await
                                    {
                                        tracing::warn!(job_id=%job.id, queue, %code, %fail_error, "job handler failed");
                                    }
                                }
                            }
                            drop(permit);
                        }
                        Ok(None) => {
                            drop(permit);
                            poll::wait_for_work(notified, idle).await;
                            idle = (idle * 2).min(poll::MAX_IDLE);
                        }
                        Err(error) => {
                            drop(permit);
                            tracing::warn!(queue, %error, "queue polling failed");
                            poll::wait_for_work(notified, Duration::from_secs(3)).await;
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
                // One EXISTS per status so each reads its partial queue index
                // (jobs_ready_claim / jobs_expired_lease), not the whole
                // job history.
                let pending: bool = sqlx::query_scalar(
                    "SELECT EXISTS(SELECT 1 FROM operations.jobs WHERE queue='delivery' AND status='QUEUED' AND kind='delivery.reconcile')
                         OR EXISTS(SELECT 1 FROM operations.jobs WHERE queue='delivery' AND status='RUNNING' AND kind='delivery.reconcile')",
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
    // A loop that ends on its own still drains and hands back the other
    // in-flight jobs below before the process exits with an error.
    let unexpected = tokio::select! {
        _ = tokio::signal::ctrl_c() => false,
        _ = sigterm.recv() => false,
        _ = tasks.join_next() => true,
    };
    if unexpected {
        tracing::error!("worker task unexpectedly exited; draining before exit");
    }
    // Graceful drain (sandbox round 2: SIGTERM killed in-flight jobs, which
    // then sat until their lease expired, up to JOB_LEASE_SECONDS per deploy).
    // Stop claiming, wait for every in-flight job to release its capacity
    // permit, then exit. Jobs still running after the drain window are
    // cancelled and released below (no attempt consumed).
    shutdown.store(true, Ordering::SeqCst);
    tracing::info!(drain_seconds, "shutdown requested: draining in-flight jobs");
    let mut drained = tokio::time::timeout(
        Duration::from_secs(drain_seconds),
        limiter.acquire_many(max_in_flight as u32),
    )
    .await;
    let extra = delivery_drain_seconds.saturating_sub(drain_seconds);
    if drained.is_err() && extra > 0 && sending.load(Ordering::SeqCst) > 0 {
        tracing::info!(
            extra_seconds = extra,
            "DSP send in flight: extending the drain"
        );
        drained = tokio::time::timeout(
            Duration::from_secs(extra),
            limiter.acquire_many(max_in_flight as u32),
        )
        .await;
    }
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
    anyhow::ensure!(!unexpected, "worker task unexpectedly exited");
    Ok(())
}
