#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Not database::connect: its 15 s statement timeout (meant for request
    // and job queries) fails any index build or backfill on a large table and
    // with it the deploy. Migrations run without a statement limit; the short
    // lock timeout stays so a migration waiting on a busy table fails fast
    // instead of queueing every query behind its lock.
    let pool = sqlx::postgres::PgPoolOptions::new()
        .max_connections(1)
        .after_connect(|c, _| {
            Box::pin(async move {
                sqlx::raw_sql("SET statement_timeout=0; SET lock_timeout='3s'")
                    .execute(&mut *c)
                    .await?;
                Ok(())
            })
        })
        .connect(&std::env::var("DATABASE_URL")?)
        .await?;
    audeniq_core::database::MIGRATOR.run(&pool).await?;
    println!("Migrations applied");
    Ok(())
}
