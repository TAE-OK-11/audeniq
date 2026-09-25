//! Four independent fail-closed checks over pinned preparation bytes.
//! DB freshness facts come from Muse's orchestrator; no state transition here.
use crate::{
    domain::{FreshnessPin, freshness_guard},
    ern::{generate_prepared_ern, validate_metadata, validate_xml},
    error::Result,
    preparation_model::{PreparedRelease, VerificationPackage},
    route_plan::verify_scope,
    storage::ObjectStore,
};
use serde::Serialize;
use sha2::{Digest, Sha256};

pub struct CurrentFacts {
    pub pin: FreshnessPin,
    pub hold: bool,
    pub contract_active: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum CheckStatus {
    Pass,
    Fail,
    Unknown,
}

#[derive(Debug, Serialize)]
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

/// Largest asset any stage accepts. It is the upload cap itself (the largest
/// audio master), so nothing that passed upload can be refused later for
/// size. Hash verification streams (`ObjectStore::digest`), so memory stays
/// constant regardless of the object size.
pub const MAX_PREFLIGHT_ASSET_BYTES: i64 = crate::uploads::MAX_AUDIO_BYTES;

pub async fn check_files(c: &PreparedRelease, store: &dyn ObjectStore) -> CheckStatus {
    if validate_metadata(c).is_err() {
        return CheckStatus::Fail;
    }
    let mut outcome = CheckStatus::Pass;
    for a in std::iter::once(&c.artwork).chain(c.tracks.iter().map(|t| &t.audio)) {
        if a.size_bytes > MAX_PREFLIGHT_ASSET_BYTES {
            return CheckStatus::Fail;
        }
        match store.head(&a.object_key).await {
            Ok(Some(meta)) if meta.size == a.size_bytes && meta.content_type == a.content_type => {}
            Ok(_) => return CheckStatus::Fail,
            Err(_) => {
                outcome = CheckStatus::Unknown;
                continue;
            }
        }
        match store.digest(&a.object_key, a.size_bytes as u64).await {
            Ok(d) => {
                if i64::try_from(d.size).ok() != Some(a.size_bytes) || d.sha256 != a.sha256 {
                    return CheckStatus::Fail;
                }
            }
            Err(crate::error::Error::PolicyGate(_)) => return CheckStatus::Fail,
            Err(_) => outcome = CheckStatus::Unknown,
        }
    }
    outcome
}

/// Reuses the existing shared FreshnessGuard. The expected pin must identify the
/// bytes being checked, not an independently supplied (possibly stale) package.
pub fn check_rights(
    c: &PreparedRelease,
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
    c: &PreparedRelease,
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
pub fn preparation_bytes(c: &PreparedRelease) -> Result<Vec<u8>> {
    Ok(generate_prepared_ern(c)?.into_bytes())
}
