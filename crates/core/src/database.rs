use sqlx::{PgPool, postgres::PgPoolOptions};
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

pub async fn connect(url: &str, max: u32) -> Result<PgPool, sqlx::Error> {
    // Validate every checkout, including immediately after a DB/pooler restart.
    // sqlx evicts failed pings and retries acquisition within this budget. Its
    // mandatory release ping also evicts failures; do not suppress that check.
    // These are CLIENT lifetimes, independent of PgBouncer's server lifetimes.
    let options = PgPoolOptions::new()
        .max_connections(max)
        .min_connections(0)
        .max_lifetime(Duration::from_secs(30 * 60))
        .idle_timeout(Duration::from_secs(240))
        .acquire_timeout(Duration::from_secs(5))
        .test_before_acquire(true);
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

/// Safe, bounded labels only: never emit a DB message or connection URL.
pub fn failure_kind(error: &sqlx::Error) -> &'static str {
    match error {
        sqlx::Error::PoolTimedOut => "timeout",
        sqlx::Error::Io(_)
        | sqlx::Error::Tls(_)
        | sqlx::Error::PoolClosed
        | sqlx::Error::WorkerCrashed => "unreachable",
        sqlx::Error::Database(d) => {
            let code = d.code();
            let code = code.as_deref().unwrap_or("");
            if code.starts_with("28") {
                "auth"
            } else if d.message().contains("server_login_retry") {
                // PgBouncer caches backend connection failures, including
                // network errors. This alone does NOT prove bad credentials.
                "backend_login_retry"
            } else if code == "3D000" || code == "42501" {
                "configuration"
            } else if code == "57014" {
                "timeout"
            } else if code.starts_with("08") || code.starts_with("57P0") {
                "unreachable"
            } else {
                "query"
            }
        }
        _ => "query",
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn failure_labels_do_not_include_error_details() {
        assert_eq!(super::failure_kind(&sqlx::Error::PoolTimedOut), "timeout");
        assert_eq!(super::failure_kind(&sqlx::Error::PoolClosed), "unreachable");
        assert_eq!(
            super::failure_kind(&sqlx::Error::Io(std::io::Error::new(
                std::io::ErrorKind::UnexpectedEof,
                "sensitive detail"
            ))),
            "unreachable"
        );
    }
}
