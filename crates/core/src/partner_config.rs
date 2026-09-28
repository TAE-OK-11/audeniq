//! Per-partner delivery configuration (F6).
//!
//! What a signed DSP contract hands over — host, account, key, bucket,
//! webhook secret, feed format — lives outside the database, one JSON file
//! per partner in `PARTNER_CONFIG_DIR` (`D-5.json`, `D-1.json`, ...). The
//! database keeps only the evidence flags (`execution.partner_onboarding`)
//! and the kill switch (`delivery_enabled`); a file alone never enables a
//! send.
//!
//! Secrets are never written inline: every credential field is a reference,
//! `{"env": "NAME"}` or `{"file": "/run/secrets/name"}`, resolved when the
//! file is loaded. A config that names a missing secret fails to load and
//! the partner simply has no adapter (sends fail closed with
//! `EXECUTION_NO_ADAPTER`).
//!
//! Example (`deploy/partners/D-5.example.json` has the full set):
//!
//! ```json
//! {
//!   "partner_id": "D-5",
//!   "adapter": "ddex",
//!   "transport": {"kind": "sftp", "host": "sftp.partner.example", "username": "audeniq",
//!                 "private_key": {"file": "/run/secrets/d5_key"},
//!                 "known_hosts": {"file": "/run/secrets/d5_known_hosts"},
//!                 "remote_root": "/inbox"},
//!   "ddex": {"choreography": "batch", "ack_dir": "/outbox"}
//! }
//! ```
use crate::error::{Error, Result};
use serde::Deserialize;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// A credential reference. Exactly one of `env` / `file`.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SecretRef {
    #[serde(default)]
    pub env: Option<String>,
    #[serde(default)]
    pub file: Option<PathBuf>,
}

/// A resolved secret. `Debug` never prints the value.
#[derive(Clone, Default)]
pub struct Secret(String);

impl Secret {
    pub fn expose(&self) -> &str {
        &self.0
    }
    pub fn from_value(v: impl Into<String>) -> Self {
        Self(v.into())
    }
}

impl std::fmt::Debug for Secret {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Secret(***)")
    }
}

impl SecretRef {
    pub fn resolve(&self) -> Result<Secret> {
        match (&self.env, &self.file) {
            (Some(name), None) => std::env::var(name)
                .ok()
                .filter(|v| !v.is_empty())
                .map(Secret)
                .ok_or(Error::PolicyGate("PARTNER_SECRET_MISSING")),
            (None, Some(path)) => std::fs::read_to_string(path)
                .ok()
                .map(|v| v.trim_end_matches(['\n', '\r']).to_string())
                .filter(|v| !v.is_empty())
                .map(Secret)
                .ok_or(Error::PolicyGate("PARTNER_SECRET_MISSING")),
            _ => Err(Error::PolicyGate("PARTNER_SECRET_REF_INVALID")),
        }
    }

    /// Credentials some tools read from a path (an SSH private key, a
    /// known_hosts file). An env reference is materialized into a private
    /// temp file once per load.
    pub fn resolve_path(&self, label: &str) -> Result<PathBuf> {
        if let (None, Some(p)) = (&self.env, &self.file) {
            if p.is_file() {
                return Ok(p.clone());
            }
            return Err(Error::PolicyGate("PARTNER_SECRET_MISSING"));
        }
        let value = self.resolve()?;
        let dir = std::env::temp_dir().join(format!("audeniq-partner-{}", std::process::id()));
        std::fs::create_dir_all(&dir).map_err(|_| Error::Internal)?;
        let path = dir.join(label);
        write_private(&path, format!("{}\n", value.expose()).as_bytes())?;
        Ok(path)
    }
}

fn write_private(path: &Path, bytes: &[u8]) -> Result<()> {
    use std::io::Write;
    #[cfg(unix)]
    use std::os::unix::fs::OpenOptionsExt;
    let mut o = std::fs::OpenOptions::new();
    o.write(true).create(true).truncate(true);
    #[cfg(unix)]
    o.mode(0o600);
    let mut f = o.open(path).map_err(|_| Error::Internal)?;
    f.write_all(bytes).map_err(|_| Error::Internal)
}

/// Which adapter speaks to the partner.
#[derive(Debug, Clone, Copy, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum AdapterKind {
    /// DDEX ERN 3.8.2 over a file drop (SFTP / S3), ERN choreography.
    Ddex,
    /// Partner-specific metadata feed (JSON + CSV manifest) over a file drop.
    /// The Korean services publish no distributor spec; the manifest mapping
    /// is the part a contract adjusts.
    PartnerSpec,
    /// Partner REST API (JSON release body + file uploads + status polling).
    HttpApi,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum TransportConfig {
    Sftp {
        host: String,
        #[serde(default = "default_ssh_port")]
        port: u16,
        username: String,
        private_key: SecretRef,
        /// Pinned host key(s). Host key checking is always strict.
        known_hosts: SecretRef,
        #[serde(default = "default_root")]
        remote_root: String,
        #[serde(default = "default_connect_timeout")]
        connect_timeout_secs: u64,
        /// Whole-invocation cap (large album uploads).
        #[serde(default = "default_transfer_timeout")]
        transfer_timeout_secs: u64,
    },
    S3 {
        endpoint: String,
        #[serde(default = "default_region")]
        region: String,
        bucket: String,
        #[serde(default)]
        prefix: String,
        access_key_id: SecretRef,
        secret_access_key: SecretRef,
    },
    /// Local directory drop. Only for staging rehearsals and tests: refused
    /// unless `AUDENIQ_ALLOW_LOCAL_PARTNER_TRANSPORT=true`.
    Local { root: PathBuf },
    /// No file drop: the HTTP API adapter carries files itself.
    None,
}

fn default_ssh_port() -> u16 {
    22
}
fn default_root() -> String {
    "/".into()
}
fn default_connect_timeout() -> u64 {
    20
}
fn default_transfer_timeout() -> u64 {
    3600
}
fn default_region() -> String {
    "us-east-1".into()
}

#[derive(Debug, Clone, Copy, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum Choreography {
    /// DDEX ERN choreography, Batch profile: `<batch>/<UPC>/…` then
    /// `BatchComplete_<batch>.xml` once every file is in place.
    #[default]
    Batch,
    /// Release-by-release profile: `<UPC>_<timestamp>/…` with a per-release
    /// completion marker.
    ReleaseByRelease,
}

/// When a delivered release counts as LIVE.
#[derive(Debug, Clone, Copy, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum LivePolicy {
    /// Only an explicit partner signal (status API / live webhook) or staff
    /// evidence marks LIVE. Safe default.
    #[default]
    Explicit,
    /// A successful ingestion ACK means the partner will publish on the
    /// deal start date; the ACK is recorded as LIVE evidence.
    OnAck,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DdexOptions {
    #[serde(default)]
    pub choreography: Choreography,
    /// Where the partner drops acknowledgement files (relative to the
    /// transport root). `None`: ACKs are looked for inside the batch folder.
    #[serde(default)]
    pub ack_dir: Option<String>,
    /// Sub-folder for our batches under the transport root.
    #[serde(default)]
    pub inbox_dir: Option<String>,
    #[serde(default)]
    pub live_policy: LivePolicy,
    /// Resource files go into `resources/` next to the XML (some partners);
    /// default: next to the XML, as the ERN `FilePath` states.
    #[serde(default)]
    pub resources_subdir: bool,
}

impl Default for DdexOptions {
    fn default() -> Self {
        Self {
            choreography: Choreography::Batch,
            ack_dir: None,
            inbox_dir: None,
            live_policy: LivePolicy::Explicit,
            resources_subdir: false,
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PartnerSpecOptions {
    /// Manifest files written next to the audio: `json`, `csv` or both.
    #[serde(default = "default_manifest_formats")]
    pub formats: Vec<String>,
    /// Rename manifest keys to the partner's column names
    /// (`{"isrc": "ISRC코드"}`); unmapped keys keep their names.
    #[serde(default)]
    pub field_map: BTreeMap<String, String>,
    /// Completion marker file name; `{id}` is replaced by the delivery id.
    #[serde(default = "default_marker")]
    pub complete_marker: String,
    /// Directory the partner writes result files to (JSON `{"status":…}`).
    #[serde(default)]
    pub result_dir: Option<String>,
    #[serde(default)]
    pub inbox_dir: Option<String>,
    #[serde(default)]
    pub live_policy: LivePolicy,
}

fn default_manifest_formats() -> Vec<String> {
    vec!["json".into(), "csv".into()]
}
fn default_marker() -> String {
    "{id}.complete".into()
}

impl Default for PartnerSpecOptions {
    fn default() -> Self {
        Self {
            formats: default_manifest_formats(),
            field_map: BTreeMap::new(),
            complete_marker: default_marker(),
            result_dir: None,
            inbox_dir: None,
            live_policy: LivePolicy::Explicit,
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum HttpAuth {
    Bearer {
        token: SecretRef,
    },
    ApiKey {
        header: String,
        key: SecretRef,
    },
    /// OAuth2 client credentials; the token is cached until expiry.
    Oauth2 {
        token_url: String,
        client_id: SecretRef,
        client_secret: SecretRef,
        #[serde(default)]
        scope: Option<String>,
    },
    /// HMAC-SHA256 over `timestamp.method.path.sha256(body)`.
    Hmac {
        key_id: String,
        secret: SecretRef,
        #[serde(default = "default_sig_header")]
        header: String,
    },
}

fn default_sig_header() -> String {
    "X-Signature".into()
}

/// Path templates of a partner REST API. `{id}` = partner submission id,
/// `{name}` = file name, `{key}` = our idempotency key (url-encoded).
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HttpApiConfig {
    pub base_url: String,
    pub auth: HttpAuth,
    #[serde(default = "p_create")]
    pub create_path: String,
    #[serde(default = "p_file")]
    pub file_path: String,
    #[serde(default = "p_commit")]
    pub commit_path: String,
    #[serde(default = "p_status")]
    pub status_path: String,
    #[serde(default = "p_lookup")]
    pub lookup_path: String,
    #[serde(default = "p_takedown")]
    pub takedown_path: String,
    #[serde(default = "p_update")]
    pub update_path: String,
    /// JSON pointer of the submission id in the create response.
    #[serde(default = "p_id_pointer")]
    pub id_pointer: String,
    #[serde(default = "p_status_pointer")]
    pub status_pointer: String,
    #[serde(default = "p_release_pointer")]
    pub release_id_pointer: String,
    /// Partner status value -> ACCEPTED | REJECTED | LIVE | TAKEN_DOWN | PENDING.
    #[serde(default = "default_status_map")]
    pub status_map: BTreeMap<String, String>,
    #[serde(default = "default_http_timeout")]
    pub timeout_secs: u64,
    /// Body carries the DDEX ERN XML instead of the JSON manifest.
    #[serde(default)]
    pub send_ddex_xml: bool,
}

fn p_create() -> String {
    "/v1/releases".into()
}
fn p_file() -> String {
    "/v1/releases/{id}/files/{name}".into()
}
fn p_commit() -> String {
    "/v1/releases/{id}/commit".into()
}
fn p_status() -> String {
    "/v1/releases/{id}".into()
}
fn p_lookup() -> String {
    "/v1/releases?idempotency_key={key}".into()
}
fn p_takedown() -> String {
    "/v1/releases/{id}/takedown".into()
}
fn p_update() -> String {
    "/v1/releases/{id}".into()
}
fn p_id_pointer() -> String {
    "/id".into()
}
fn p_status_pointer() -> String {
    "/status".into()
}
fn p_release_pointer() -> String {
    "/release_id".into()
}
fn default_http_timeout() -> u64 {
    60
}
fn default_status_map() -> BTreeMap<String, String> {
    [
        ("received", "PENDING"),
        ("processing", "PENDING"),
        ("pending", "PENDING"),
        ("accepted", "ACCEPTED"),
        ("ingested", "ACCEPTED"),
        ("rejected", "REJECTED"),
        ("failed", "REJECTED"),
        ("live", "LIVE"),
        ("published", "LIVE"),
        ("taken_down", "TAKEN_DOWN"),
        ("removed", "TAKEN_DOWN"),
    ]
    .into_iter()
    .map(|(a, b)| (a.to_string(), b.to_string()))
    .collect()
}

/// Inbound partner notifications (`POST /api/partner-hooks/{partner_id}`).
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WebhookConfig {
    pub secret: SecretRef,
    /// Header carrying `hex(hmac_sha256(secret, timestamp + "." + body))`
    /// (or of the body alone when `timestamp_header` is null).
    #[serde(default = "default_sig_header")]
    pub signature_header: String,
    #[serde(default = "default_ts_header")]
    pub timestamp_header: Option<String>,
    /// Allowed clock skew for the timestamp, seconds.
    #[serde(default = "default_skew")]
    pub max_skew_secs: i64,
}

fn default_ts_header() -> Option<String> {
    Some("X-Timestamp".into())
}
fn default_skew() -> i64 {
    300
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PartnerConfig {
    pub partner_id: String,
    pub adapter: AdapterKind,
    #[serde(default = "default_transport")]
    pub transport: TransportConfig,
    #[serde(default)]
    pub ddex: DdexOptions,
    #[serde(default)]
    pub partner_spec: PartnerSpecOptions,
    #[serde(default)]
    pub http_api: Option<HttpApiConfig>,
    #[serde(default)]
    pub webhook: Option<WebhookConfig>,
    /// Optional status endpoint for file-drop partners (LIVE polling).
    #[serde(default)]
    pub status_api: Option<HttpApiConfig>,
}

fn default_transport() -> TransportConfig {
    TransportConfig::None
}

pub fn valid_partner_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 64
        && id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
}

impl PartnerConfig {
    pub fn parse(text: &str) -> Result<Self> {
        let c: Self = serde_json::from_str(text).map_err(|e| {
            tracing::warn!(error=%e, "partner config parse failed");
            Error::InvalidCode("PARTNER_CONFIG_INVALID")
        })?;
        c.validate()?;
        Ok(c)
    }

    fn validate(&self) -> Result<()> {
        if !valid_partner_id(&self.partner_id) {
            return Err(Error::InvalidCode("PARTNER_CONFIG_INVALID"));
        }
        match (&self.adapter, &self.transport) {
            (AdapterKind::HttpApi, _) if self.http_api.is_none() => {
                Err(Error::InvalidCode("PARTNER_HTTP_API_MISSING"))
            }
            (AdapterKind::Ddex | AdapterKind::PartnerSpec, TransportConfig::None) => {
                Err(Error::InvalidCode("PARTNER_TRANSPORT_MISSING"))
            }
            (_, TransportConfig::Local { .. })
                if std::env::var("AUDENIQ_ALLOW_LOCAL_PARTNER_TRANSPORT").as_deref()
                    != Ok("true") =>
            {
                Err(Error::InvalidCode("PARTNER_LOCAL_TRANSPORT_DISABLED"))
            }
            _ => Ok(()),
        }?;
        for api in [&self.http_api, &self.status_api].into_iter().flatten() {
            let u = url::Url::parse(&api.base_url)
                .map_err(|_| Error::InvalidCode("PARTNER_URL_INVALID"))?;
            let local_http = u.scheme() == "http"
                && matches!(u.host_str(), Some("127.0.0.1" | "localhost"))
                && std::env::var("AUDENIQ_ALLOW_LOCAL_PARTNER_TRANSPORT").as_deref() == Ok("true");
            if u.scheme() != "https" && !local_http {
                return Err(Error::InvalidCode("PARTNER_URL_INVALID"));
            }
        }
        Ok(())
    }

    /// Endpoint as recorded in onboarding (`sftp://host/root`,
    /// `s3://bucket/prefix`, `https://…`). Never contains credentials.
    pub fn endpoint_label(&self) -> String {
        match &self.transport {
            TransportConfig::Sftp {
                host,
                port,
                remote_root,
                ..
            } => format!("sftp://{host}:{port}{remote_root}"),
            TransportConfig::S3 { bucket, prefix, .. } => format!("s3://{bucket}/{prefix}"),
            TransportConfig::Local { root } => format!("file://{}", root.display()),
            TransportConfig::None => self
                .http_api
                .as_ref()
                .map(|a| a.base_url.clone())
                .unwrap_or_default(),
        }
    }
}

/// Directory holding `<partner_id>.json` files.
pub fn config_dir() -> Option<PathBuf> {
    std::env::var_os("PARTNER_CONFIG_DIR")
        .map(PathBuf::from)
        .filter(|p| p.is_dir())
}

/// Load one partner's config from `PARTNER_CONFIG_DIR`. `Ok(None)`: no file.
pub fn load(partner_id: &str) -> Result<Option<PartnerConfig>> {
    if !valid_partner_id(partner_id) {
        return Err(Error::Invalid);
    }
    let Some(dir) = config_dir() else {
        return Ok(None);
    };
    load_from(&dir, partner_id)
}

pub fn load_from(dir: &Path, partner_id: &str) -> Result<Option<PartnerConfig>> {
    let path = dir.join(format!("{partner_id}.json"));
    let text = match std::fs::read_to_string(&path) {
        Ok(t) => t,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(_) => return Err(Error::Internal),
    };
    let c = PartnerConfig::parse(&text)?;
    if c.partner_id != partner_id {
        return Err(Error::InvalidCode("PARTNER_CONFIG_ID_MISMATCH"));
    }
    Ok(Some(c))
}

/// Every config in the directory (for `audeniq-admin partner check`).
pub fn load_all() -> Vec<(String, Result<PartnerConfig>)> {
    let Some(dir) = config_dir() else {
        return Vec::new();
    };
    let mut out = Vec::new();
    if let Ok(entries) = std::fs::read_dir(&dir) {
        for e in entries.flatten() {
            let name = e.file_name().to_string_lossy().to_string();
            let Some(id) = name.strip_suffix(".json") else {
                continue;
            };
            if id.ends_with(".example") || !valid_partner_id(id) {
                continue;
            }
            let r = load_from(&dir, id).and_then(|c| c.ok_or(Error::NotFound));
            out.push((id.to_string(), r));
        }
    }
    out.sort_by(|a, b| a.0.cmp(&b.0));
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_sftp_ddex_config_and_hides_secrets() {
        let c = PartnerConfig::parse(
            r#"{"partner_id":"D-5","adapter":"ddex",
                "transport":{"kind":"sftp","host":"sftp.example","username":"u",
                  "private_key":{"file":"/run/secrets/k"},"known_hosts":{"file":"/run/secrets/kh"},
                  "remote_root":"/inbox"},
                "ddex":{"choreography":"batch","ack_dir":"/outbox","live_policy":"on_ack"}}"#,
        )
        .unwrap();
        assert_eq!(c.adapter, AdapterKind::Ddex);
        assert_eq!(c.endpoint_label(), "sftp://sftp.example:22/inbox");
        assert_eq!(c.ddex.live_policy, LivePolicy::OnAck);
        assert_eq!(format!("{:?}", Secret::from_value("x")), "Secret(***)");
    }

    #[test]
    fn rejects_inline_secrets_unknown_fields_and_plain_http() {
        assert!(
            PartnerConfig::parse(
                r#"{"partner_id":"D-5","adapter":"ddex","transport":{"kind":"sftp","host":"h",
                "username":"u","private_key":"inline-key","known_hosts":{"file":"/x"}}}"#
            )
            .is_err()
        );
        assert!(PartnerConfig::parse(r#"{"partner_id":"D-5","adapter":"ddex"}"#).is_err());
        assert!(
            PartnerConfig::parse(
                r#"{"partner_id":"D-1","adapter":"http_api","http_api":{"base_url":"http://p.example",
                "auth":{"kind":"bearer","token":{"env":"T"}}}}"#
            )
            .is_err()
        );
        assert!(PartnerConfig::parse(r#"{"partner_id":"../x","adapter":"http_api"}"#).is_err());
    }

    #[test]
    fn shipped_examples_parse() {
        for (id, text) in [
            (
                "D-5",
                include_str!("../../../deploy/partners/examples/D-5.ddex-sftp.json"),
            ),
            (
                "D-7",
                include_str!("../../../deploy/partners/examples/D-7.ddex-s3.json"),
            ),
            (
                "D-1",
                include_str!("../../../deploy/partners/examples/D-1.partner-spec-sftp.json"),
            ),
            (
                "D-2",
                include_str!("../../../deploy/partners/examples/D-2.http-api.json"),
            ),
        ] {
            let c = PartnerConfig::parse(text).unwrap_or_else(|e| panic!("{id}: {e}"));
            assert_eq!(c.partner_id, id);
        }
    }

    #[test]
    fn secret_ref_needs_exactly_one_source() {
        let both = SecretRef {
            env: Some("A".into()),
            file: Some("/x".into()),
        };
        assert!(matches!(
            both.resolve(),
            Err(Error::PolicyGate("PARTNER_SECRET_REF_INVALID"))
        ));
        let missing = SecretRef {
            env: Some("AUDENIQ_TEST_SURELY_UNSET_VAR".into()),
            file: None,
        };
        assert!(missing.resolve().is_err());
    }
}
