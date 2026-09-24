//! Four independent fail-closed checks over pinned preparation bytes.
//! DB freshness facts come from Muse's orchestrator; no state transition here.
use crate::{
    domain::{FreshnessPin, freshness_guard},
    ern::{generate_ern, validate_metadata, validate_xml},
    error::Result,
    preparation_model::{CanonicalRelease, VerificationPackage},
    route_plan::verify_scope,
    storage::ObjectStore,
};
use sha2::{Digest, Sha256};

pub struct CurrentFacts {
    pub pin: FreshnessPin,
    pub hold: bool,
    pub contract_active: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CheckStatus {
    Pass,
    Fail,
    Unknown,
}

#[derive(Debug)]
pub struct PreflightReport {
    pub xml: CheckStatus,
    pub metadata: CheckStatus,
    pub files: CheckStatus,
    pub rights: CheckStatus,
}

impl PreflightReport {
    pub fn passed(&self) -> bool {
        [self.xml, self.metadata, self.files, self.rights]
            .iter()
            .all(|s| *s == CheckStatus::Pass)
    }
}

fn status(pass: bool) -> CheckStatus {
    if pass {
        CheckStatus::Pass
    } else {
        CheckStatus::Fail
    }
}

/// Bounded per-object check. The existing ObjectStore API buffers `get`; this
/// preparation pass therefore refuses assets over 64 MiB. Large masters remain
/// blocked until the store supplies bounded streaming/hash verification.
pub const MAX_PREFLIGHT_ASSET_BYTES: i64 = 64 * 1024 * 1024;

pub async fn check_files(c: &CanonicalRelease, store: &dyn ObjectStore) -> CheckStatus {
    if validate_metadata(c).is_err() {
        return CheckStatus::Fail;
    }
    let mut outcome = CheckStatus::Pass;
    for a in std::iter::once(&c.artwork).chain(c.tracks.iter().map(|t| &t.audio)) {
        if a.size_bytes > MAX_PREFLIGHT_ASSET_BYTES {
            return CheckStatus::Fail;
        }
        match store.head(&a.object_key).await {
            Ok(Some(meta)) if meta.size == a.size_bytes && meta.content_type == a.content_type => {
            }
            Ok(_) => return CheckStatus::Fail,
            Err(_) => {
                outcome = CheckStatus::Unknown;
                continue;
            }
        }
        match store.get(&a.object_key).await {
            Ok(bytes) => {
                if i64::try_from(bytes.len()).ok() != Some(a.size_bytes)
                    || hex::encode(Sha256::digest(&bytes)) != a.sha256
                {
                    return CheckStatus::Fail;
                }
            }
            Err(_) => outcome = CheckStatus::Unknown,
        }
    }
    outcome
}

/// Reuses the existing shared FreshnessGuard. The expected pin must identify the
/// bytes being checked, not an independently supplied (possibly stale) package.
pub fn check_rights(
    c: &CanonicalRelease,
    v: &VerificationPackage,
    xml: &str,
    expected: &FreshnessPin,
    current: &CurrentFacts,
) -> bool {
    verify_scope(c, v).is_ok()
        && expected.revision_id == c.revision_id
        && expected.snapshot_id == c.snapshot_id
        && expected.verification_hash == c.verification_package_hash
        && i64::try_from(expected.rights_epoch).ok() == Some(c.rights_epoch)
        && !expected.route_contract_id.is_nil()
        && expected.package_hash == hex::encode(Sha256::digest(xml.as_bytes()))
        && freshness_guard(
            expected,
            &current.pin,
            current.hold,
            current.contract_active,
        )
        .is_ok()
}

pub async fn preflight(
    c: &CanonicalRelease,
    v: &VerificationPackage,
    xml: &str,
    expected: &FreshnessPin,
    current: &CurrentFacts,
    store: &dyn ObjectStore,
) -> PreflightReport {
    PreflightReport {
        xml: status(validate_xml(c, xml).is_ok()),
        metadata: status(validate_metadata(c).is_ok()),
        files: check_files(c, store).await,
        rights: status(check_rights(c, v, xml, expected, current)),
    }
}

/// Convenience for callers that want the canonical byte representation before
/// pinning it. Does not persist, mark READY, or enqueue anything.
pub fn preparation_bytes(c: &CanonicalRelease) -> Result<Vec<u8>> {
    Ok(generate_ern(c)?.into_bytes())
}
