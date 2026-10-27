//! 백엔드가 직접 받는 HTTPS — AWS CloudFront가 이 서버를 오리진으로 부른다 (deploy/aws/README.md)
//!
//! 예전에는 Cloudflare Tunnel이 TLS를 끝내고 평문 8080으로 넘겼다. AWS에서는 ALB 없이 api가 인증서를
//! 직접 들고 `TLS_BIND`(컨테이너 8443 → 서버 443)에서 CloudFront의 요청을 받는다.
//!
//! - 인증서: `TLS_CERT_FILE`(전체 체인 PEM)·`TLS_KEY_FILE`. 5분마다 파일을 다시 보고, 바뀌었으면 다음 연결부터
//!   새 인증서를 쓴다(갱신 뒤 재시작 없음, deploy/tls-cert.sh). 새 파일이 깨졌으면 이전 인증서를 계속 쓴다.
//! - 연결 출처: `TLS_ALLOWED_SOURCES=cloudfront`(기본)이면 CloudFront 오리진 쪽 IP 대역
//!   (`CLOUDFRONT_ORIGIN_FACING`)에서 온 연결만 받고, 나머지는 TLS 핸드셰이크 전에 끊는다. 대역은 AWS
//!   ip-ranges.json을 6시간마다 받고, 첫 응답 전에는 저장소 스냅샷(config/cloudfront-origin-facing.json)을 쓴다.
//! - 요청: 이 리스너로 온 요청은 화면 파일까지 모두 서비스 비밀(`x-audeniq-service`, CloudFront origin custom
//!   header)이 있어야 한다. 다른 AWS 계정의 CloudFront가 이 서버를 오리진으로 걸어도 403이다.
//!
//! 평문 `API_BIND`는 컨테이너 healthcheck와 (전환 전까지) 터널용으로 그대로 둔다.
use crate::{api::AppState, error::Error, studio_static};
use axum::{
    Router,
    extract::{Request, State},
    middleware::{self, Next},
    response::{IntoResponse, Response},
};
use ipnet::IpNet;
use rustls::{
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
use tokio_rustls::{TlsAcceptor, server::TlsStream};

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

/// 누가 이 리스너에 연결할 수 있나
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Sources {
    /// CloudFront 오리진 쪽 IP 대역만
    CloudFront,
    /// 아무 주소나 (개발·시험용, production에서는 거절)
    Any,
}

#[derive(Clone, Debug)]
pub struct Settings {
    pub bind: String,
    pub cert_file: PathBuf,
    pub key_file: PathBuf,
    pub sources: Sources,
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
        let cert_file = get("TLS_CERT_FILE")
            .ok_or_else(|| anyhow::anyhow!("TLS_BIND requires TLS_CERT_FILE"))?;
        let key_file =
            get("TLS_KEY_FILE").ok_or_else(|| anyhow::anyhow!("TLS_BIND requires TLS_KEY_FILE"))?;
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
        Ok(Some(Self {
            bind,
            cert_file: cert_file.into(),
            key_file: key_file.into(),
            sources,
        }))
    }

    /// 인증서를 읽고(실패하면 시작하지 않는다) 포트를 연다. 인증서 재로드·IP 대역 갱신 작업도 여기서 띄운다.
    pub async fn bind(self) -> anyhow::Result<TlsListener> {
        let provider = Arc::new(rustls::crypto::aws_lc_rs::default_provider());
        let certs = Arc::new(CertStore::load(&self.cert_file, &self.key_file, &provider)?);
        let mut config = rustls::ServerConfig::builder_with_provider(provider.clone())
            .with_safe_default_protocol_versions()?
            .with_no_client_auth()
            .with_cert_resolver(certs.clone());
        // CloudFront는 오리진에 HTTP/1.1로 붙지만, h2를 내미는 상대(시험용 curl, 이후 gRPC 등)도 받는다
        config.alpn_protocols = vec![b"h2".to_vec(), b"http/1.1".to_vec()];
        let acceptor = TlsAcceptor::from(Arc::new(config));

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
            "HTTPS listener (TLS_BIND)"
        );

        tokio::spawn(reload_certs(certs, provider));
        if self.sources == Sources::CloudFront {
            tokio::spawn(refresh_ranges(allow.clone()));
        }
        let (tx, rx) = tokio::sync::mpsc::channel(64);
        tokio::spawn(accept_loop(tcp, acceptor, allow, tx));
        Ok(TlsListener { rx, local })
    }
}

/// 이 리스너로 서빙할 앱: 화면·API 모두 서비스 비밀이 있어야 한다.
pub fn app(s: AppState, studio: Option<Arc<studio_static::Studio>>) -> Router {
    studio_static::app(s.clone(), studio).layer(middleware::from_fn_with_state(s, require_edge))
}

async fn require_edge(State(s): State<AppState>, req: Request, next: Next) -> Response {
    if !s.edge_authenticated(req.headers()) {
        tracing::warn!(path = %req.uri().path(), "HTTPS request without the edge service secret");
        return Error::Forbidden.into_response();
    }
    next.run(req).await
}

/// 핸드셰이크를 마친 연결을 axum에 넘긴다. 핸드셰이크는 연결마다 따로 돌아서 느린 상대가 다른 연결을 막지 않는다.
pub struct TlsListener {
    rx: tokio::sync::mpsc::Receiver<(TlsStream<TcpStream>, SocketAddr)>,
    local: SocketAddr,
}

impl axum::serve::Listener for TlsListener {
    type Io = TlsStream<TcpStream>;
    type Addr = SocketAddr;

    async fn accept(&mut self) -> (Self::Io, Self::Addr) {
        match self.rx.recv().await {
            Some(connection) => connection,
            // 받는 작업이 끝났으면(일어나지 않음) 새 연결도 없다
            None => std::future::pending().await,
        }
    }

    fn local_addr(&self) -> std::io::Result<Self::Addr> {
        Ok(self.local)
    }
}

async fn accept_loop(
    tcp: tokio::net::TcpListener,
    acceptor: TlsAcceptor,
    allow: Arc<Allow>,
    tx: tokio::sync::mpsc::Sender<(TlsStream<TcpStream>, SocketAddr)>,
) {
    let handshakes = Arc::new(tokio::sync::Semaphore::new(MAX_HANDSHAKES));
    let mut rejected = 0u64;
    let mut last_report = Instant::now();
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
        if !allow.permits(peer.ip()) {
            rejected += 1;
            if rejected == 1 || last_report.elapsed() >= Duration::from_secs(60) {
                tracing::warn!(peer = %peer.ip(), rejected, "HTTPS connection refused: not a CloudFront origin address");
                rejected = 0;
                last_report = Instant::now();
            }
            continue;
        }
        let Ok(permit) = handshakes.clone().try_acquire_owned() else {
            tracing::warn!(peer = %peer.ip(), "HTTPS connection dropped: too many handshakes in progress");
            continue;
        };
        if let Err(error) = stream.set_nodelay(true) {
            tracing::debug!(%error, "TCP_NODELAY not set");
        }
        let acceptor = acceptor.clone();
        let tx = tx.clone();
        tokio::spawn(async move {
            match tokio::time::timeout(HANDSHAKE_TIMEOUT, acceptor.accept(stream)).await {
                Ok(Ok(tls)) => {
                    drop(permit);
                    let _ = tx.send((tls, peer)).await;
                }
                Ok(Err(error)) => {
                    tracing::debug!(peer = %peer.ip(), %error, "TLS handshake failed")
                }
                Err(_) => tracing::debug!(peer = %peer.ip(), "TLS handshake timed out"),
            }
        });
    }
}

// ---- 인증서 ----

/// 지금 쓰는 인증서. 파일이 바뀌면 [`CertStore::reload`]가 바꿔 끼운다.
#[derive(Debug)]
struct CertStore {
    cert_file: PathBuf,
    key_file: PathBuf,
    current: RwLock<(Fingerprint, Arc<CertifiedKey>)>,
}

type Fingerprint = [u8; 32];

impl CertStore {
    fn load(cert: &Path, key: &Path, provider: &CryptoProvider) -> anyhow::Result<Self> {
        let (fingerprint, certified) = read_pair(cert, key, provider)?;
        Ok(Self {
            cert_file: cert.to_owned(),
            key_file: key.to_owned(),
            current: RwLock::new((fingerprint, Arc::new(certified))),
        })
    }

    /// 파일이 바뀌었으면 다시 읽는다. 새로 바꿔 끼웠으면 true.
    fn reload(&self, provider: &CryptoProvider) -> anyhow::Result<bool> {
        let fingerprint = fingerprint(
            &std::fs::read(&self.cert_file)?,
            &std::fs::read(&self.key_file)?,
        );
        if self.current.read().unwrap_or_else(|e| e.into_inner()).0 == fingerprint {
            return Ok(false);
        }
        let (fingerprint, certified) = read_pair(&self.cert_file, &self.key_file, provider)?;
        *self.current.write().unwrap_or_else(|e| e.into_inner()) =
            (fingerprint, Arc::new(certified));
        Ok(true)
    }
}

impl ResolvesServerCert for CertStore {
    fn resolve(&self, _hello: ClientHello<'_>) -> Option<Arc<CertifiedKey>> {
        Some(
            self.current
                .read()
                .unwrap_or_else(|e| e.into_inner())
                .1
                .clone(),
        )
    }
}

fn fingerprint(cert: &[u8], key: &[u8]) -> Fingerprint {
    let mut h = Sha256::new();
    h.update(cert);
    h.update([0]);
    h.update(key);
    h.finalize().into()
}

fn read_pair(
    cert: &Path,
    key: &Path,
    provider: &CryptoProvider,
) -> anyhow::Result<(Fingerprint, CertifiedKey)> {
    let cert_pem = std::fs::read(cert)
        .map_err(|e| anyhow::anyhow!("TLS_CERT_FILE {}: {e}", cert.display()))?;
    let key_pem =
        std::fs::read(key).map_err(|e| anyhow::anyhow!("TLS_KEY_FILE {}: {e}", key.display()))?;
    Ok((
        fingerprint(&cert_pem, &key_pem),
        parse_pair(&cert_pem, &key_pem, provider)?,
    ))
}

fn parse_pair(
    cert_pem: &[u8],
    key_pem: &[u8],
    provider: &CryptoProvider,
) -> anyhow::Result<CertifiedKey> {
    let chain = CertificateDer::pem_slice_iter(cert_pem)
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| anyhow::anyhow!("TLS_CERT_FILE is not PEM: {e}"))?;
    anyhow::ensure!(!chain.is_empty(), "TLS_CERT_FILE has no certificate");
    let key = PrivateKeyDer::from_pem_slice(key_pem)
        .map_err(|e| anyhow::anyhow!("TLS_KEY_FILE is not a PEM private key: {e}"))?;
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

/// 연결을 받을 주소 대역. `None`은 아무 주소나.
struct Allow {
    nets: Option<RwLock<Arc<Vec<IpNet>>>>,
}

impl Allow {
    fn cloudfront() -> Self {
        let nets = parse_snapshot(SNAPSHOT).expect("config/cloudfront-origin-facing.json is valid");
        Self {
            nets: Some(RwLock::new(Arc::new(nets))),
        }
    }

    fn any() -> Self {
        Self { nets: None }
    }

    fn len(&self) -> usize {
        self.nets
            .as_ref()
            .map_or(0, |n| n.read().unwrap_or_else(|e| e.into_inner()).len())
    }

    fn permits(&self, ip: IpAddr) -> bool {
        let Some(nets) = &self.nets else { return true };
        // 듀얼 스택 소켓의 ::ffff:a.b.c.d 는 IPv4로 본다
        let ip = ip.to_canonical();
        nets.read()
            .unwrap_or_else(|e| e.into_inner())
            .iter()
            .any(|n| n.contains(&ip))
    }

    fn replace(&self, new: Vec<IpNet>) {
        if let Some(nets) = &self.nets {
            *nets.write().unwrap_or_else(|e| e.into_inner()) = Arc::new(new);
        }
    }
}

#[derive(serde::Deserialize)]
struct Snapshot {
    prefixes: Vec<IpNet>,
}

fn parse_snapshot(text: &str) -> anyhow::Result<Vec<IpNet>> {
    let snapshot: Snapshot = serde_json::from_str(text)?;
    check_prefixes(snapshot.prefixes)
}

#[derive(serde::Deserialize)]
struct AwsRanges {
    prefixes: Vec<AwsV4>,
    ipv6_prefixes: Vec<AwsV6>,
}
#[derive(serde::Deserialize)]
struct AwsV4 {
    ip_prefix: IpNet,
    service: String,
}
#[derive(serde::Deserialize)]
struct AwsV6 {
    ipv6_prefix: IpNet,
    service: String,
}

/// AWS ip-ranges.json → CloudFront 오리진 쪽 대역
fn parse_aws_ranges(text: &[u8]) -> anyhow::Result<Vec<IpNet>> {
    let ranges: AwsRanges = serde_json::from_slice(text)?;
    let v4 = ranges
        .prefixes
        .into_iter()
        .filter(|p| p.service == CLOUDFRONT_SERVICE)
        .map(|p| p.ip_prefix);
    let v6 = ranges
        .ipv6_prefixes
        .into_iter()
        .filter(|p| p.service == CLOUDFRONT_SERVICE)
        .map(|p| p.ipv6_prefix);
    check_prefixes(v4.chain(v6).collect())
}

fn check_prefixes(mut nets: Vec<IpNet>) -> anyhow::Result<Vec<IpNet>> {
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
    nets.sort();
    nets.dedup();
    Ok(nets)
}

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
            Ok(nets) => {
                tracing::info!(
                    prefixes = nets.len(),
                    "CloudFront origin-facing IP ranges updated"
                );
                allow.replace(nets);
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

async fn fetch_ranges(client: &reqwest::Client) -> anyhow::Result<Vec<IpNet>> {
    let response = client
        .get(AWS_IP_RANGES_URL)
        .send()
        .await?
        .error_for_status()?;
    parse_aws_ranges(&response.bytes().await?)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn snapshot_is_cloudfront_origin_facing_only() {
        let allow = Allow::cloudfront();
        assert!(allow.len() >= MIN_IPV4_PREFIXES);
        // 스냅샷의 첫 대역 안쪽 주소는 받는다
        let first = parse_snapshot(SNAPSHOT).unwrap()[0];
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
        let first = parse_snapshot(SNAPSHOT)
            .unwrap()
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
        let nets = parse_aws_ranges(json.as_bytes()).unwrap();
        assert_eq!(nets.len(), 13);
        let allow = Allow::any_with(nets);
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
        fn any_with(nets: Vec<IpNet>) -> Self {
            Self {
                nets: Some(RwLock::new(Arc::new(nets))),
            }
        }
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
        let files = [
            ("TLS_BIND", "0.0.0.0:8443"),
            ("TLS_CERT_FILE", "/tls/fullchain.pem"),
            ("TLS_KEY_FILE", "/tls/privkey.pem"),
        ];
        assert!(settings(&[]).unwrap().is_none(), "off without TLS_BIND");
        assert!(settings(&files[..2]).is_err(), "key file missing");
        assert_eq!(
            settings(&files).unwrap().unwrap().sources,
            Sources::CloudFront,
            "CloudFront by default"
        );
        assert!(
            settings(&[
                files[0],
                files[1],
                files[2],
                ("TLS_ALLOWED_SOURCES", "everyone")
            ])
            .is_err()
        );
        let any = [files[0], files[1], files[2], ("TLS_ALLOWED_SOURCES", "any")];
        assert_eq!(settings(&any).unwrap().unwrap().sources, Sources::Any);
        let production = [any[0], any[1], any[2], any[3], ("APP_ENV", "production")];
        assert!(
            settings(&production).is_err(),
            "production must not accept every source"
        );
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
        let store = CertStore::load(&c, &k, &provider).unwrap();
        let served = |s: &CertStore| s.current.read().unwrap().1.cert[0].to_vec();
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
            CertStore::load(&c, &k, &provider).is_err(),
            "mismatched key"
        );
        assert!(CertStore::load(&dir.join("missing.pem"), &k, &provider).is_err());
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
        let dir = temp_dir("serve");
        let (cert_file, key_file) = write_pair(&dir, CERT_A, KEY_A);
        let listener = Settings {
            bind: "127.0.0.1:0".into(),
            cert_file,
            key_file,
            sources,
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
    async fn connections_from_outside_cloudfront_are_closed_before_tls() {
        // 127.0.0.1은 CloudFront 대역이 아니다
        let (addr, dir) = serve(Sources::CloudFront).await;
        assert!(https_get(addr, CERT_A).await.is_err());
        std::fs::remove_dir_all(dir).unwrap();
    }
}
