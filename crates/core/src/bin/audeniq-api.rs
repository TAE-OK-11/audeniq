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

/// Below compose's 30 s stop_grace_period.
const SHUTDOWN_GRACE: std::time::Duration = std::time::Duration::from_secs(25);

/// `audeniq-api healthcheck`: the container healthcheck (the image has no
/// curl). Exit 0 only when this instance answers `/ready`, which also reads
/// the database. Deploys wait on it before the tunnel sends traffic.
fn healthcheck() -> anyhow::Result<()> {
    use std::io::{Read, Write};
    let bind = std::env::var("API_BIND").unwrap_or_else(|_| "0.0.0.0:8080".into());
    let port = bind.rsplit(':').next().unwrap_or("8080");
    let secret = std::env::var("EDGE_SERVICE_SECRET").unwrap_or_default();
    let timeout = std::time::Duration::from_secs(4);
    let addr: std::net::SocketAddr = format!("127.0.0.1:{port}").parse()?;
    let mut tcp = std::net::TcpStream::connect_timeout(&addr, timeout)?;
    tcp.set_read_timeout(Some(timeout))?;
    tcp.set_write_timeout(Some(timeout))?;
    write!(
        tcp,
        "GET /ready HTTP/1.1\r\nHost: localhost\r\nx-audeniq-service: {secret}\r\nConnection: close\r\n\r\n"
    )?;
    let mut head = [0u8; 12];
    tcp.read_exact(&mut head)?;
    anyhow::ensure!(
        &head[9..12] == b"200",
        "not ready: {}",
        String::from_utf8_lossy(&head)
    );
    Ok(())
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    if std::env::args().nth(1).as_deref() == Some("healthcheck") {
        return healthcheck();
    }
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
    audeniq_core::temp_sweep::spawn(vec![std::env::temp_dir()]);
    // docker stop / deploys send SIGTERM: stop accepting, finish in-flight
    // requests (compose stop_grace_period: 30 s) instead of dropping them.
    // Live streams close on the signal; anything still running after
    // SHUTDOWN_GRACE is abandoned with a log line before Docker kills it.
    let mut sigterm = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())?;
    let (signalled, mut on_signal) = tokio::sync::watch::channel(false);
    let server = axum::serve(listener, api::router(state)).with_graceful_shutdown(async move {
        tokio::select! {
            _ = tokio::signal::ctrl_c() => {},
            _ = sigterm.recv() => {},
        }
        tracing::info!("shutdown signal: draining in-flight requests");
        audeniq_core::live::begin_shutdown();
        let _ = signalled.send(true);
    });
    let deadline = async move {
        let _ = on_signal.wait_for(|s| *s).await;
        tokio::time::sleep(SHUTDOWN_GRACE).await;
    };
    tokio::select! {
        served = server => served?,
        () = deadline => tracing::warn!(
            grace_seconds = SHUTDOWN_GRACE.as_secs(),
            "requests still running at the shutdown deadline; exiting"
        ),
    }
    Ok(())
}
