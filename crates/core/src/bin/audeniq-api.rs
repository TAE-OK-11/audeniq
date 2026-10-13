use audeniq_core::{
    api,
    config::Config,
    database,
    storage::{DisabledStore, ObjectStore, S3Store},
};
use axum::serve::ListenerExt;
use std::sync::Arc;
#[global_allocator]
static GLOBAL: mimalloc::MiMalloc = mimalloc::MiMalloc;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .json()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();
    let config = Config::from_env()?;
    if audeniq_core::config::kms_enabled()? {
        audeniq_core::payout_keys::KeyRing::from_env(true)?;
    }
    // Emails, names and contact data are stored only sealed (migration 0102):
    // production does not start without a key (KMS keyring or local key).
    anyhow::ensure!(
        std::env::var("APP_ENV").as_deref() != Ok("production")
            || audeniq_core::payout_keys::KeyRing::from_env(false)?.ready(),
        "PII_KEYRING_REQUIRED: set PAYOUT_KMS_ENABLED=true with the materialized keyring, or PAYOUT_ACCOUNT_KEY"
    );
    let pool = database::connect(&config.database_url, database::max_connections(6)?).await?;
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
    // TCP_NODELAY: responses are small JSON bodies on keep-alive connections
    // from the tunnel; Nagle + delayed ACK can hold one back for ~40 ms.
    let listener = tokio::net::TcpListener::bind(&config.bind)
        .await?
        .tap_io(|tcp| {
            if let Err(error) = tcp.set_nodelay(true) {
                tracing::debug!(%error, "TCP_NODELAY not set");
            }
        });
    // Notification emails (SES / OCI Email Delivery over SMTP); a bad
    // configuration stops startup instead of silently dropping mail.
    let mailer = audeniq_core::email::Mailer::from_env(&config.origin)?;
    match &mailer {
        Some(m) => tracing::info!(providers = ?m.provider_names(), "notification email enabled"),
        None => tracing::info!("notification email disabled (EMAIL_PROVIDERS empty)"),
    }
    let state = api::AppState::new(pool, config, storage).await?;
    audeniq_core::email::spawn(state.clone(), mailer);
    // docker stop / deploys send SIGTERM: stop accepting, finish in-flight
    // requests (compose stop_grace_period: 30 s) instead of dropping them.
    let mut sigterm = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())?;
    axum::serve(listener, api::router(state))
        .with_graceful_shutdown(async move {
            tokio::select! {
                _ = tokio::signal::ctrl_c() => {},
                _ = sigterm.recv() => {},
            }
            tracing::info!("shutdown signal: draining in-flight requests");
        })
        .await?;
    Ok(())
}
