use audeniq_core::{
    api,
    config::Config,
    database,
    storage::{DisabledStore, ObjectStore, S3Store},
};
use std::sync::Arc;
#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .json()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();
    let config = Config::from_env()?;
    let pool = database::connect(&config.database_url, 6).await?;
    let storage: Arc<dyn ObjectStore> = if std::env::var("STORAGE_ENABLED").as_deref() == Ok("true")
    {
        Arc::new(S3Store::new(
            &std::env::var("S3_ENDPOINT")?,
            std::env::var("S3_BUCKET")?,
            std::env::var("S3_ACCESS_KEY_ID")?,
            std::env::var("S3_SECRET_ACCESS_KEY")?,
            std::env::var("S3_REGION").unwrap_or_else(|_| "auto".into()),
            !config.secure_cookie,
        )?)
    } else {
        Arc::new(DisabledStore)
    };
    let listener = tokio::net::TcpListener::bind(&config.bind).await?;
    let state = api::AppState::new(pool, config, storage).await?;
    axum::serve(listener, api::router(state))
        .with_graceful_shutdown(async {
            let _ = tokio::signal::ctrl_c().await;
        })
        .await?;
    Ok(())
}
