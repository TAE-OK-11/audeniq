use sqlx::{PgPool, postgres::PgPoolOptions};
pub static MIGRATOR: sqlx::migrate::Migrator = sqlx::migrate!("../../migrations");
pub async fn connect(url: &str, max: u32) -> Result<PgPool, sqlx::Error> {
    PgPoolOptions::new()
        .max_connections(max)
        .acquire_timeout(std::time::Duration::from_secs(5))
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
