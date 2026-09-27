use sqlx::{PgPool, postgres::PgPoolOptions};
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
    let options = PgPoolOptions::new()
        .max_connections(max)
        .acquire_timeout(std::time::Duration::from_secs(5));
    if behind_pooler() {
        return options.connect(url).await;
    }
    options
        .after_connect(|c, _| {
            Box::pin(async move {
                sqlx::query("SET statement_timeout='15s'")
                    .execute(&mut *c)
                    .await?;
                sqlx::query("SET lock_timeout='3s'")
                    .execute(&mut *c)
                    .await?;
                Ok(())
            })
        })
        .connect(url)
        .await
}
