//! 백엔드 HTTPS(crate::edge_tls) 인증서를 api가 Let's Encrypt에서 직접 받고 자동으로 갱신한다
//!
//! - 켜기: `TLS_ACME_DOMAIN=origin.audeniq.com` (CloudFront `ApiOriginDomain`과 같은 이름, DNS는 이 서버 IP).
//!   `TLS_ACME_EMAIL`(선택)은 Let's Encrypt 계정 연락처. `TLS_ACME_DIRECTORY`는 production(기본)·staging·ACME URL.
//! - 검증: TLS-ALPN-01(RFC 8737). Let's Encrypt가 이 서버 443에 ALPN `acme-tls/1`로 붙으면 검증용 자체 서명
//!   인증서로 답한다. 그래서 80 포트·DNS API 토큰이 필요 없다. 검증 연결은 CloudFront 대역 밖에서 오므로
//!   인증서를 받는 몇 초 동안만, 그 도메인 SNI의 `acme-tls/1` 핸드셰이크만 받고 바로 닫는다(요청은 받지 않는다).
//!   서버 방화벽이 443을 CloudFront 대역으로만 열어 두면 검증이 실패한다: 443은 열어 두고 거르기는 api에 맡긴다.
//! - 저장: `TLS_ACME_DIR`(기본 /var/lib/audeniq-tls, compose 볼륨) 아래 `<ACME 서버>/<도메인>/`에
//!   `fullchain.pem`·`privkey.pem`(0600), 계정 키는 `<ACME 서버>/account.json`. 재시작하면 저장된 인증서로 바로 뜬다.
//! - 갱신: 12시간마다 보고, 유효 기간의 마지막 1/3에 들어서면 새로 받는다(90일 인증서면 만료 30일 전, 더 짧은
//!   인증서도 같은 비율). 실패하면 5분부터 두 배씩(최대 6시간) 다시 하고, 그동안은 지금 인증서를 계속 쓴다.
use crate::edge_tls::CertStore;
use chrono::{DateTime, NaiveDate, Utc};
use instant_acme::{
    Account, AccountCredentials, AuthorizationStatus, ChallengeType, Identifier, LetsEncrypt,
    NewAccount, NewOrder, OrderStatus, RetryPolicy,
};
use rustls::{
    ServerConfig,
    crypto::CryptoProvider,
    pki_types::{CertificateDer, PrivateKeyDer, PrivatePkcs8KeyDer},
    server::ClientHello,
};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, RwLock};
use std::time::Duration;

/// RFC 8737 §6.2
const ACME_TLS_ALPN: &[u8] = b"acme-tls/1";
const DEFAULT_DIR: &str = "/var/lib/audeniq-tls";
const CHECK_INTERVAL: Duration = Duration::from_secs(12 * 3600);
const FIRST_RETRY: Duration = Duration::from_secs(300);
const MAX_RETRY: Duration = Duration::from_secs(6 * 3600);
/// 검증·발급을 기다리는 시간 (Let's Encrypt는 보통 몇 초)
const ORDER_WAIT: Duration = Duration::from_secs(120);

#[derive(Clone, Debug)]
pub struct Acme {
    domain: String,
    contact: Option<String>,
    directory_url: String,
    root: PathBuf,
    /// 시험용 ACME 서버(Pebble 등)의 루트 CA. production에서는 쓸 수 없다
    ca_file: Option<PathBuf>,
}

impl Acme {
    /// `TLS_ACME_DOMAIN`이 비어 있으면 Let's Encrypt를 쓰지 않는다.
    pub(crate) fn from_values(
        get: &impl Fn(&str) -> Option<String>,
    ) -> anyhow::Result<Option<Self>> {
        let Some(domain) = get("TLS_ACME_DOMAIN") else {
            return Ok(None);
        };
        let domain = domain.to_ascii_lowercase();
        anyhow::ensure!(
            domain.contains('.')
                && domain.len() <= 253
                && domain.split('.').all(|label| {
                    !label.is_empty()
                        && label.len() <= 63
                        && !label.starts_with('-')
                        && !label.ends_with('-')
                        && label
                            .bytes()
                            .all(|b| b.is_ascii_alphanumeric() || b == b'-')
                }),
            "TLS_ACME_DOMAIN must be one DNS name (no wildcard), not {domain:?}"
        );
        let contact = match get("TLS_ACME_EMAIL") {
            None => None,
            Some(email) => {
                anyhow::ensure!(
                    email.contains('@')
                        && !email.contains(char::is_whitespace)
                        && !email.contains(':'),
                    "TLS_ACME_EMAIL is not an email address"
                );
                Some(format!("mailto:{email}"))
            }
        };
        let directory_url = match get("TLS_ACME_DIRECTORY").as_deref() {
            None | Some("production") => LetsEncrypt::Production.url().to_owned(),
            Some("staging") => LetsEncrypt::Staging.url().to_owned(),
            Some(url) => {
                crate::external_http::validate_url(url, false)?;
                url.to_owned()
            }
        };
        let ca_file = get("TLS_ACME_CA_FILE").map(PathBuf::from);
        anyhow::ensure!(
            ca_file.is_none() || get("APP_ENV").as_deref() != Some("production"),
            "TLS_ACME_CA_FILE is for test ACME servers only"
        );
        Ok(Some(Self {
            domain,
            contact,
            directory_url,
            root: get("TLS_ACME_DIR")
                .unwrap_or_else(|| DEFAULT_DIR.into())
                .into(),
            ca_file,
        }))
    }

    /// ACME 서버마다 따로 (staging 인증서가 production 것으로 쓰이지 않게)
    fn server_dir(&self) -> PathBuf {
        let name = if self.directory_url == LetsEncrypt::Production.url() {
            "letsencrypt".to_owned()
        } else if self.directory_url == LetsEncrypt::Staging.url() {
            "letsencrypt-staging".to_owned()
        } else {
            let digest = <sha2::Sha256 as sha2::Digest>::digest(self.directory_url.as_bytes());
            format!("acme-{}", &crate::hex::encode(digest)[..12])
        };
        self.root.join(name)
    }

    fn cert_dir(&self) -> PathBuf {
        self.server_dir().join(&self.domain)
    }

    /// 지난번에 받아 둔 인증서로 시작한다. 없거나 깨졌으면 빈 채로(받을 때까지 핸드셰이크 실패).
    pub(crate) fn load_stored(&self, provider: &CryptoProvider) -> CertStore {
        let store = CertStore::empty();
        let dir = self.cert_dir();
        match (
            std::fs::read(dir.join("fullchain.pem")),
            std::fs::read(dir.join("privkey.pem")),
        ) {
            (Ok(chain), Ok(key)) => match store.install(&chain, &key, provider) {
                Ok(()) => tracing::info!(
                    domain = %self.domain,
                    not_after = ?store.leaf().and_then(|c| validity(&c)).map(|v| v.1),
                    "Let's Encrypt certificate loaded"
                ),
                Err(error) => {
                    tracing::warn!(%error, dir = %dir.display(), "stored certificate not usable; requesting a new one")
                }
            },
            _ => {
                tracing::info!(domain = %self.domain, "no stored certificate yet; requesting one from Let's Encrypt")
            }
        }
        store
    }

    async fn account(&self) -> anyhow::Result<Account> {
        let builder = || match &self.ca_file {
            Some(ca) => Account::builder_with_root(ca),
            None => Account::builder(),
        };
        let path = self.server_dir().join("account.json");
        if let Ok(saved) = std::fs::read(&path) {
            let credentials: AccountCredentials = serde_json::from_slice(&saved)?;
            return Ok(builder()?.from_credentials(credentials).await?);
        }
        let contact: Vec<&str> = self.contact.iter().map(String::as_str).collect();
        let (account, credentials) = builder()?
            .create(
                &NewAccount {
                    contact: &contact,
                    terms_of_service_agreed: true,
                    only_return_existing: false,
                },
                self.directory_url.clone(),
                None,
            )
            .await?;
        write_private(&path, &serde_json::to_vec(&credentials)?)?;
        tracing::info!(directory = %self.directory_url, "ACME account created");
        Ok(account)
    }

    /// 주문 → TLS-ALPN-01 검증 → 발급. (전체 체인 PEM, 개인 키 PEM)
    async fn issue(
        &self,
        challenges: &Arc<Challenges>,
        provider: &Arc<CryptoProvider>,
    ) -> anyhow::Result<(String, String)> {
        let account = self.account().await?;
        let identifiers = [Identifier::Dns(self.domain.clone())];
        let mut order = account.new_order(&NewOrder::new(&identifiers)).await?;
        let mut answering = Vec::new();
        {
            // 검증 받을 동안만 답한다: 아래에서 주문이 Ready가 되면 answering을 버린다
            let mut authorizations = order.authorizations();
            while let Some(authorization) = authorizations.next().await {
                let mut authorization = authorization?;
                match authorization.status {
                    AuthorizationStatus::Pending => {}
                    AuthorizationStatus::Valid => continue,
                    other => anyhow::bail!("ACME authorization is {other:?}"),
                }
                let mut challenge = authorization
                    .challenge(ChallengeType::TlsAlpn01)
                    .ok_or_else(|| {
                        anyhow::anyhow!("the ACME server offers no tls-alpn-01 challenge")
                    })?;
                let name = challenge.identifier().to_string();
                let config = challenge_config(
                    &name,
                    challenge.key_authorization().digest().as_ref(),
                    provider,
                )?;
                answering.push(challenges.answer(&name, config));
                challenge.set_ready().await?;
            }
        }
        let wait = RetryPolicy::new()
            .initial_delay(Duration::from_secs(1))
            .timeout(ORDER_WAIT);
        let status = order.poll_ready(&wait).await?;
        drop(answering);
        if status != OrderStatus::Ready {
            let mut reasons = Vec::new();
            let mut authorizations = order.authorizations();
            while let Some(Ok(authorization)) = authorizations.next().await {
                reasons.extend(
                    authorization
                        .challenges
                        .iter()
                        .filter_map(|c| c.error.as_ref().map(|e| e.to_string())),
                );
            }
            anyhow::bail!("ACME order is {status:?}: {}", reasons.join("; "));
        }
        let key_pem = order.finalize().await?;
        let chain_pem = order.poll_certificate(&wait).await?;
        Ok((chain_pem, key_pem))
    }

    fn save(&self, chain_pem: &str, key_pem: &str) -> anyhow::Result<()> {
        let dir = self.cert_dir();
        write_private(&dir.join("privkey.pem"), key_pem.as_bytes())?;
        write_private(&dir.join("fullchain.pem"), chain_pem.as_bytes())
    }
}

/// 받아 둔 인증서를 보고, 갱신할 때가 되면 새로 받는다 (edge_tls가 띄운다)
pub(crate) async fn run(
    acme: Acme,
    store: Arc<CertStore>,
    challenges: Arc<Challenges>,
    provider: Arc<CryptoProvider>,
) {
    let mut failures = 0u32;
    loop {
        let now = Utc::now();
        let current = store.leaf().and_then(|c| validity(&c));
        let wait = match current.map(|(from, to)| renew_at(from, to)) {
            Some(at) if at > now => (at - now)
                .to_std()
                .unwrap_or(CHECK_INTERVAL)
                .min(CHECK_INTERVAL),
            _ => match acme
                .issue(&challenges, &provider)
                .await
                .and_then(|(chain, key)| {
                    store.install(chain.as_bytes(), key.as_bytes(), &provider)?;
                    acme.save(&chain, &key)?;
                    Ok(())
                }) {
                Ok(()) => {
                    failures = 0;
                    tracing::info!(
                        domain = %acme.domain,
                        not_after = ?store.leaf().and_then(|c| validity(&c)).map(|v| v.1),
                        "Let's Encrypt certificate issued"
                    );
                    CHECK_INTERVAL
                }
                Err(error) => {
                    failures += 1;
                    let retry = retry_after(failures);
                    tracing::error!(
                        domain = %acme.domain,
                        %error,
                        failures,
                        retry_seconds = retry.as_secs(),
                        not_after = ?current.map(|v| v.1),
                        "Let's Encrypt certificate not issued; keeping the current one"
                    );
                    retry
                }
            },
        };
        tokio::time::sleep(wait).await;
    }
}

/// 유효 기간의 마지막 1/3에 들어서는 때
fn renew_at(not_before: DateTime<Utc>, not_after: DateTime<Utc>) -> DateTime<Utc> {
    not_after - (not_after - not_before) / 3
}

fn retry_after(failures: u32) -> Duration {
    FIRST_RETRY
        .saturating_mul(1u32 << failures.saturating_sub(1).min(16))
        .min(MAX_RETRY)
}

// ---- TLS-ALPN-01 ----

/// 검증 중인 도메인 → 검증용 인증서로 답하는 TLS 설정
#[derive(Default)]
pub struct Challenges {
    map: RwLock<HashMap<String, Arc<ServerConfig>>>,
}

impl Challenges {
    /// 받는 중인 검증이 있나 (없으면 CloudFront 밖의 연결은 ClientHello도 읽지 않는다)
    pub(crate) fn pending(&self) -> bool {
        !self
            .map
            .read()
            .unwrap_or_else(|e| e.into_inner())
            .is_empty()
    }

    pub(crate) fn config(&self, server_name: &str) -> Option<Arc<ServerConfig>> {
        self.map
            .read()
            .unwrap_or_else(|e| e.into_inner())
            .get(&server_name.to_ascii_lowercase())
            .cloned()
    }

    /// 돌려준 값이 살아 있는 동안만 답한다
    fn answer(self: &Arc<Self>, name: &str, config: Arc<ServerConfig>) -> Answering {
        let name = name.to_ascii_lowercase();
        self.map
            .write()
            .unwrap_or_else(|e| e.into_inner())
            .insert(name.clone(), config);
        Answering {
            challenges: self.clone(),
            name,
        }
    }
}

struct Answering {
    challenges: Arc<Challenges>,
    name: String,
}

impl Drop for Answering {
    fn drop(&mut self) {
        self.challenges
            .map
            .write()
            .unwrap_or_else(|e| e.into_inner())
            .remove(&self.name);
    }
}

/// Let's Encrypt의 검증 연결: ALPN에 `acme-tls/1`
pub(crate) fn is_challenge(hello: &ClientHello<'_>) -> bool {
    hello
        .alpn()
        .is_some_and(|mut protocols| protocols.any(|p| p == ACME_TLS_ALPN))
}

/// 도메인 이름과 key authorization 해시(acmeIdentifier 확장)를 넣은 자체 서명 인증서로 답하는 설정
fn challenge_config(
    name: &str,
    digest: &[u8],
    provider: &Arc<CryptoProvider>,
) -> anyhow::Result<Arc<ServerConfig>> {
    let mut params = rcgen::CertificateParams::new(vec![name.to_owned()])?;
    params.custom_extensions = vec![rcgen::CustomExtension::new_acme_identifier(digest)];
    let key = rcgen::KeyPair::generate()?;
    let cert = params.self_signed(&key)?;
    // with_single_cert·CertifiedKey::from_der는 webpki로 인증서를 해석해 critical acmeIdentifier 확장을
    // 거절한다: 키만 불러와 그대로 건넨다 (방금 만든 키라 짝은 맞다)
    let signing = provider
        .key_provider
        .load_private_key(PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from(
            key.serialize_der(),
        )))?;
    let certified = rustls::sign::CertifiedKey::new(vec![cert.der().clone()], signing);
    let mut config = ServerConfig::builder_with_provider(provider.clone())
        .with_safe_default_protocol_versions()?
        .with_no_client_auth()
        .with_cert_resolver(Arc::new(ChallengeCert(Arc::new(certified))));
    config.alpn_protocols = vec![ACME_TLS_ALPN.to_vec()];
    Ok(Arc::new(config))
}

#[derive(Debug)]
struct ChallengeCert(Arc<rustls::sign::CertifiedKey>);

impl rustls::server::ResolvesServerCert for ChallengeCert {
    fn resolve(&self, _hello: ClientHello<'_>) -> Option<Arc<rustls::sign::CertifiedKey>> {
        Some(self.0.clone())
    }
}

// ---- 인증서 유효 기간 (DER) ----

/// X.509 인증서의 (notBefore, notAfter). TBSCertificate의 validity만 읽는다 (RFC 5280 §4.1).
pub(crate) fn validity(cert: &CertificateDer<'_>) -> Option<(DateTime<Utc>, DateTime<Utc>)> {
    let (certificate, _) = der_tlv(cert.as_ref(), 0x30)?;
    let (tbs, _) = der_tlv(certificate, 0x30)?;
    let mut rest = tbs;
    if rest.first() == Some(&0xA0) {
        rest = der_skip(rest)?; // version
    }
    rest = der_skip(rest)?; // serialNumber
    rest = der_skip(rest)?; // signature
    rest = der_skip(rest)?; // issuer
    let (validity, _) = der_tlv(rest, 0x30)?;
    let (not_before, rest) = der_time(validity)?;
    let (not_after, _) = der_time(rest)?;
    Some((not_before, not_after))
}

/// (내용, 나머지)
fn der_tlv(input: &[u8], tag: u8) -> Option<(&[u8], &[u8])> {
    let (&found, rest) = input.split_first()?;
    if found != tag {
        return None;
    }
    let (&first, rest) = rest.split_first()?;
    let (len, rest) = if first < 0x80 {
        (first as usize, rest)
    } else {
        let n = (first & 0x7f) as usize;
        if n == 0 || n > 4 || rest.len() < n {
            return None;
        }
        let len = rest[..n]
            .iter()
            .fold(0usize, |acc, &b| (acc << 8) | b as usize);
        (len, &rest[n..])
    };
    (rest.len() >= len).then(|| rest.split_at(len))
}

fn der_skip(input: &[u8]) -> Option<&[u8]> {
    der_tlv(input, *input.first()?).map(|(_, rest)| rest)
}

/// UTCTime(YYMMDDHHMMSSZ, 50 미만은 20YY) 또는 GeneralizedTime(YYYYMMDDHHMMSSZ)
fn der_time(input: &[u8]) -> Option<(DateTime<Utc>, &[u8])> {
    let tag = *input.first()?;
    let (value, rest) = der_tlv(input, tag)?;
    let text = std::str::from_utf8(value).ok()?.strip_suffix('Z')?;
    let digits = |s: &str| s.parse::<u32>().ok();
    let (year, rest_text) = match (tag, text.len()) {
        (0x17, 12) => {
            let yy = digits(&text[..2])? as i32;
            (if yy < 50 { 2000 + yy } else { 1900 + yy }, &text[2..])
        }
        (0x18, 14) => (digits(&text[..4])? as i32, &text[4..]),
        _ => return None,
    };
    let time = NaiveDate::from_ymd_opt(year, digits(&rest_text[0..2])?, digits(&rest_text[2..4])?)?
        .and_hms_opt(
            digits(&rest_text[4..6])?,
            digits(&rest_text[6..8])?,
            digits(&rest_text[8..10])?,
        )?;
    Some((time.and_utc(), rest))
}

/// 비밀 파일: 0600으로 임시 파일에 쓰고 이름을 바꿔 넣는다 (반쯤 쓴 파일을 읽지 않게)
fn write_private(path: &Path, bytes: &[u8]) -> anyhow::Result<()> {
    use std::io::Write;
    use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt};
    let dir = path
        .parent()
        .ok_or_else(|| anyhow::anyhow!("no parent directory"))?;
    std::fs::DirBuilder::new()
        .recursive(true)
        .mode(0o700)
        .create(dir)?;
    let tmp = path.with_extension("tmp");
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .mode(0o600)
        .open(&tmp)?;
    file.write_all(bytes)?;
    file.sync_all()?;
    std::fs::rename(&tmp, path)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use rustls::pki_types::pem::PemObject;

    fn settings(pairs: &[(&str, &str)]) -> anyhow::Result<Option<Acme>> {
        let get = |name: &str| {
            pairs
                .iter()
                .find(|(k, _)| *k == name)
                .map(|(_, v)| v.to_string())
        };
        Acme::from_values(&get)
    }

    #[test]
    fn settings_choose_the_directory_and_check_names() {
        assert!(settings(&[]).unwrap().is_none());
        let acme = settings(&[("TLS_ACME_DOMAIN", "Origin.Audeniq.com")])
            .unwrap()
            .unwrap();
        assert_eq!(acme.domain, "origin.audeniq.com");
        assert_eq!(acme.directory_url, LetsEncrypt::Production.url());
        assert_eq!(
            acme.cert_dir(),
            PathBuf::from("/var/lib/audeniq-tls/letsencrypt/origin.audeniq.com")
        );
        let staging = settings(&[
            ("TLS_ACME_DOMAIN", "o.audeniq.com"),
            ("TLS_ACME_DIRECTORY", "staging"),
            ("TLS_ACME_DIR", "/d"),
        ])
        .unwrap()
        .unwrap();
        assert_eq!(
            staging.cert_dir(),
            PathBuf::from("/d/letsencrypt-staging/o.audeniq.com")
        );
        let mail = settings(&[
            ("TLS_ACME_DOMAIN", "o.audeniq.com"),
            ("TLS_ACME_EMAIL", "ops@audeniq.com"),
        ])
        .unwrap()
        .unwrap();
        assert_eq!(mail.contact.as_deref(), Some("mailto:ops@audeniq.com"));
        for bad in [
            "*.audeniq.com",
            "localhost",
            "a..b",
            "-a.b",
            "a b.c",
            "a_b.c",
        ] {
            assert!(settings(&[("TLS_ACME_DOMAIN", bad)]).is_err(), "{bad}");
        }
        assert!(
            settings(&[
                ("TLS_ACME_DOMAIN", "o.audeniq.com"),
                ("TLS_ACME_EMAIL", "nope")
            ])
            .is_err()
        );
        assert!(
            settings(&[
                ("TLS_ACME_DOMAIN", "o.audeniq.com"),
                ("TLS_ACME_DIRECTORY", "http://acme.test/dir")
            ])
            .is_err()
        );
        let test_ca = [
            ("TLS_ACME_DOMAIN", "o.audeniq.com"),
            ("TLS_ACME_CA_FILE", "/ca.pem"),
            ("APP_ENV", "production"),
        ];
        assert!(
            settings(&test_ca).is_err(),
            "test CA only outside production"
        );
    }

    fn pem_der(pem: &str) -> CertificateDer<'static> {
        CertificateDer::from_pem_slice(pem.as_bytes()).unwrap()
    }

    #[test]
    fn validity_reads_utc_and_generalized_times() {
        // openssl x509 -noout -dates: notBefore=Oct 27 13:54 2026 GMT (UTCTime), notAfter=2126 (GeneralizedTime)
        let (from, to) = validity(&pem_der(include_str!("../tests/fixtures/tls/a.crt"))).unwrap();
        assert_eq!(from.format("%Y-%m-%d").to_string(), "2026-10-27");
        assert_eq!(to.format("%Y-%m-%d").to_string(), "2126-10-03");
        assert!(validity(&CertificateDer::from(vec![0x30, 0x03, 0x02, 0x01, 0x01])).is_none());
        assert!(validity(&CertificateDer::from(Vec::new())).is_none());
    }

    #[test]
    fn validity_matches_the_dates_a_certificate_was_made_with() {
        let mut params = rcgen::CertificateParams::new(vec!["o.audeniq.com".into()]).unwrap();
        params.not_before = rcgen::date_time_ymd(2049, 12, 31);
        params.not_after = rcgen::date_time_ymd(2050, 3, 31);
        let cert = params
            .self_signed(&rcgen::KeyPair::generate().unwrap())
            .unwrap();
        let (from, to) = validity(cert.der()).unwrap();
        assert_eq!(from.to_rfc3339(), "2049-12-31T00:00:00+00:00");
        assert_eq!(to.to_rfc3339(), "2050-03-31T00:00:00+00:00");
    }

    #[test]
    fn renewal_starts_in_the_last_third_and_retries_back_off() {
        let from = DateTime::parse_from_rfc3339("2026-01-01T00:00:00Z")
            .unwrap()
            .to_utc();
        let to = from + chrono::Duration::days(90);
        assert_eq!(renew_at(from, to), from + chrono::Duration::days(60));
        let short = from + chrono::Duration::days(45);
        assert_eq!(renew_at(from, short), from + chrono::Duration::days(30));
        assert_eq!(retry_after(1), Duration::from_secs(300));
        assert_eq!(retry_after(2), Duration::from_secs(600));
        assert_eq!(retry_after(30), MAX_RETRY);
    }

    #[test]
    fn challenges_are_answered_only_while_the_order_runs() {
        let provider = Arc::new(rustls::crypto::aws_lc_rs::default_provider());
        let challenges = Arc::new(Challenges::default());
        assert!(!challenges.pending());
        let config = challenge_config("o.audeniq.com", &[7; 32], &provider).unwrap();
        assert_eq!(config.alpn_protocols, vec![ACME_TLS_ALPN.to_vec()]);
        let answering = challenges.answer("O.audeniq.com", config);
        assert!(challenges.pending());
        assert!(challenges.config("o.AUDENIQ.com").is_some());
        assert!(challenges.config("other.audeniq.com").is_none());
        drop(answering);
        assert!(!challenges.pending());
    }

    #[test]
    fn stored_certificates_survive_a_restart() {
        let provider = rustls::crypto::aws_lc_rs::default_provider();
        let root = std::env::temp_dir().join(format!("audeniq-acme-{}", uuid::Uuid::new_v4()));
        let acme = settings(&[
            ("TLS_ACME_DOMAIN", "o.audeniq.com"),
            ("TLS_ACME_DIR", root.to_str().unwrap()),
        ])
        .unwrap()
        .unwrap();
        assert!(acme.load_stored(&provider).leaf().is_none());
        acme.save(
            include_str!("../tests/fixtures/tls/a.crt"),
            include_str!("../tests/fixtures/tls/a.key"),
        )
        .unwrap();
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(acme.cert_dir().join("privkey.pem"))
            .unwrap()
            .permissions()
            .mode();
        assert_eq!(mode & 0o777, 0o600);
        let leaf = acme.load_stored(&provider).leaf().unwrap();
        assert_eq!(leaf, pem_der(include_str!("../tests/fixtures/tls/a.crt")));
        std::fs::remove_dir_all(root).unwrap();
    }
}
