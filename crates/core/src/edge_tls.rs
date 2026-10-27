//! 백엔드가 직접 받는 HTTPS — AWS CloudFront가 이 서버를 오리진으로 부른다 (deploy/aws/README.md)
//!
//! 예전에는 Cloudflare Tunnel이 TLS를 끝내고 평문 8080으로 넘겼다. AWS에서는 ALB 없이 api가 인증서를
//! 직접 들고 `TLS_BIND`(컨테이너 8443 → 서버 443)에서 CloudFront의 요청을 받는다.
//!
//! - 인증서 (둘 중 하나):
//!   - `TLS_ACME_DOMAIN`: api가 Let's Encrypt에서 직접 받고 자동으로 갱신한다(crate::edge_acme, TLS-ALPN-01이라
//!     443 하나로 끝나고 80·DNS 토큰이 필요 없다). 받은 인증서는 `TLS_ACME_DIR`에 남아 재시작해도 그대로 쓴다.
//!   - `TLS_CERT_FILE`(전체 체인 PEM)·`TLS_KEY_FILE`: 바깥에서 받은 인증서(deploy/tls-cert.sh 등). 5분마다 파일을
//!     다시 보고, 바뀌었으면 다음 연결부터 새 인증서를 쓴다. 새 파일이 깨졌으면 이전 인증서를 계속 쓴다.
//! - 연결 출처: `TLS_ALLOWED_SOURCES=cloudfront`(기본)이면 CloudFront 오리진 쪽 IP 대역
//!   (`CLOUDFRONT_ORIGIN_FACING`)에서 온 연결만 받고, 나머지는 TLS 핸드셰이크 전에 끊는다. 대역은 AWS
//!   ip-ranges.json을 6시간마다 받고, 첫 응답 전에는 저장소 스냅샷(config/cloudfront-origin-facing.json)을 쓴다.
//!   예외는 하나: 인증서를 받는 동안에만 Let's Encrypt 검증 연결(ALPN `acme-tls/1`, 그 도메인 SNI)에 검증용
//!   인증서로 답하고 바로 닫는다. 요청은 받지 않는다.
//! - 이름: 오리진 이름(`TLS_ORIGIN_DOMAIN`, 예: api-origin.audeniq.com) 하나만 받는다. TLS SNI가 다르면 인증서도
//!   보내지 않고 끊고, HTTP Host(h2 :authority)가 다르면 403. 예외는 스튜디오·관리자 주소와 `TLS_EXTRA_HOSTS`.
//! - 요청: 이 리스너로 온 요청은 화면 파일까지 모두 서비스 비밀(`x-audeniq-service`, CloudFront origin custom
//!   header)이 있어야 한다. 다른 AWS 계정의 CloudFront가 이 서버를 오리진으로 걸어도 403이다.
//!
//! 평문 `API_BIND`는 컨테이너 healthcheck와 (전환 전까지) 터널용으로 그대로 둔다.
use crate::{api::AppState, edge_acme, error::Error, studio_static};
use axum::{
    Router,
    extract::{Request, State},
    middleware::{self, Next},
    response::{IntoResponse, Response},
};
use ipnet::IpNet;
use rustls::{
    ServerConfig,
    crypto::CryptoProvider,
    pki_types::{CertificateDer, PrivateKeyDer, pem::PemObject},
    server::{ClientHello, ResolvesServerCert},
    sign::CertifiedKey,
};
use sha2::{Digest, Sha256};
use std::net::{IpAddr, SocketAddr};
use std::path::{Path, PathBuf};
use std::sync::{Arc, RwLock};
use std::time::{Duration, Instant};
use tokio::net::TcpStream;
use tokio::sync::OwnedSemaphorePermit;
use tokio_rustls::{LazyConfigAcceptor, server::TlsStream};

/// AWS가 공개하는 IP 대역 (https://docs.aws.amazon.com/vpc/latest/userguide/aws-ip-ranges.html)
pub const AWS_IP_RANGES_URL: &str = "https://ip-ranges.amazonaws.com/ip-ranges.json";
/// CloudFront가 오리진에 연결할 때 쓰는 주소만 (엣지 전체 `CLOUDFRONT`보다 좁다)
const CLOUDFRONT_SERVICE: &str = "CLOUDFRONT_ORIGIN_FACING";
/// 이보다 적으면 받은 목록이 잘못된 것으로 보고 버린다 (지금 IPv4 약 50개)
const MIN_IPV4_PREFIXES: usize = 10;
const SNAPSHOT: &str = include_str!("../../../config/cloudfront-origin-facing.json");

const RANGES_REFRESH: Duration = Duration::from_secs(6 * 3600);
const RANGES_RETRY: Duration = Duration::from_secs(600);
const CERT_RELOAD: Duration = Duration::from_secs(300);
/// 느린 핸드셰이크가 연결 슬롯을 오래 잡지 않게
const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(10);
/// 동시에 진행하는 핸드셰이크 상한 (넘으면 새 연결을 바로 끊는다)
const MAX_HANDSHAKES: usize = 256;
/// 동시에 여는 연결 상한 기본값 (`TLS_MAX_CONNECTIONS`). CloudFront는 연결을 재사용해서 보통 수십 개
const DEFAULT_MAX_CONNECTIONS: usize = 512;
/// 요청 헤더를 다 받을 때까지 (HTTP/1.1). CloudFront는 헤더를 한 번에 보낸다: 느린 헤더(slowloris)를 끊는다
const HEADER_READ_TIMEOUT: Duration = Duration::from_secs(10);
/// 어느 쪽으로도 바이트가 오가지 않으면 끊는다. CloudFront의 오리진 응답 대기(60초)·keep-alive(60초)보다 길게
const IDLE_TIMEOUT: Duration = Duration::from_secs(75);
/// h2 연결 하나의 동시 스트림
const MAX_H2_STREAMS: u32 = 128;

/// 누가 이 리스너에 연결할 수 있나
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Sources {
    /// CloudFront 오리진 쪽 IP 대역만
    CloudFront,
    /// 아무 주소나 (개발·시험용, production에서는 거절)
    Any,
}

/// 인증서를 어디서 얻나
#[derive(Clone, Debug)]
pub enum Certificates {
    /// 바깥에서 받은 PEM 파일 (5분마다 다시 읽음)
    Files { cert: PathBuf, key: PathBuf },
    /// api가 Let's Encrypt에서 직접 받고 갱신
    Acme(edge_acme::Acme),
}

#[derive(Clone, Debug)]
pub struct Settings {
    pub bind: String,
    pub certificates: Certificates,
    pub sources: Sources,
    /// 이 리스너가 받는 이름 (TLS SNI·HTTP Host). 맨 앞이 오리진 도메인
    pub hosts: OriginHosts,
    /// 동시에 여는 연결 상한 (핸드셰이크 중인 것 포함)
    pub max_connections: usize,
}

/// 오리진으로 받는 이름: `TLS_ORIGIN_DOMAIN`(= CloudFront ApiOriginDomain, 예: api-origin.audeniq.com)과
/// 예외로 스튜디오·관리자 주소(`APP_ORIGIN`·`ADMIN_ORIGIN`의 호스트), `TLS_EXTRA_HOSTS`(쉼표).
/// 그 밖의 이름으로 온 연결·요청은 거절한다 (서버 IP로 바로, 다른 도메인을 이 서버로 돌린 것 등).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OriginHosts(Arc<[String]>);

impl OriginHosts {
    fn new(names: impl IntoIterator<Item = String>) -> Self {
        let mut hosts: Vec<String> = Vec::new();
        for name in names {
            let name = normalize_host(&name);
            if !name.is_empty() && !hosts.contains(&name) {
                hosts.push(name);
            }
        }
        Self(hosts.into())
    }

    /// 오리진 도메인
    pub fn origin(&self) -> &str {
        &self.0[0]
    }

    /// `name`은 SNI 그대로이거나 Host 헤더(포트가 붙을 수 있다)
    pub fn allows(&self, name: &str) -> bool {
        let name = name.trim();
        // [IPv6]:port, host:port → host. IP로 바로 온 요청은 어차피 목록에 없다
        let host = match name.rsplit_once(':') {
            Some((host, port))
                if !host.contains(':') && port.bytes().all(|b| b.is_ascii_digit()) =>
            {
                host
            }
            _ => name,
        };
        let host = host.strip_suffix('.').unwrap_or(host);
        self.0
            .iter()
            .any(|allowed| allowed.eq_ignore_ascii_case(host))
    }
}

fn normalize_host(name: &str) -> String {
    let name = name.trim().to_ascii_lowercase();
    name.strip_suffix('.').map(str::to_owned).unwrap_or(name)
}

/// `https://studio.audeniq.com` → `studio.audeniq.com`
fn origin_host(origin: Option<String>) -> Option<String> {
    let url = url::Url::parse(&origin?).ok()?;
    url.host_str().map(str::to_owned)
}

impl Settings {
    /// `TLS_BIND`가 비어 있으면 HTTPS 리스너를 켜지 않는다.
    pub fn from_env() -> anyhow::Result<Option<Self>> {
        Self::from_values(crate::config::env_value)
    }

    fn from_values(get: impl Fn(&str) -> Option<String>) -> anyhow::Result<Option<Self>> {
        let Some(bind) = get("TLS_BIND") else {
            return Ok(None);
        };
        // TLS_ACME_DOMAIN이 있으면 Let's Encrypt (compose가 늘 넣어 두는 TLS_CERT_FILE은 쓰지 않는다)
        let certificates = match edge_acme::Acme::from_values(&get)? {
            Some(acme) => Certificates::Acme(acme),
            None => Certificates::Files {
                cert: get("TLS_CERT_FILE")
                    .ok_or_else(|| {
                        anyhow::anyhow!("TLS_BIND requires TLS_ACME_DOMAIN or TLS_CERT_FILE")
                    })?
                    .into(),
                key: get("TLS_KEY_FILE")
                    .ok_or_else(|| anyhow::anyhow!("TLS_CERT_FILE requires TLS_KEY_FILE"))?
                    .into(),
            },
        };
        let sources = match get("TLS_ALLOWED_SOURCES").as_deref() {
            None | Some("cloudfront") => Sources::CloudFront,
            Some("any") => Sources::Any,
            Some(other) => {
                anyhow::bail!("TLS_ALLOWED_SOURCES must be cloudfront or any, not {other:?}")
            }
        };
        anyhow::ensure!(
            sources == Sources::CloudFront || get("APP_ENV").as_deref() != Some("production"),
            "production accepts HTTPS only from CloudFront (TLS_ALLOWED_SOURCES=cloudfront)"
        );
        // 오리진 도메인: TLS_ORIGIN_DOMAIN, 없으면 Let's Encrypt로 받는 이름 (둘 다면 같아야 한다)
        let acme_domain = match &certificates {
            Certificates::Acme(acme) => Some(acme.domain().to_owned()),
            Certificates::Files { .. } => None,
        };
        let origin = match (
            get("TLS_ORIGIN_DOMAIN").map(|d| normalize_host(&d)),
            acme_domain,
        ) {
            (Some(origin), Some(acme)) => {
                anyhow::ensure!(
                    origin == acme,
                    "TLS_ORIGIN_DOMAIN ({origin}) and TLS_ACME_DOMAIN ({acme}) must be the same name"
                );
                origin
            }
            (Some(origin), None) | (None, Some(origin)) => origin,
            (None, None) => {
                anyhow::bail!("TLS_BIND requires TLS_ORIGIN_DOMAIN (the CloudFront origin name)")
            }
        };
        let extra = get("TLS_EXTRA_HOSTS").unwrap_or_default();
        let hosts = OriginHosts::new(
            [
                Some(origin),
                origin_host(get("APP_ORIGIN")),
                origin_host(get("ADMIN_ORIGIN")),
            ]
            .into_iter()
            .flatten()
            .chain(extra.split(',').map(str::to_owned)),
        );
        let max_connections = match get("TLS_MAX_CONNECTIONS") {
            None => DEFAULT_MAX_CONNECTIONS,
            Some(n) => n
                .parse()
                .ok()
                .filter(|n| (16..=65_536).contains(n))
                .ok_or_else(|| anyhow::anyhow!("TLS_MAX_CONNECTIONS must be 16..=65536"))?,
        };
        Ok(Some(Self {
            bind,
            certificates,
            sources,
            hosts,
            max_connections,
        }))
    }

    /// 인증서를 준비하고 포트를 연다. 인증서 갱신·IP 대역 갱신 작업도 여기서 띄운다.
    /// 파일 인증서가 깨졌으면 시작하지 않는다. Let's Encrypt는 아직 인증서가 없으면 받을 때까지
    /// 핸드셰이크가 실패할 뿐 api는 뜬다(평문 API·healthcheck는 그대로).
    pub async fn bind(self) -> anyhow::Result<TlsListener> {
        let provider = Arc::new(rustls::crypto::aws_lc_rs::default_provider());
        let certs = Arc::new(match &self.certificates {
            Certificates::Files { cert, key } => CertStore::from_files(cert, key, &provider)?,
            Certificates::Acme(acme) => acme.load_stored(&provider),
        });
        let mut config = ServerConfig::builder_with_provider(provider.clone())
            .with_safe_default_protocol_versions()?
            .with_no_client_auth()
            .with_cert_resolver(certs.clone());
        // CloudFront는 오리진에 HTTP/1.1로 붙지만, h2를 내미는 상대(시험용 curl, 이후 gRPC 등)도 받는다
        config.alpn_protocols = vec![b"h2".to_vec(), b"http/1.1".to_vec()];

        let allow = Arc::new(match self.sources {
            Sources::CloudFront => Allow::cloudfront(),
            Sources::Any => Allow::any(),
        });
        let tcp = tokio::net::TcpListener::bind(&self.bind).await?;
        let local = tcp.local_addr()?;
        tracing::info!(
            bind = %local,
            sources = ?self.sources,
            prefixes = allow.len(),
            hosts = ?self.hosts.0,
            max_connections = self.max_connections,
            certificate = match &self.certificates {
                Certificates::Files { .. } => "files",
                Certificates::Acme(_) => "letsencrypt",
            },
            "HTTPS listener (TLS_BIND)"
        );

        let challenges = Arc::new(edge_acme::Challenges::default());
        match self.certificates {
            Certificates::Files { .. } => {
                tokio::spawn(reload_certs(certs, provider));
            }
            Certificates::Acme(acme) => {
                tokio::spawn(edge_acme::run(acme, certs, challenges.clone(), provider));
            }
        }
        if self.sources == Sources::CloudFront {
            tokio::spawn(refresh_ranges(allow.clone()));
        }
        let (tx, rx) = tokio::sync::mpsc::channel(64);
        let hosts = self.hosts.clone();
        let connections = Arc::new(tokio::sync::Semaphore::new(self.max_connections));
        tokio::spawn(accept_loop(
            tcp,
            Arc::new(config),
            allow,
            challenges,
            self.hosts,
            connections,
            tx,
        ));
        Ok(TlsListener { rx, local, hosts })
    }
}

/// 이 리스너로 서빙할 앱: 화면·API 모두 오리진 이름(Host)과 서비스 비밀이 맞아야 한다.
pub fn app(s: AppState, studio: Option<Arc<studio_static::Studio>>, hosts: OriginHosts) -> Router {
    studio_static::app(s.clone(), studio)
        .layer(middleware::from_fn_with_state((s, hosts), require_edge))
}

async fn require_edge(
    State((s, hosts)): State<(AppState, OriginHosts)>,
    req: Request,
    next: Next,
) -> Response {
    // HTTP/1.1은 Host 헤더, h2는 :authority(uri)
    let host = req
        .headers()
        .get(axum::http::header::HOST)
        .and_then(|v| v.to_str().ok())
        .or_else(|| req.uri().authority().map(|a| a.as_str()));
    if !host.is_some_and(|h| hosts.allows(h)) {
        tracing::warn!(
            host = host.unwrap_or(""),
            "HTTPS request for a name other than the origin"
        );
        return Error::Forbidden.into_response();
    }
    if !s.edge_authenticated(req.headers()) {
        tracing::warn!(path = %req.uri().path(), "HTTPS request without the edge service secret");
        return Error::Forbidden.into_response();
    }
    next.run(req).await
}

/// 핸드셰이크를 마친 연결 (연결 상한의 몫을 함께 들고 있다)
type Accepted = (TlsStream<TcpStream>, SocketAddr, OwnedSemaphorePermit);

/// 핸드셰이크를 마친 연결을 [`serve`]에 넘긴다. 핸드셰이크는 연결마다 따로 돌아서 느린 상대가 다른 연결을 막지 않는다.
pub struct TlsListener {
    rx: tokio::sync::mpsc::Receiver<Accepted>,
    local: SocketAddr,
    hosts: OriginHosts,
}

impl TlsListener {
    /// 요청에서 확인할 이름 ([`app`]에 넘긴다)
    pub fn hosts(&self) -> OriginHosts {
        self.hosts.clone()
    }

    pub fn local_addr(&self) -> SocketAddr {
        self.local
    }
}

/// 이 리스너의 연결을 서빙한다. axum::serve와 같지만 연결마다 헤더 읽기·유휴 시간 제한이 있다.
/// `shutdown`이 끝나면 새 연결을 받지 않고, 열린 연결은 처리 중인 요청을 마치고 닫는다.
pub async fn serve(
    mut listener: TlsListener,
    app: Router,
    shutdown: impl std::future::Future<Output = ()> + Send,
) -> std::io::Result<()> {
    use hyper_util::rt::{TokioExecutor, TokioIo, TokioTimer};
    let (signal_tx, signal_rx) = tokio::sync::watch::channel(());
    let (close_tx, close_rx) = tokio::sync::watch::channel(());
    let mut shutdown = std::pin::pin!(shutdown);
    loop {
        let (tls, peer, permit) = tokio::select! {
            accepted = listener.rx.recv() => match accepted {
                Some(accepted) => accepted,
                None => break,
            },
            () = &mut shutdown => break,
        };
        let app = app.clone();
        let mut signal = signal_rx.clone();
        let close = close_rx.clone();
        tokio::spawn(async move {
            let service =
                hyper::service::service_fn(move |req: hyper::Request<hyper::body::Incoming>| {
                    let mut app = app.clone();
                    // Router는 늘 준비돼 있다 (poll_ready 없이 call)
                    async move {
                        tower_service::Service::call(&mut app, req.map(axum::body::Body::new)).await
                    }
                });
            let mut builder = hyper_util::server::conn::auto::Builder::new(TokioExecutor::new());
            builder
                .http1()
                .timer(TokioTimer::new())
                .header_read_timeout(HEADER_READ_TIMEOUT);
            builder
                .http2()
                .timer(TokioTimer::new())
                .max_concurrent_streams(MAX_H2_STREAMS);
            let io = TokioIo::new(Idle::new(tls, IDLE_TIMEOUT));
            let mut conn = std::pin::pin!(builder.serve_connection_with_upgrades(io, service));
            loop {
                tokio::select! {
                    result = conn.as_mut() => {
                        if let Err(error) = result {
                            tracing::debug!(peer = %peer.ip(), %error, "HTTPS connection ended");
                        }
                        break;
                    }
                    _ = signal.changed() => conn.as_mut().graceful_shutdown(),
                }
            }
            drop((permit, close));
        });
    }
    // 새 연결은 그만 받고(받는 작업도 끝난다), 열린 연결에 알린 뒤 모두 닫힐 때까지
    drop(listener);
    let _ = signal_tx.send(());
    drop(signal_rx);
    drop(close_rx);
    close_tx.closed().await;
    Ok(())
}

/// 어느 쪽으로도 바이트가 `timeout` 동안 오가지 않으면 연결을 닫는다.
/// 읽기·쓰기가 될 때마다 시한을 다시 잡는다 (실시간 알림은 20초마다 keep-alive를 써서 끊기지 않는다).
struct Idle<T> {
    io: T,
    timeout: Duration,
    deadline: std::pin::Pin<Box<tokio::time::Sleep>>,
}

impl<T> Idle<T> {
    fn new(io: T, timeout: Duration) -> Self {
        Self {
            io,
            timeout,
            deadline: Box::pin(tokio::time::sleep(timeout)),
        }
    }

    fn touch(&mut self) {
        let next = tokio::time::Instant::now() + self.timeout;
        self.deadline.as_mut().reset(next);
    }

    fn expired(&mut self, cx: &mut std::task::Context<'_>) -> bool {
        self.deadline.as_mut().poll(cx).is_ready()
    }
}

fn idle_error() -> std::io::Error {
    std::io::Error::new(std::io::ErrorKind::TimedOut, "HTTPS connection idle")
}

impl<T: tokio::io::AsyncRead + Unpin> tokio::io::AsyncRead for Idle<T> {
    fn poll_read(
        self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        buf: &mut tokio::io::ReadBuf<'_>,
    ) -> std::task::Poll<std::io::Result<()>> {
        use std::task::Poll;
        let this = self.get_mut();
        match std::pin::Pin::new(&mut this.io).poll_read(cx, buf) {
            Poll::Ready(result) => {
                this.touch();
                Poll::Ready(result)
            }
            Poll::Pending if this.expired(cx) => Poll::Ready(Err(idle_error())),
            Poll::Pending => Poll::Pending,
        }
    }
}

impl<T: tokio::io::AsyncWrite + Unpin> tokio::io::AsyncWrite for Idle<T> {
    fn poll_write(
        self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        data: &[u8],
    ) -> std::task::Poll<std::io::Result<usize>> {
        use std::task::Poll;
        let this = self.get_mut();
        match std::pin::Pin::new(&mut this.io).poll_write(cx, data) {
            Poll::Ready(result) => {
                this.touch();
                Poll::Ready(result)
            }
            // 받는 쪽이 읽지 않아 쓰기가 막힌 채로도 끝없이 기다리지 않는다
            Poll::Pending if this.expired(cx) => Poll::Ready(Err(idle_error())),
            Poll::Pending => Poll::Pending,
        }
    }

    fn poll_flush(
        self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<std::io::Result<()>> {
        std::pin::Pin::new(&mut self.get_mut().io).poll_flush(cx)
    }

    fn poll_shutdown(
        self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<std::io::Result<()>> {
        std::pin::Pin::new(&mut self.get_mut().io).poll_shutdown(cx)
    }
}

async fn accept_loop(
    tcp: tokio::net::TcpListener,
    config: Arc<ServerConfig>,
    allow: Arc<Allow>,
    challenges: Arc<edge_acme::Challenges>,
    hosts: OriginHosts,
    connections: Arc<tokio::sync::Semaphore>,
    tx: tokio::sync::mpsc::Sender<Accepted>,
) {
    let handshakes = Arc::new(tokio::sync::Semaphore::new(MAX_HANDSHAKES));
    let mut rejected = 0u64;
    let mut last_report = Instant::now();
    let mut full = 0u64;
    let mut last_full_report = Instant::now();
    loop {
        let (stream, peer) = tokio::select! {
            accepted = tcp.accept() => match accepted {
                Ok(v) => v,
                Err(error) => {
                    // 파일 기술자 부족 등: 잠깐 쉬고 다시
                    tracing::warn!(%error, "HTTPS accept failed");
                    tokio::time::sleep(Duration::from_millis(100)).await;
                    continue;
                }
            },
            // 서버가 멈췄다(종료 중)
            () = tx.closed() => return,
        };
        let allowed = allow.permits(peer.ip());
        // 인증서를 받는 중이 아니면 CloudFront 밖의 연결은 ClientHello도 읽지 않는다
        if !allowed && !challenges.pending() {
            rejected += 1;
            if rejected == 1 || last_report.elapsed() >= Duration::from_secs(60) {
                tracing::warn!(peer = %peer.ip(), rejected, "HTTPS connection refused: not a CloudFront origin address");
                rejected = 0;
                last_report = Instant::now();
            }
            continue;
        }
        let Ok(connection) = connections.clone().try_acquire_owned() else {
            full += 1;
            if full == 1 || last_full_report.elapsed() >= Duration::from_secs(60) {
                tracing::warn!(peer = %peer.ip(), dropped = full, "HTTPS connection dropped: connection limit reached (TLS_MAX_CONNECTIONS)");
                full = 0;
                last_full_report = Instant::now();
            }
            continue;
        };
        let Ok(permit) = handshakes.clone().try_acquire_owned() else {
            tracing::warn!(peer = %peer.ip(), "HTTPS connection dropped: too many handshakes in progress");
            continue;
        };
        if let Err(error) = stream.set_nodelay(true) {
            tracing::debug!(%error, "TCP_NODELAY not set");
        }
        let (config, challenges, hosts, tx) = (
            config.clone(),
            challenges.clone(),
            hosts.clone(),
            tx.clone(),
        );
        tokio::spawn(async move {
            let shake = handshake(stream, peer, allowed, config, &challenges, &hosts);
            match tokio::time::timeout(HANDSHAKE_TIMEOUT, shake).await {
                Ok(Ok(Some(tls))) => {
                    drop(permit);
                    let _ = tx.send((tls, peer, connection)).await;
                }
                Ok(Ok(None)) => {}
                Ok(Err(error)) => {
                    tracing::debug!(peer = %peer.ip(), %error, "TLS handshake failed")
                }
                Err(_) => tracing::debug!(peer = %peer.ip(), "TLS handshake timed out"),
            }
        });
    }
}

/// ClientHello를 보고 나눈다: Let's Encrypt 검증(acme-tls/1)이면 검증용 인증서로 답하고 닫고(None),
/// 그 밖에는 CloudFront에서 온, 오리진 이름(SNI)으로 온 연결만 보통 핸드셰이크를 마친다.
async fn handshake(
    stream: TcpStream,
    peer: SocketAddr,
    allowed: bool,
    config: Arc<ServerConfig>,
    challenges: &edge_acme::Challenges,
    hosts: &OriginHosts,
) -> std::io::Result<Option<TlsStream<TcpStream>>> {
    let start = LazyConfigAcceptor::new(rustls::server::Acceptor::default(), stream).await?;
    let hello = start.client_hello();
    if edge_acme::is_challenge(&hello) {
        let challenge = hello.server_name().and_then(|name| challenges.config(name));
        if let Some(challenge) = challenge {
            let mut tls = start.into_stream(challenge).await?;
            tracing::info!(peer = %peer.ip(), "ACME TLS-ALPN-01 challenge answered");
            let _ = tokio::io::AsyncWriteExt::shutdown(&mut tls).await;
        }
        return Ok(None);
    }
    if !allowed {
        tracing::debug!(peer = %peer.ip(), "HTTPS connection refused during ACME validation: not a CloudFront origin address");
        return Ok(None);
    }
    // SNI 없이(IP로 바로) 오거나 다른 이름이면 인증서도 보내지 않고 닫는다
    if !hello.server_name().is_some_and(|name| hosts.allows(name)) {
        tracing::debug!(peer = %peer.ip(), sni = hello.server_name().unwrap_or(""), "HTTPS connection refused: not the origin name");
        return Ok(None);
    }
    Ok(Some(start.into_stream(config).await?))
}

// ---- 인증서 ----

/// 지금 쓰는 인증서. 파일 모드는 [`CertStore::reload`]가, Let's Encrypt는 [`CertStore::install`]이 바꿔 끼운다.
/// 아직 없으면(Let's Encrypt 첫 발급 전) 핸드셰이크가 실패한다.
#[derive(Debug)]
pub(crate) struct CertStore {
    files: Option<(PathBuf, PathBuf)>,
    current: RwLock<Option<Loaded>>,
}

#[derive(Debug)]
struct Loaded {
    fingerprint: Fingerprint,
    key: Arc<CertifiedKey>,
}

type Fingerprint = [u8; 32];

impl CertStore {
    pub(crate) fn empty() -> Self {
        Self {
            files: None,
            current: RwLock::new(None),
        }
    }

    fn from_files(cert: &Path, key: &Path, provider: &CryptoProvider) -> anyhow::Result<Self> {
        let loaded = read_pair(cert, key, provider)?;
        Ok(Self {
            files: Some((cert.to_owned(), key.to_owned())),
            current: RwLock::new(Some(loaded)),
        })
    }

    fn fingerprint(&self) -> Option<Fingerprint> {
        self.current
            .read()
            .unwrap_or_else(|e| e.into_inner())
            .as_ref()
            .map(|l| l.fingerprint)
    }

    /// 파일 모드: 파일이 바뀌었으면 다시 읽는다. 새로 바꿔 끼웠으면 true.
    fn reload(&self, provider: &CryptoProvider) -> anyhow::Result<bool> {
        let Some((cert, key)) = &self.files else {
            return Ok(false);
        };
        let fingerprint = fingerprint(&std::fs::read(cert)?, &std::fs::read(key)?);
        if self.fingerprint() == Some(fingerprint) {
            return Ok(false);
        }
        let loaded = read_pair(cert, key, provider)?;
        *self.current.write().unwrap_or_else(|e| e.into_inner()) = Some(loaded);
        Ok(true)
    }

    /// 새 인증서(PEM)로 바꿔 끼운다. 깨졌거나 키가 맞지 않으면 지금 것을 그대로 둔다.
    pub(crate) fn install(
        &self,
        cert_pem: &[u8],
        key_pem: &[u8],
        provider: &CryptoProvider,
    ) -> anyhow::Result<()> {
        let key = parse_pair(cert_pem, key_pem, provider)?;
        *self.current.write().unwrap_or_else(|e| e.into_inner()) = Some(Loaded {
            fingerprint: fingerprint(cert_pem, key_pem),
            key: Arc::new(key),
        });
        Ok(())
    }

    /// 지금 인증서의 맨 앞(서버) 인증서
    pub(crate) fn leaf(&self) -> Option<CertificateDer<'static>> {
        self.current
            .read()
            .unwrap_or_else(|e| e.into_inner())
            .as_ref()
            .and_then(|l| l.key.cert.first().cloned())
    }
}

impl ResolvesServerCert for CertStore {
    fn resolve(&self, _hello: ClientHello<'_>) -> Option<Arc<CertifiedKey>> {
        self.current
            .read()
            .unwrap_or_else(|e| e.into_inner())
            .as_ref()
            .map(|l| l.key.clone())
    }
}

fn fingerprint(cert: &[u8], key: &[u8]) -> Fingerprint {
    let mut h = Sha256::new();
    h.update(cert);
    h.update([0]);
    h.update(key);
    h.finalize().into()
}

fn read_pair(cert: &Path, key: &Path, provider: &CryptoProvider) -> anyhow::Result<Loaded> {
    let cert_pem = std::fs::read(cert)
        .map_err(|e| anyhow::anyhow!("TLS_CERT_FILE {}: {e}", cert.display()))?;
    let key_pem =
        std::fs::read(key).map_err(|e| anyhow::anyhow!("TLS_KEY_FILE {}: {e}", key.display()))?;
    Ok(Loaded {
        fingerprint: fingerprint(&cert_pem, &key_pem),
        key: Arc::new(parse_pair(&cert_pem, &key_pem, provider)?),
    })
}

pub(crate) fn parse_pair(
    cert_pem: &[u8],
    key_pem: &[u8],
    provider: &CryptoProvider,
) -> anyhow::Result<CertifiedKey> {
    let chain = CertificateDer::pem_slice_iter(cert_pem)
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| anyhow::anyhow!("TLS certificate is not PEM: {e}"))?;
    anyhow::ensure!(!chain.is_empty(), "TLS certificate file has no certificate");
    let key = PrivateKeyDer::from_pem_slice(key_pem)
        .map_err(|e| anyhow::anyhow!("TLS key is not a PEM private key: {e}"))?;
    // 키가 인증서와 짝이 맞는지도 확인한다
    CertifiedKey::from_der(chain, key, provider)
        .map_err(|e| anyhow::anyhow!("TLS certificate and key do not match: {e}"))
}

async fn reload_certs(certs: Arc<CertStore>, provider: Arc<CryptoProvider>) {
    let mut tick = tokio::time::interval(CERT_RELOAD);
    tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    tick.tick().await;
    loop {
        tick.tick().await;
        let (certs, provider) = (certs.clone(), provider.clone());
        match tokio::task::spawn_blocking(move || certs.reload(&provider)).await {
            Ok(Ok(true)) => tracing::info!("TLS certificate reloaded"),
            Ok(Ok(false)) => {}
            Ok(Err(error)) => {
                tracing::error!(%error, "TLS certificate not reloaded; keeping the current one")
            }
            Err(error) => tracing::error!(%error, "TLS certificate reload panicked"),
        }
    }
}

// ---- 연결 출처 ----
// 연결마다 한 번 보는 값이라 싸게: 대역을 정수 구간으로 바꿔 합치고 정렬해 두고 이분 탐색한다
// (지금 87개 대역 → 합친 구간 수십 개, 조회 6~7번 비교). 목록은 6시간마다 통째로 바꾼다.

/// 연결을 받을 주소. `None`은 아무 주소나.
struct Allow {
    ranges: Option<RwLock<Ranges>>,
}

impl Allow {
    fn cloudfront() -> Self {
        Self {
            ranges: Some(RwLock::new(snapshot_ranges())),
        }
    }

    fn any() -> Self {
        Self { ranges: None }
    }

    fn len(&self) -> usize {
        self.ranges
            .as_ref()
            .map_or(0, |r| r.read().unwrap_or_else(|e| e.into_inner()).prefixes)
    }

    fn permits(&self, ip: IpAddr) -> bool {
        let Some(ranges) = &self.ranges else {
            return true;
        };
        ranges
            .read()
            .unwrap_or_else(|e| e.into_inner())
            .contains(ip)
    }

    fn replace(&self, new: Ranges) {
        if let Some(ranges) = &self.ranges {
            *ranges.write().unwrap_or_else(|e| e.into_inner()) = new;
        }
    }
}

/// CloudFront 오리진 쪽 대역: 겹치거나 맞닿은 대역을 합친 [시작, 끝] 정수 구간, 시작 순
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Ranges {
    v4: Box<[(u32, u32)]>,
    v6: Box<[(u128, u128)]>,
    /// 합치기 전 대역 수 (로그용)
    prefixes: usize,
}

impl Ranges {
    pub fn from_nets(nets: &[IpNet]) -> Self {
        let mut v4 = Vec::new();
        let mut v6 = Vec::new();
        for net in nets {
            match net {
                IpNet::V4(n) => v4.push((u32::from(n.network()), u32::from(n.broadcast()))),
                IpNet::V6(n) => v6.push((u128::from(n.network()), u128::from(n.broadcast()))),
            }
        }
        Self {
            v4: merge(v4),
            v6: merge(v6),
            prefixes: nets.len(),
        }
    }

    /// 듀얼 스택 소켓의 ::ffff:a.b.c.d 는 IPv4로 본다
    pub fn contains(&self, ip: IpAddr) -> bool {
        match ip.to_canonical() {
            IpAddr::V4(ip) => within(&self.v4, u32::from(ip)),
            IpAddr::V6(ip) => within(&self.v6, u128::from(ip)),
        }
    }

    /// 합친 구간 수 (IPv4, IPv6)
    pub fn intervals(&self) -> (usize, usize) {
        (self.v4.len(), self.v6.len())
    }
}

fn merge<T: Ord + Copy + num_step::Step>(mut ranges: Vec<(T, T)>) -> Box<[(T, T)]> {
    ranges.sort_unstable();
    let mut merged: Vec<(T, T)> = Vec::with_capacity(ranges.len());
    for (start, end) in ranges {
        match merged.last_mut() {
            Some(last) if last.1.next().is_none_or(|next| start <= next) => {
                last.1 = last.1.max(end)
            }
            _ => merged.push((start, end)),
        }
    }
    merged.into_boxed_slice()
}

fn within<T: Ord + Copy>(ranges: &[(T, T)], ip: T) -> bool {
    let after = ranges.partition_point(|&(start, _)| start <= ip);
    after > 0 && ip <= ranges[after - 1].1
}

/// u32·u128의 다음 값 (끝이면 None) — 맞닿은 구간 합치기용
mod num_step {
    pub trait Step: Sized {
        fn next(self) -> Option<Self>;
    }
    impl Step for u32 {
        fn next(self) -> Option<Self> {
            self.checked_add(1)
        }
    }
    impl Step for u128 {
        fn next(self) -> Option<Self> {
            self.checked_add(1)
        }
    }
}

#[derive(serde::Deserialize)]
struct Snapshot {
    prefixes: Vec<IpNet>,
}

/// 저장소 스냅샷 (config/cloudfront-origin-facing.json)
pub fn snapshot_ranges() -> Ranges {
    let snapshot: Snapshot =
        serde_json::from_str(SNAPSHOT).expect("config/cloudfront-origin-facing.json is valid");
    check_prefixes(snapshot.prefixes).expect("config/cloudfront-origin-facing.json is valid")
}

/// AWS ip-ranges.json → CloudFront 오리진 쪽 대역.
/// 2.7 MB·1만 8천 항목 중 CloudFront 것(약 90개)만 남긴다: 항목은 원문에서 빌려 읽어(문자열 할당 없음)
/// 서비스 이름이 맞을 때만 대역을 해석한다.
pub fn parse_aws_ranges(text: &[u8]) -> anyhow::Result<Ranges> {
    let ranges: AwsRanges = serde_json::from_slice(text)?;
    let mut nets = ranges.prefixes.0;
    nets.extend(ranges.ipv6_prefixes.0);
    check_prefixes(nets)
}

#[derive(serde::Deserialize)]
struct AwsRanges {
    prefixes: CloudFrontOnly,
    ipv6_prefixes: CloudFrontOnly,
}

/// 배열을 읽으면서 CloudFront 오리진 쪽 항목만 모은다
struct CloudFrontOnly(Vec<IpNet>);

#[derive(serde::Deserialize)]
struct AwsPrefix<'a> {
    #[serde(alias = "ipv6_prefix", borrow)]
    ip_prefix: std::borrow::Cow<'a, str>,
    #[serde(borrow)]
    service: std::borrow::Cow<'a, str>,
}

impl<'de> serde::Deserialize<'de> for CloudFrontOnly {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct Visit;
        impl<'de> serde::de::Visitor<'de> for Visit {
            type Value = CloudFrontOnly;
            fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
                f.write_str("an array of AWS IP prefixes")
            }
            fn visit_seq<A: serde::de::SeqAccess<'de>>(
                self,
                mut seq: A,
            ) -> Result<CloudFrontOnly, A::Error> {
                let mut nets = Vec::new();
                while let Some(prefix) = seq.next_element::<AwsPrefix<'de>>()? {
                    if prefix.service == CLOUDFRONT_SERVICE {
                        nets.push(prefix.ip_prefix.parse().map_err(serde::de::Error::custom)?);
                    }
                }
                Ok(CloudFrontOnly(nets))
            }
        }
        deserializer.deserialize_seq(Visit)
    }
}

fn check_prefixes(nets: Vec<IpNet>) -> anyhow::Result<Ranges> {
    let v4 = nets.iter().filter(|n| matches!(n, IpNet::V4(_))).count();
    anyhow::ensure!(
        v4 >= MIN_IPV4_PREFIXES,
        "{CLOUDFRONT_SERVICE}: only {v4} IPv4 prefixes"
    );
    // 아무 주소나 열리는 대역(/0 등)이 섞인 목록은 믿지 않는다
    anyhow::ensure!(
        nets.iter()
            .all(|n| n.prefix_len() >= if n.addr().is_ipv4() { 8 } else { 16 }),
        "{CLOUDFRONT_SERVICE}: prefix too wide"
    );
    Ok(Ranges::from_nets(&nets))
}

/// ip-ranges.json은 지금 2.7 MB: 이보다 크면 받지 않는다
const MAX_RANGES_BYTES: usize = 16 * 1024 * 1024;

async fn refresh_ranges(allow: Arc<Allow>) {
    let client = match crate::external_http::client_builder(false)
        .timeout(Duration::from_secs(30))
        .build()
    {
        Ok(c) => c,
        Err(error) => {
            tracing::error!(%error, "CloudFront IP ranges: no HTTP client; using the built-in snapshot");
            return;
        }
    };
    loop {
        let wait = match fetch_ranges(&client).await {
            Ok(ranges) => {
                tracing::info!(
                    prefixes = ranges.prefixes,
                    intervals = ?ranges.intervals(),
                    "CloudFront origin-facing IP ranges updated"
                );
                allow.replace(ranges);
                RANGES_REFRESH
            }
            Err(error) => {
                tracing::warn!(%error, "CloudFront IP ranges not updated; keeping the current list");
                RANGES_RETRY
            }
        };
        tokio::time::sleep(wait).await;
    }
}

/// 본문은 한 버퍼에 이어 붙인다 (Content-Length만큼 미리 잡아 다시 복사하지 않는다)
async fn fetch_ranges(client: &reqwest::Client) -> anyhow::Result<Ranges> {
    let mut response = client
        .get(AWS_IP_RANGES_URL)
        .send()
        .await?
        .error_for_status()?;
    let hint = response.content_length().unwrap_or(0) as usize;
    let mut body = Vec::with_capacity(hint.min(MAX_RANGES_BYTES));
    while let Some(chunk) = response.chunk().await? {
        anyhow::ensure!(
            body.len() + chunk.len() <= MAX_RANGES_BYTES,
            "ip-ranges.json larger than {MAX_RANGES_BYTES} bytes"
        );
        body.extend_from_slice(&chunk);
    }
    let ranges = tokio::task::spawn_blocking(move || parse_aws_ranges(&body)).await??;
    Ok(ranges)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn snapshot_nets() -> Vec<IpNet> {
        serde_json::from_str::<Snapshot>(SNAPSHOT).unwrap().prefixes
    }

    #[test]
    fn snapshot_is_cloudfront_origin_facing_only() {
        let allow = Allow::cloudfront();
        assert!(allow.len() >= MIN_IPV4_PREFIXES);
        // 스냅샷의 첫 대역 안쪽 주소는 받는다
        let first = snapshot_nets()[0];
        assert!(allow.permits(first.addr()));
        for outside in [
            "127.0.0.1",
            "10.0.0.5",
            "8.8.8.8",
            "1.1.1.1",
            "::1",
            "2001:db8::1",
        ] {
            assert!(!allow.permits(outside.parse().unwrap()), "{outside}");
        }
    }

    #[test]
    fn ipv4_mapped_peers_are_checked_as_ipv4() {
        let allow = Allow::cloudfront();
        let first = snapshot_nets()
            .into_iter()
            .find_map(|n| match n {
                IpNet::V4(v4) => Some(v4.addr()),
                IpNet::V6(_) => None,
            })
            .unwrap();
        assert!(allow.permits(IpAddr::V6(first.to_ipv6_mapped())));
        assert!(!allow.permits("::ffff:8.8.8.8".parse().unwrap()));
    }

    #[test]
    fn any_allows_everyone() {
        assert!(Allow::any().permits("8.8.8.8".parse().unwrap()));
    }

    fn aws_json(v4: &[(&str, &str)], v6: &[(&str, &str)]) -> String {
        let p4: Vec<_> = v4
            .iter()
            .map(|(p, s)| serde_json::json!({"ip_prefix": p, "service": s, "region": "GLOBAL"}))
            .collect();
        let p6: Vec<_> = v6
            .iter()
            .map(|(p, s)| serde_json::json!({"ipv6_prefix": p, "service": s, "region": "GLOBAL"}))
            .collect();
        serde_json::json!({"syncToken": "1", "createDate": "x", "prefixes": p4, "ipv6_prefixes": p6}).to_string()
    }

    #[test]
    fn aws_ranges_keep_only_the_origin_facing_service() {
        let mut v4: Vec<(String, &str)> = (0..12)
            .map(|i| (format!("130.176.{i}.0/24"), CLOUDFRONT_SERVICE))
            .collect();
        v4.push(("52.95.0.0/16".into(), "AMAZON"));
        v4.push(("120.52.22.96/27".into(), "CLOUDFRONT"));
        let v4: Vec<(&str, &str)> = v4.iter().map(|(p, s)| (p.as_str(), *s)).collect();
        let json = aws_json(
            &v4,
            &[
                ("2600:9000:1000::/36", CLOUDFRONT_SERVICE),
                ("2600:1f00::/24", "AMAZON"),
            ],
        );
        let ranges = parse_aws_ranges(json.as_bytes()).unwrap();
        assert_eq!(ranges.prefixes, 13);
        // 130.176.0.0/24 … 130.176.11.0/24는 맞닿아 한 구간이 된다
        assert_eq!(ranges.intervals(), (1, 1));
        let allow = Allow::any_with(ranges);
        assert!(allow.permits("130.176.3.9".parse().unwrap()));
        assert!(allow.permits("2600:9000:1000::1".parse().unwrap()));
        assert!(
            !allow.permits("52.95.1.1".parse().unwrap()),
            "other AWS services"
        );
        assert!(
            !allow.permits("120.52.22.100".parse().unwrap()),
            "CloudFront edge (viewer side)"
        );
    }

    #[test]
    fn suspicious_range_lists_are_rejected() {
        let few = aws_json(&[("130.176.0.0/24", CLOUDFRONT_SERVICE)], &[]);
        assert!(
            parse_aws_ranges(few.as_bytes()).is_err(),
            "too few prefixes"
        );
        let mut wide: Vec<(String, &str)> = (0..12)
            .map(|i| (format!("130.176.{i}.0/24"), CLOUDFRONT_SERVICE))
            .collect();
        wide.push(("0.0.0.0/0".into(), CLOUDFRONT_SERVICE));
        let wide: Vec<(&str, &str)> = wide.iter().map(|(p, s)| (p.as_str(), *s)).collect();
        assert!(
            parse_aws_ranges(aws_json(&wide, &[]).as_bytes()).is_err(),
            "/0"
        );
        assert!(parse_aws_ranges(b"<html>").is_err());
    }

    impl Allow {
        fn any_with(ranges: Ranges) -> Self {
            Self {
                ranges: Some(RwLock::new(ranges)),
            }
        }
    }

    /// 합친 구간 이분 탐색이 대역을 하나씩 보는 것과 같은 답을 내는지: 모든 대역의 경계 안팎
    #[test]
    fn merged_ranges_answer_like_a_linear_scan() {
        let mut nets = snapshot_nets();
        // 겹침·맞닿음·포함·끝 주소
        for extra in [
            "10.0.0.0/24",
            "10.0.1.0/24",
            "10.0.0.128/25",
            "255.255.255.0/24",
            "ffff::/16",
            "2001:db8::/32",
            "2001:db9::/32",
        ] {
            nets.push(extra.parse().unwrap());
        }
        let ranges = Ranges::from_nets(&nets);
        let linear = |ip: IpAddr| nets.iter().any(|n| n.contains(&ip));
        let mut probes: Vec<IpAddr> = Vec::new();
        for n in &nets {
            match n {
                IpNet::V4(n) => {
                    let (lo, hi) = (u32::from(n.network()), u32::from(n.broadcast()));
                    for v in [
                        lo.wrapping_sub(1),
                        lo,
                        lo + (hi - lo) / 2,
                        hi,
                        hi.wrapping_add(1),
                    ] {
                        probes.push(IpAddr::V4(v.into()));
                    }
                }
                IpNet::V6(n) => {
                    let (lo, hi) = (u128::from(n.network()), u128::from(n.broadcast()));
                    for v in [
                        lo.wrapping_sub(1),
                        lo,
                        lo + (hi - lo) / 2,
                        hi,
                        hi.wrapping_add(1),
                    ] {
                        probes.push(IpAddr::V6(v.into()));
                    }
                }
            }
        }
        probes.extend(
            [
                "0.0.0.0",
                "255.255.255.255",
                "::",
                "ffff:ffff:ffff:ffff:ffff:ffff:ffff:ffff",
            ]
            .map(|s| s.parse::<IpAddr>().unwrap()),
        );
        for ip in probes {
            assert_eq!(ranges.contains(ip), linear(ip), "{ip}");
        }
        let (v4, v6) = ranges.intervals();
        assert!(v4 + v6 < nets.len(), "adjacent and nested prefixes merge");
    }

    #[test]
    fn settings_require_certificate_files_and_cloudfront_in_production() {
        let settings = |pairs: &[(&str, &str)]| {
            let pairs: Vec<(String, String)> = pairs
                .iter()
                .map(|(k, v)| (k.to_string(), v.to_string()))
                .collect();
            Settings::from_values(move |name| {
                pairs
                    .iter()
                    .find(|(k, _)| k == name)
                    .map(|(_, v)| v.clone())
            })
        };
        const BIND: (&str, &str) = ("TLS_BIND", "0.0.0.0:8443");
        const CERT: (&str, &str) = ("TLS_CERT_FILE", "/tls/fullchain.pem");
        const KEY: (&str, &str) = ("TLS_KEY_FILE", "/tls/privkey.pem");
        const ORIGIN: (&str, &str) = ("TLS_ORIGIN_DOMAIN", "api-origin.audeniq.com");
        let files = [BIND, CERT, KEY, ORIGIN];
        assert!(settings(&[]).unwrap().is_none(), "off without TLS_BIND");
        let acme = settings(&[
            BIND,
            CERT,
            KEY,
            ("TLS_ACME_DOMAIN", "api-origin.audeniq.com"),
        ])
        .unwrap()
        .unwrap();
        assert!(
            matches!(acme.certificates, Certificates::Acme(_)),
            "Let's Encrypt wins over the compose default files"
        );
        assert_eq!(
            acme.hosts.origin(),
            "api-origin.audeniq.com",
            "origin = Let's Encrypt name"
        );
        let files_mode = settings(&files).unwrap().unwrap();
        assert!(matches!(
            files_mode.certificates,
            Certificates::Files { .. }
        ));
        assert_eq!(
            files_mode.sources,
            Sources::CloudFront,
            "CloudFront by default"
        );
        assert!(
            settings(&[BIND, ORIGIN]).is_err(),
            "needs Let's Encrypt or certificate files"
        );
        assert!(settings(&[BIND, CERT, ORIGIN]).is_err(), "key file missing");
        assert!(
            settings(&[BIND, CERT, KEY]).is_err(),
            "needs the origin name"
        );
        assert!(
            settings(&[BIND, ORIGIN, ("TLS_ACME_DOMAIN", "other.audeniq.com")]).is_err(),
            "origin and Let's Encrypt names must match"
        );
        assert!(settings(&[BIND, CERT, KEY, ORIGIN, ("TLS_ALLOWED_SOURCES", "everyone")]).is_err());
        let any = [BIND, CERT, KEY, ORIGIN, ("TLS_ALLOWED_SOURCES", "any")];
        assert_eq!(settings(&any).unwrap().unwrap().sources, Sources::Any);
        let production = [
            BIND,
            CERT,
            KEY,
            ORIGIN,
            ("TLS_ALLOWED_SOURCES", "any"),
            ("APP_ENV", "production"),
        ];
        assert!(
            settings(&production).is_err(),
            "production must not accept every source"
        );

        // 오리진 이름 + 예외로 스튜디오·관리자 주소, TLS_EXTRA_HOSTS
        let hosts = settings(&[
            BIND,
            CERT,
            KEY,
            ("TLS_ORIGIN_DOMAIN", "API-Origin.audeniq.com."),
            ("APP_ORIGIN", "https://studio.audeniq.com"),
            ("ADMIN_ORIGIN", "https://admin.audeniq.com"),
            ("TLS_EXTRA_HOSTS", " ops.audeniq.com, ,studio.audeniq.com"),
        ])
        .unwrap()
        .unwrap()
        .hosts;
        assert_eq!(
            &*hosts.0,
            [
                "api-origin.audeniq.com",
                "studio.audeniq.com",
                "admin.audeniq.com",
                "ops.audeniq.com"
            ]
        );
    }

    #[test]
    fn only_origin_names_are_allowed() {
        let hosts =
            OriginHosts::new(["api-origin.audeniq.com".into(), "studio.audeniq.com".into()]);
        for ok in [
            "api-origin.audeniq.com",
            "API-ORIGIN.audeniq.com",
            "api-origin.audeniq.com:443",
            "api-origin.audeniq.com.",
            "studio.audeniq.com",
        ] {
            assert!(hosts.allows(ok), "{ok}");
        }
        for bad in [
            "",
            "203.0.113.10",
            "203.0.113.10:443",
            "[2001:db8::1]:443",
            "audeniq.com",
            "evil.com",
            "api-origin.audeniq.com.evil.com",
            "xapi-origin.audeniq.com",
            "api-origin.audeniq.com:x",
        ] {
            assert!(!hosts.allows(bad), "{bad}");
        }
    }

    const CERT_A: &str = include_str!("../tests/fixtures/tls/a.crt");
    const KEY_A: &str = include_str!("../tests/fixtures/tls/a.key");
    const CERT_B: &str = include_str!("../tests/fixtures/tls/b.crt");
    const KEY_B: &str = include_str!("../tests/fixtures/tls/b.key");

    fn temp_dir(name: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("audeniq-edge-tls-{name}-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn write_pair(dir: &Path, cert: &str, key: &str) -> (PathBuf, PathBuf) {
        let (c, k) = (dir.join("fullchain.pem"), dir.join("privkey.pem"));
        std::fs::write(&c, cert).unwrap();
        std::fs::write(&k, key).unwrap();
        (c, k)
    }

    fn der(pem: &str) -> Vec<u8> {
        CertificateDer::pem_slice_iter(pem.as_bytes())
            .next()
            .unwrap()
            .unwrap()
            .to_vec()
    }

    #[test]
    fn certificate_reloads_only_when_the_files_change_and_stay_valid() {
        let provider = rustls::crypto::aws_lc_rs::default_provider();
        let dir = temp_dir("reload");
        let (c, k) = write_pair(&dir, CERT_A, KEY_A);
        let store = CertStore::from_files(&c, &k, &provider).unwrap();
        let served = |s: &CertStore| s.leaf().unwrap().to_vec();
        assert_eq!(served(&store), der(CERT_A));
        assert!(!store.reload(&provider).unwrap(), "unchanged files");

        // 인증서만 바뀌고 키는 예전 것: 짝이 안 맞으니 예전 인증서를 계속 쓴다
        std::fs::write(&c, CERT_B).unwrap();
        assert!(store.reload(&provider).is_err());
        assert_eq!(served(&store), der(CERT_A));

        std::fs::write(&k, KEY_B).unwrap();
        assert!(store.reload(&provider).unwrap());
        assert_eq!(served(&store), der(CERT_B));

        std::fs::write(&c, "not a certificate").unwrap();
        assert!(store.reload(&provider).is_err());
        assert_eq!(served(&store), der(CERT_B));
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn startup_refuses_a_broken_or_mismatched_pair() {
        let provider = rustls::crypto::aws_lc_rs::default_provider();
        let dir = temp_dir("broken");
        let (c, k) = write_pair(&dir, CERT_A, KEY_B);
        assert!(
            CertStore::from_files(&c, &k, &provider).is_err(),
            "mismatched key"
        );
        assert!(CertStore::from_files(&dir.join("missing.pem"), &k, &provider).is_err());
        std::fs::remove_dir_all(dir).unwrap();
    }

    /// HTTPS로 GET 한 번: (상태 줄, 서버 인증서)
    async fn https_get(addr: SocketAddr, root: &str) -> std::io::Result<(String, Vec<u8>)> {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let mut roots = rustls::RootCertStore::empty();
        roots
            .add(CertificateDer::from_pem_slice(root.as_bytes()).unwrap())
            .unwrap();
        let mut config = rustls::ClientConfig::builder_with_provider(Arc::new(
            rustls::crypto::aws_lc_rs::default_provider(),
        ))
        .with_safe_default_protocol_versions()
        .unwrap()
        .with_root_certificates(roots)
        .with_no_client_auth();
        // CloudFront처럼 http/1.1만 내미는 클라이언트
        config.alpn_protocols = vec![b"http/1.1".to_vec()];
        let connector = tokio_rustls::TlsConnector::from(Arc::new(config));
        let tcp = TcpStream::connect(addr).await?;
        let name = rustls::pki_types::ServerName::try_from("localhost").unwrap();
        let mut tls = connector.connect(name, tcp).await?;
        let peer = tls.get_ref().1.peer_certificates().unwrap()[0].to_vec();
        assert_eq!(tls.get_ref().1.alpn_protocol(), Some(&b"http/1.1"[..]));
        tls.write_all(b"GET /ping HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n")
            .await?;
        let mut body = Vec::new();
        tls.read_to_end(&mut body).await?;
        let text = String::from_utf8_lossy(&body);
        Ok((text.lines().next().unwrap_or("").to_owned(), peer))
    }

    async fn serve(sources: Sources) -> (SocketAddr, PathBuf) {
        serve_for(sources, "localhost").await
    }

    async fn serve_for(sources: Sources, origin: &str) -> (SocketAddr, PathBuf) {
        let dir = temp_dir("serve");
        let (cert, key) = write_pair(&dir, CERT_A, KEY_A);
        let listener = Settings {
            bind: "127.0.0.1:0".into(),
            certificates: Certificates::Files { cert, key },
            sources,
            hosts: OriginHosts::new([origin.into()]),
        }
        .bind()
        .await
        .unwrap();
        let addr = axum::serve::Listener::local_addr(&listener).unwrap();
        let app = Router::new().route("/ping", axum::routing::get(|| async { "pong" }));
        tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        (addr, dir)
    }

    #[tokio::test]
    async fn serves_https_with_the_configured_certificate() {
        let (addr, dir) = serve(Sources::Any).await;
        let (status, peer) = https_get(addr, CERT_A).await.unwrap();
        assert_eq!(status, "HTTP/1.1 200 OK");
        assert_eq!(peer, der(CERT_A));
        // 다른 인증서를 믿는 클라이언트는 핸드셰이크에서 거절한다
        assert!(https_get(addr, CERT_B).await.is_err());
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[tokio::test]
    async fn offers_h2_to_clients_that_ask_for_it() {
        let (addr, dir) = serve(Sources::Any).await;
        let mut roots = rustls::RootCertStore::empty();
        roots
            .add(CertificateDer::from_pem_slice(CERT_A.as_bytes()).unwrap())
            .unwrap();
        let mut config = rustls::ClientConfig::builder_with_provider(Arc::new(
            rustls::crypto::aws_lc_rs::default_provider(),
        ))
        .with_safe_default_protocol_versions()
        .unwrap()
        .with_root_certificates(roots)
        .with_no_client_auth();
        config.alpn_protocols = vec![b"h2".to_vec(), b"http/1.1".to_vec()];
        let tcp = TcpStream::connect(addr).await.unwrap();
        let name = rustls::pki_types::ServerName::try_from("localhost").unwrap();
        let mut tls = tokio_rustls::TlsConnector::from(Arc::new(config))
            .connect(name, tcp)
            .await
            .unwrap();
        assert_eq!(tls.get_ref().1.alpn_protocol(), Some(&b"h2"[..]));
        // h2 연결 서문 + 빈 SETTINGS → 서버도 SETTINGS 프레임(type 4)으로 답한다
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        tls.write_all(b"PRI * HTTP/2.0\r\n\r\nSM\r\n\r\n\0\0\0\x04\0\0\0\0\0")
            .await
            .unwrap();
        let mut frame = [0u8; 9];
        tls.read_exact(&mut frame).await.unwrap();
        assert_eq!(frame[3], 4, "SETTINGS");
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[tokio::test]
    async fn connections_for_another_name_are_closed_before_the_certificate() {
        // 인증서는 localhost용이지만 이 리스너의 오리진 이름은 api-origin.audeniq.test: SNI localhost는 거절
        let (addr, dir) = serve_for(Sources::Any, "api-origin.audeniq.test").await;
        assert!(https_get(addr, CERT_A).await.is_err());
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[tokio::test]
    async fn connections_from_outside_cloudfront_are_closed_before_tls() {
        // 127.0.0.1은 CloudFront 대역이 아니다
        let (addr, dir) = serve(Sources::CloudFront).await;
        assert!(https_get(addr, CERT_A).await.is_err());
        std::fs::remove_dir_all(dir).unwrap();
    }
}
