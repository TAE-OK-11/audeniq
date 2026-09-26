//! Four independent fail-closed checks over pinned preparation bytes.
//! DB freshness facts come from Muse's orchestrator; no state transition here.
use crate::{
    domain::{FreshnessPin, freshness_guard},
    ern::{generate_prepared_ern, validate_metadata, validate_xml},
    error::Result,
    preparation_model::{AssetRef, PreparedRelease, VerificationPackage},
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

/// One asset's object-store verification: HEAD metadata match, then a
/// streaming SHA-256/size check via `ObjectStore::digest` — constant memory
/// regardless of asset size, no full download into RAM.
async fn check_one_asset(a: &AssetRef, store: &dyn ObjectStore) -> CheckStatus {
    match store.head(&a.object_key).await {
        Ok(Some(meta)) if meta.size == a.size_bytes && meta.content_type == a.content_type => {}
        Ok(_) => return CheckStatus::Fail,
        Err(_) => return CheckStatus::Unknown,
    }
    match store.digest(&a.object_key, a.size_bytes as u64).await {
        Ok(d) => {
            if i64::try_from(d.size).ok() == Some(a.size_bytes) && d.sha256 == a.sha256 {
                CheckStatus::Pass
            } else {
                CheckStatus::Fail
            }
        }
        Err(crate::error::Error::PolicyGate(_)) => CheckStatus::Fail,
        Err(_) => CheckStatus::Unknown,
    }
}

pub async fn check_files(c: &PreparedRelease, store: &dyn ObjectStore) -> CheckStatus {
    if validate_metadata(c).is_err() {
        return CheckStatus::Fail;
    }
    let assets: Vec<&AssetRef> = std::iter::once(&c.artwork)
        .chain(c.tracks.iter().map(|t| &t.audio))
        .collect();
    if assets
        .iter()
        .any(|a| a.size_bytes > MAX_PREFLIGHT_ASSET_BYTES)
    {
        return CheckStatus::Fail;
    }
    // Object-store round trips dominate this check; run them with bounded
    // concurrency, eight assets at a time (the old loop was fully
    // sequential). Each asset resolves to Pass/Fail/Unknown independently;
    // Fail wins, else Unknown, else Pass — same verdict as the sequential
    // loop.
    //
    // NOTE: `futures::stream::buffer_unordered` was tried here and
    // reverted: buffering the borrowed per-asset futures broke the
    // higher-ranked `Send` bound the worker's spawned tasks require
    // ("implementation of Send is not general enough"). The chunked
    // `tokio::join!` below awaits every future directly in this frame,
    // which keeps the borrow checker happy.
    let mut outcome = CheckStatus::Pass;
    for chunk in assets.chunks(8) {
        let (r1, r2, r3, r4, r5, r6, r7, r8) = tokio::join!(
            check_some(chunk, 0, store),
            check_some(chunk, 1, store),
            check_some(chunk, 2, store),
            check_some(chunk, 3, store),
            check_some(chunk, 4, store),
            check_some(chunk, 5, store),
            check_some(chunk, 6, store),
            check_some(chunk, 7, store),
        );
        for r in [r1, r2, r3, r4, r5, r6, r7, r8] {
            match r {
                CheckStatus::Fail => return CheckStatus::Fail,
                CheckStatus::Unknown => outcome = CheckStatus::Unknown,
                CheckStatus::Pass => {}
            }
        }
    }
    outcome
}

/// Probe `chunk[i]` when present, else `Pass`. Exists so the fan-out in
/// [`check_files`] can name eight statically-known futures for
/// `tokio::join!` without storing borrowed futures in a combinator.
async fn check_some(chunk: &[&AssetRef], i: usize, store: &dyn ObjectStore) -> CheckStatus {
    match chunk.get(i) {
        Some(a) => check_one_asset(a, store).await,
        None => CheckStatus::Pass,
    }
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
