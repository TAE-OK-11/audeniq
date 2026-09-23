#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let pool = audeniq_core::database::connect(&std::env::var("DATABASE_URL")?, 1).await?;
    audeniq_core::database::MIGRATOR.run(&pool).await?;
    println!("Migrations applied");
    Ok(())
}
