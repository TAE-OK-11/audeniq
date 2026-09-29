use sqlx::{Connection, PgPool, postgres::PgPoolOptions};
use std::time::Duration;
pub static MIGRATOR: sqlx::migrate::Migrator = sqlx::migrate!("../../migrations");

/// True when `DATABASE_URL` points at PgBouncer in transaction pooling mode
/// (`DATABASE_POOLER=pgbouncer`). Session state does not survive there: the
/// per-connection timeouts come from the runtime roles' defaults instead
/// (`ALTER ROLE ... SET`, deploy/grants.sql), and LISTEN / migrations use a
/// direct connection (`DATABASE_LISTEN_URL`, the owner URL).
pub fn behind_pooler() -> bool {
    std::env::var("DATABASE_POOLER").is_ok_and(|v| v.eq_ignore_ascii_case("pgbouncer"))
}

/// A pooled connection idle for longer than this is pinged before use.
/// sqlx's default pings on *every* acquire: one extra database round trip
/// per query/transaction (2–3 per API request). A connection that was just
/// returned to the pool is known-good; only one that sat idle long enough
/// for PgBouncer/PostgreSQL/a network hop to drop it is worth checking.
const PING_AFTER_IDLE: Duration = Duration::from_secs(15);

pub async fn connect(url: &str, max: u32) -> Result<PgPool, sqlx::Error> {
    // Keep a couple of connections open even when idle: after a quiet spell
    // the first request no longer pays connect + SCRAM auth (cold start).
    // Connections above that floor are closed after 10 idle minutes (a
    // traffic spike does not pin PgBouncer/PostgreSQL slots forever), and all
    // are recycled every 30 min so server-side state (PgBouncer restarts,
    // role setting changes) does not live forever.
    let options = PgPoolOptions::new()
        .max_connections(max)
        .min_connections(max.min(2))
        .max_lifetime(Duration::from_secs(30 * 60))
        .idle_timeout(Duration::from_secs(10 * 60))
        .acquire_timeout(Duration::from_secs(5))
        .test_before_acquire(false)
        .before_acquire(|c, meta| {
            Box::pin(async move {
                if meta.idle_for >= PING_AFTER_IDLE {
                    c.ping().await?;
                }
                Ok(true)
            })
        });
    if behind_pooler() {
        return options.connect(url).await;
    }
    options
        .after_connect(|c, _| {
            Box::pin(async move {
                // One round trip for both settings.
                sqlx::raw_sql("SET statement_timeout='15s'; SET lock_timeout='3s'")
                    .execute(&mut *c)
                    .await?;
                Ok(())
            })
        })
        .connect(url)
        .await
}

/// Pool size from `DATABASE_MAX_CONNECTIONS` (2..=64), else `default`.
pub fn max_connections(default: u32) -> anyhow::Result<u32> {
    match std::env::var("DATABASE_MAX_CONNECTIONS") {
        Ok(raw) => {
            let n: u32 = raw.parse()?;
            anyhow::ensure!(
                (2..=64).contains(&n),
                "DATABASE_MAX_CONNECTIONS must be 2..=64"
            );
            Ok(n)
        }
        Err(std::env::VarError::NotPresent) => Ok(default),
        Err(error) => Err(error.into()),
    }
}
