use audeniq_core::{api, config::Config, database, storage};
use std::{sync::Arc, time::Duration};

fn env_usize(name: &str, default: usize, min: usize, max: usize) -> anyhow::Result<usize> {
    let value = match std::env::var(name) {
        Ok(raw) => raw.parse::<usize>()?,
        Err(std::env::VarError::NotPresent) => default,
        Err(error) => return Err(error.into()),
    };
    anyhow::ensure!((min..=max).contains(&value), "{name} must be {min}..={max}");
    Ok(value)
}

/// Resolves on SIGTERM (container stop / orchestrator) or Ctrl-C. Before,
/// only Ctrl-C triggered the graceful path, so every deploy killed in-flight
/// requests (including upload completions) instead of draining them.
async fn shutdown_signal() {
    let terminate = async {
        match tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate()) {
            Ok(mut s) => {
                s.recv().await;
            }
            Err(_) => std::future::pending::<()>().await,
        }
    };
    tokio::select! {
        _ = tokio::signal::ctrl_c() => {},
        _ = terminate => {},
    }
    tracing::info!("shutdown requested: draining in-flight requests");
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .json()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();
    let config = Config::from_env()?;
    // Sized for concurrent uploads and bulk catalog edits. The old fixed 6
    // was exhausted by a handful of simultaneous requests.
    let database_max = env_usize("DATABASE_MAX_CONNECTIONS", 20, 2, 200)?;
    // Argon2id holds ~19 MiB per concurrent hash.
    let password_slots = env_usize("PASSWORD_HASH_CONCURRENCY", 2, 1, 16)?;
    let pool = database::connect(&config.database_url, database_max as u32).await?;
    let storage = storage::store_from_env(!config.secure_cookie)?;
    let listener = tokio::net::TcpListener::bind(&config.bind).await?;
    let mut state = api::AppState::new(pool.clone(), config, storage).await?;
    state.password_slots = Arc::new(tokio::sync::Semaphore::new(password_slots));
    tokio::spawn(housekeeping(pool));
    axum::serve(listener, api::router(state))
        .with_graceful_shutdown(shutdown_signal())
        .await?;
    Ok(())
}

/// Periodic cleanup of rows that only grow: rate-limit buckets whose
/// 15-minute window ended long ago. Failures are logged and retried next
/// round; they never affect request handling.
async fn housekeeping(pool: sqlx::PgPool) {
    let mut tick = tokio::time::interval(Duration::from_secs(3600));
    tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    loop {
        tick.tick().await;
        match audeniq_core::auth::purge_expired_rate_limits(&pool).await {
            Ok(n) if n > 0 => tracing::info!(deleted = n, "expired rate-limit buckets purged"),
            Ok(_) => {}
            Err(error) => tracing::warn!(?error, "rate-limit housekeeping failed"),
        }
    }
}
