//! Real partner adapters (F6): what turns a staff-approved, contract-routed
//! delivery job into bytes on a DSP's ingestion endpoint.
//!
//! - [`drop::FileDropAdapter`] in DDEX mode: ERN 3.8.2 + resources into an
//!   SFTP/S3 drop following the DDEX ERN choreography (batch or
//!   release-by-release profile), completion marker last, ACK files polled.
//!   Global DSPs (D-5..D-11).
//! - [`drop::FileDropAdapter`] in partner-spec mode: the same drop with a
//!   JSON + CSV release feed instead of (or next to) DDEX. Korean services
//!   (D-1..D-4) until their contract defines the format.
//! - [`api::HttpApiAdapter`]: partner REST API (create → upload files →
//!   commit, status polling, takedown).
//!
//! Which adapter a partner gets, and its endpoint and credentials, come
//! from `PARTNER_CONFIG_DIR/<partner_id>.json` (`crate::partner_config`).
//! Capability flags are the intersection of what the adapter implements
//! and what the partner's profile row enables, so a capability the
//! contract does not document stays off even when the code supports it.
//! A config file never enables sending by itself: the DB kill switch,
//! onboarding gate, contract route and staff approval all still apply.
pub mod ack;
pub mod api;
pub mod drop;
pub mod http;
pub mod inbox;
pub mod manifest;

use crate::error::{Error, Result};
use crate::execution::{AdapterRegistry, Capabilities, DspAdapter, TransferFile, TransferPackage};
use crate::partner_config::{AdapterKind, PartnerConfig};
use crate::storage::ObjectStore;
use crate::transport::Upload;
use sha2::Digest;
use sqlx::PgPool;
use std::path::PathBuf;
use std::sync::{Arc, OnceLock, RwLock};
use std::time::{Duration, Instant};

/// Largest single file an adapter stages (masters are capped at upload).
const MAX_STAGED_FILE: u64 = crate::preflight::MAX_PREFLIGHT_ASSET_BYTES as u64;

pub fn intersect(a: Capabilities, b: Capabilities) -> Capabilities {
    Capabilities {
        validate_package: a.validate_package && b.validate_package,
        prepare_transfer: a.prepare_transfer && b.prepare_transfer,
        send_or_publish: a.send_or_publish && b.send_or_publish,
        inquire_submission: a.inquire_submission && b.inquire_submission,
        parse_ack: a.parse_ack && b.parse_ack,
        get_release_status: a.get_release_status && b.get_release_status,
        update_release: a.update_release && b.update_release,
        takedown: a.takedown && b.takedown,
        receive_royalty_report: a.receive_royalty_report && b.receive_royalty_report,
    }
}

/// Local copies of the package files, verified against their pins, removed
/// when dropped.
pub struct Staged {
    pub dir: PathBuf,
    pub uploads: Vec<Upload>,
}

impl Drop for Staged {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

/// Why staging failed: storage is retryable, integrity is not.
#[derive(Debug)]
pub enum StageError {
    Storage(String),
    Integrity(String),
}

fn staging_root() -> PathBuf {
    std::env::var_os("AUDENIQ_DELIVERY_TMP")
        .map(PathBuf::from)
        .unwrap_or_else(std::env::temp_dir)
}

/// Download every file of the package to a private temp dir under its
/// delivery name, re-verifying size and SHA-256 on the way (the bytes that
/// leave are exactly the pinned bytes). `remote_dir` prefixes the upload
/// paths.
pub async fn stage_files(
    storage: &Arc<dyn ObjectStore>,
    files: &[TransferFile],
    remote_dir: &str,
) -> std::result::Result<Staged, StageError> {
    let dir = staging_root().join(format!("audeniq-dlv-{}", uuid::Uuid::new_v4().simple()));
    std::fs::create_dir_all(&dir).map_err(|e| StageError::Storage(e.to_string()))?;
    let mut staged = Staged {
        dir: dir.clone(),
        uploads: Vec::with_capacity(files.len()),
    };
    for f in files {
        if !crate::transport::valid_remote_path(&f.delivery_name) || f.delivery_name.contains('/') {
            return Err(StageError::Integrity(format!(
                "invalid delivery name {}",
                f.delivery_name
            )));
        }
        let local = dir.join(&f.delivery_name);
        let max = (f.size_bytes.max(0) as u64).min(MAX_STAGED_FILE);
        let digest = match storage.download_to(&f.object_key, &local, max).await {
            Ok(d) => d,
            Err(Error::PolicyGate(_)) => {
                return Err(StageError::Integrity(format!(
                    "{} larger than pinned",
                    f.delivery_name
                )));
            }
            Err(e) => return Err(StageError::Storage(format!("{e}"))),
        };
        if digest.sha256 != f.sha256 || digest.size != f.size_bytes as u64 {
            return Err(StageError::Integrity(format!(
                "{} does not match its pinned hash",
                f.delivery_name
            )));
        }
        staged.uploads.push(Upload::File {
            local,
            remote: crate::transport::join(remote_dir, &f.delivery_name)
                .trim_start_matches('/')
                .to_string(),
        });
    }
    Ok(staged)
}

/// Structural checks every file-drop/API adapter runs before the wire:
/// UPC present, the ERN is well-formed and references exactly the files
/// being sent under exactly those names.
pub fn check_ddex_package(package: &TransferPackage) -> Result<()> {
    let upc = package
        .upc
        .as_deref()
        .ok_or(Error::PolicyGate("DELIVERY_UPC_MISSING"))?;
    crate::identifiers::validate_upc(upc).map_err(|_| Error::PolicyGate("DELIVERY_UPC_INVALID"))?;
    let xml = std::str::from_utf8(&package.ern_xml)
        .map_err(|_| Error::PolicyGate("DELIVERY_ERN_NOT_UTF8"))?;
    let mut r = quick_xml::Reader::from_str(xml);
    let mut root = None;
    loop {
        match r.read_event() {
            Ok(quick_xml::events::Event::Start(e)) if root.is_none() => {
                root = Some(e.local_name().as_ref().to_string());
            }
            Ok(quick_xml::events::Event::Eof) => break,
            Err(_) => return Err(Error::PolicyGate("DELIVERY_ERN_MALFORMED")),
            _ => {}
        }
    }
    if root.as_deref() != Some("NewReleaseMessage") {
        return Err(Error::PolicyGate("DELIVERY_ERN_MALFORMED"));
    }
    let mut referenced = crate::transport::xml_values(xml, "FileName");
    referenced.sort();
    referenced.dedup();
    let mut sending: Vec<String> = package
        .files
        .iter()
        .map(|f| f.delivery_name.clone())
        .collect();
    sending.sort();
    if referenced != sending {
        tracing::warn!(
            ?referenced,
            ?sending,
            "ERN file references differ from the package files"
        );
        return Err(Error::PolicyGate("DELIVERY_ERN_FILE_MISMATCH"));
    }
    // Every referenced hash must be the pinned hash of that file.
    let hashes = crate::transport::xml_values(xml, "HashSum");
    for f in &package.files {
        if !hashes.iter().any(|h| h.eq_ignore_ascii_case(&f.sha256)) {
            return Err(Error::PolicyGate("DELIVERY_ERN_HASH_MISMATCH"));
        }
    }
    Ok(())
}

pub fn now_batch_id() -> String {
    // DDEX choreography batch ids: YYYYMMDDhhmmssnnn (17 digits).
    chrono::Utc::now().format("%Y%m%d%H%M%S%3f").to_string()
}

pub fn sha256_hex(b: &[u8]) -> String {
    hex::encode(sha2::Sha256::digest(b))
}

/// Build the adapter for one partner config. `profile_caps` is the
/// profile row's capability JSON (what the contract documents).
pub fn build_adapter(
    config: PartnerConfig,
    profile_caps: Capabilities,
    storage: Arc<dyn ObjectStore>,
) -> Result<Arc<dyn DspAdapter>> {
    Ok(match config.adapter {
        AdapterKind::Ddex | AdapterKind::PartnerSpec => {
            Arc::new(drop::FileDropAdapter::new(config, profile_caps, storage)?)
        }
        AdapterKind::HttpApi => Arc::new(api::HttpApiAdapter::new(config, profile_caps, storage)?),
    })
}

struct Cached {
    built: Instant,
    registry: Arc<AdapterRegistry>,
}

fn cache() -> &'static RwLock<Option<Cached>> {
    static C: OnceLock<RwLock<Option<Cached>>> = OnceLock::new();
    C.get_or_init(|| RwLock::new(None))
}

/// How long a built registry is reused. Config files and profile
/// capabilities are re-read after this (or on `invalidate`).
fn registry_ttl() -> Duration {
    Duration::from_secs(
        std::env::var("PARTNER_REGISTRY_TTL_SECS")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(300u64)
            .clamp(5, 3600),
    )
}

pub fn invalidate() {
    if let Ok(mut c) = cache().write() {
        *c = None;
    }
}

/// The worker's adapter registry: the sandbox MockDSP plus one adapter per
/// configured partner profile. Built once and reused (it used to be rebuilt
/// for every send/poll/takedown job). A partner whose config fails to load
/// is logged and left out: its jobs fail closed with EXECUTION_NO_ADAPTER.
pub async fn registry(
    pool: &PgPool,
    storage: &Arc<dyn ObjectStore>,
    mock: Arc<dyn DspAdapter>,
) -> Result<Arc<AdapterRegistry>> {
    if let Ok(c) = cache().read()
        && let Some(c) = c.as_ref()
        && c.built.elapsed() < registry_ttl()
    {
        return Ok(c.registry.clone());
    }
    let mut reg = AdapterRegistry::new();
    reg.register(mock);
    if crate::partner_config::config_dir().is_some() {
        let profiles: Vec<(String, serde_json::Value)> = sqlx::query_as(
            "SELECT partner_id, capabilities FROM execution.adapter_profiles WHERE transport <> 'mock'",
        )
        .fetch_all(pool)
        .await?;
        for (partner_id, caps) in profiles {
            match crate::partner_config::load(&partner_id) {
                Ok(Some(cfg)) => {
                    match build_adapter(cfg, Capabilities::from_json(&caps), storage.clone()) {
                        Ok(a) => reg.register(a),
                        Err(e) => {
                            tracing::warn!(partner_id, error=%e, "partner adapter not built")
                        }
                    }
                }
                Ok(None) => {}
                Err(e) => tracing::warn!(partner_id, error=%e, "partner config invalid"),
            }
        }
    }
    let registry = Arc::new(reg);
    if let Ok(mut c) = cache().write() {
        *c = Some(Cached {
            built: Instant::now(),
            registry: registry.clone(),
        });
    }
    Ok(registry)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::execution::FileRole;

    fn pkg(xml: &str, names: &[(&str, &str)]) -> TransferPackage {
        TransferPackage {
            package_id: uuid::Uuid::nil(),
            package_hash: String::new(),
            org_id: uuid::Uuid::nil(),
            release_id: uuid::Uuid::nil(),
            ern_xml: xml.as_bytes().to_vec(),
            files: names
                .iter()
                .map(|(n, h)| TransferFile {
                    object_key: format!("registered/{n}"),
                    sha256: h.to_string(),
                    size_bytes: 1,
                    content_type: "audio/flac".into(),
                    delivery_name: n.to_string(),
                    role: FileRole::Audio,
                })
                .collect(),
            upc: Some("036000291452".into()),
            prepared: None,
            genre: None,
            label: None,
        }
    }

    #[test]
    fn ddex_package_must_reference_exactly_the_sent_files() {
        let h = "a".repeat(64);
        let xml = format!(
            "<ern:NewReleaseMessage xmlns:ern=\"x\"><File><FileName>036000291452_01_001.flac</FileName><HashSum><HashSum>{h}</HashSum></HashSum></File></ern:NewReleaseMessage>"
        );
        check_ddex_package(&pkg(&xml, &[("036000291452_01_001.flac", &h)])).unwrap();
        assert!(matches!(
            check_ddex_package(&pkg(&xml, &[("other.flac", &h)])),
            Err(Error::PolicyGate("DELIVERY_ERN_FILE_MISMATCH"))
        ));
        assert!(matches!(
            check_ddex_package(&pkg(&xml, &[("036000291452_01_001.flac", &"b".repeat(64))])),
            Err(Error::PolicyGate("DELIVERY_ERN_HASH_MISMATCH"))
        ));
        assert!(matches!(
            check_ddex_package(&pkg("<Other/>", &[])),
            Err(Error::PolicyGate("DELIVERY_ERN_MALFORMED"))
        ));
        let mut no_upc = pkg(&xml, &[]);
        no_upc.upc = None;
        assert!(check_ddex_package(&no_upc).is_err());
    }

    #[test]
    fn batch_ids_follow_the_ddex_pattern() {
        let b = now_batch_id();
        assert_eq!(b.len(), 17);
        assert!(b.bytes().all(|c| c.is_ascii_digit()));
    }
}
