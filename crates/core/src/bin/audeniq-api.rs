use audeniq_core::{
    api,
    config::Config,
    database,
    storage::{DisabledStore, ObjectStore, S3Store},
    studio_static,
};
use axum::serve::ListenerExt;
use std::future::IntoFuture;
use std::sync::Arc;
#[global_allocator]
static GLOBAL: audeniq_core::allocator::Allocator = audeniq_core::allocator::Allocator;
/// jemalloc options, read at start-up (crate::allocator). `oversize_threshold:0`:
/// blocks above 8 MiB (Argon2's 19 MiB per password hash) stay in the normal
/// arenas and their decay instead of going back to the OS on every free.
/// `background_thread:true`: purging runs on jemalloc's own threads, not on
/// the request that frees (docs/BACKEND_RESOURCE_PERFORMANCE.md, Allocator).
#[allow(non_upper_case_globals)]
#[unsafe(export_name = "_rjem_malloc_conf")]
pub static malloc_conf: &[u8] = b"oversize_threshold:0,background_thread:true\0";

/// Below compose's 30 s stop_grace_period.
const SHUTDOWN_GRACE: std::time::Duration = std::time::Duration::from_secs(25);

/// `audeniq-api healthcheck`: the container healthcheck (the image has no
/// curl). Exit 0 only when this instance answers `/ready`, which also reads
/// the database. Deploys wait on it before CloudFront sends traffic.
fn healthcheck() -> anyhow::Result<()> {
    use std::io::{Read, Write};
    let bind = std::env::var("API_BIND").unwrap_or_else(|_| "0.0.0.0:8080".into());
    let port = bind.rsplit(':').next().unwrap_or("8080");
    // 앞단이 보내는 값과 같은 것 (EDGE_HEADER_SECRETS가 있으면 그 첫 값)
    let secret =
        api::edge_header_secrets(&std::env::var("EDGE_SERVICE_SECRET").unwrap_or_default())?
            .swap_remove(0);
    let timeout = std::time::Duration::from_secs(7);
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
    if std::env::var("APP_ENV").as_deref() == Ok("production") {
        // The default hook prints payloads and optional backtraces even when
        // CatchPanicLayer catches the unwind. Keep production logs redacted.
        std::panic::set_hook(Box::new(|_| tracing::error!("process task panicked")));
    }
    let config = Config::from_env()?;
    api::edge_header_secrets(&config.service_secret)?;
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
    // from the edge; Nagle + delayed ACK can hold one back for ~40 ms.
    let listener = tokio::net::TcpListener::bind(&config.bind)
        .await?
        .tap_io(|tcp| {
            if let Err(error) = tcp.set_nodelay(true) {
                tracing::debug!(%error, "TCP_NODELAY not set");
            }
        });
    // TLS_BIND: HTTPS straight from AWS CloudFront (crate::edge_tls). A bad
    // certificate stops startup like a bad bind address does.
    let tls_listener = match audeniq_core::edge_tls::Settings::from_env()? {
        Some(settings) => Some(settings.bind().await?),
        None => None,
    };
    // Notification emails (SES / OCI Email Delivery over SMTP); a bad
    // configuration stops startup instead of silently dropping mail.
    let mailer = audeniq_core::email::Mailer::from_env(&config.origin)?;
    match &mailer {
        Some(m) => tracing::info!(providers = ?m.provider_names(), "notification email enabled"),
        None => tracing::info!("notification email disabled (EMAIL_PROVIDERS empty)"),
    }
    let state = api::AppState::new(pool, config, storage).await?;
    audeniq_core::email::spawn(state.clone(), mailer);
    audeniq_core::contract_terms::spawn(state.pool.clone());
    audeniq_core::temp_sweep::spawn(vec![std::env::temp_dir()]);
    // docker stop / deploys send SIGTERM: stop accepting, finish in-flight
    // requests (compose stop_grace_period: 30 s) instead of dropping them.
    // Live streams close on the signal; anything still running after
    // SHUTDOWN_GRACE is abandoned with a log line before Docker kills it.
    let mut sigterm = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())?;
    let (signalled, mut on_signal) = tokio::sync::watch::channel(false);
    // STUDIO_DIR: the Studio screens are served here too, outside the API's
    // service-secret boundary (crate::studio_static).
    let studio = studio_static::Studio::from_env();
    match &studio {
        Some(_) => tracing::info!("serving Studio static files (STUDIO_DIR)"),
        None => tracing::info!("Studio static files not served (STUDIO_DIR empty)"),
    }
    let shutdown = {
        let on_signal = on_signal.clone();
        move || {
            let mut on_signal = on_signal.clone();
            async move {
                let _ = on_signal.wait_for(|s| *s).await;
            }
        }
    };
    tokio::spawn(async move {
        tokio::select! {
            _ = tokio::signal::ctrl_c() => {},
            _ = sigterm.recv() => {},
        }
        tracing::info!("shutdown signal: draining in-flight requests");
        audeniq_core::live::begin_shutdown();
        let _ = signalled.send(true);
    });
    let tls = match tls_listener {
        Some(listener) => {
            let app = audeniq_core::edge_tls::app(state.clone(), studio.clone(), listener.hosts());
            Some(audeniq_core::edge_tls::serve(listener, app, shutdown()))
        }
        None => None,
    };
    let app = studio_static::app(state, studio);
    let plain = axum::serve(listener, app).with_graceful_shutdown(shutdown());
    let served = async move {
        match tls {
            Some(tls) => tokio::try_join!(plain.into_future(), tls).map(|_| ()),
            None => plain.await,
        }
    };
    let deadline = async move {
        let _ = on_signal.wait_for(|s| *s).await;
        tokio::time::sleep(SHUTDOWN_GRACE).await;
    };
    tokio::select! {
        served = served => served?,
        () = deadline => tracing::warn!(
            grace_seconds = SHUTDOWN_GRACE.as_secs(),
            "requests still running at the shutdown deadline; exiting"
        ),
    }
    Ok(())
}
