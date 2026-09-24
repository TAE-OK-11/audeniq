//! DSP approval mapping only. No endpoint activation or transmission.
use crate::{
    error::{Error, Result},
    preparation_model::{CanonicalRelease, DspScope, VerificationPackage},
};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use uuid::Uuid;

/// Decode the actual F3 row format, checking its digest before trusting its DSP set.
pub fn approved_dsps(v: &VerificationPackage) -> Result<BTreeSet<Uuid>> {
    let b = &v.body;
    if crate::domain::digest(b) != v.package_hash
        || b["decision"] != "PASS"
        || b["revision_id"]
            .as_str()
            .and_then(|s| Uuid::parse_str(s).ok())
            != Some(v.revision_id)
        || b["rights_epoch"].as_i64() != Some(v.rights_epoch)
        || v.rights_epoch < 0
        || b["schema_version"].as_u64() != Some(1)
    {
        return Err(Error::Invalid);
    }
    let raw = b["approved_scope"]["dsp_ids"]
        .as_array()
        .ok_or(Error::Invalid)?;
    let mut ids = BTreeSet::new();
    for id in raw {
        let id = id
            .as_str()
            .and_then(|s| Uuid::parse_str(s).ok())
            .ok_or(Error::Invalid)?;
        if id.is_nil() || !ids.insert(id) {
            return Err(Error::Invalid);
        }
    }
    Ok(ids)
}

/// Stage 3 never approves more DSPs than the pinned Stage 2 package.
pub fn verify_scope(c: &CanonicalRelease, v: &VerificationPackage) -> Result<()> {
    if c.org_id != v.org_id
        || c.verification_package_id != v.id
        || c.revision_id != v.revision_id
        || c.rights_epoch != v.rights_epoch
        || c.verification_package_hash != v.package_hash
        || v.body["revision_hash"].as_str() != Some(c.revision_hash.as_str())
    {
        return Err(Error::Conflict);
    }
    let approved = approved_dsps(v)?;
    let scope: BTreeSet<_> = c.approved_scope.iter().map(|s| s.dsp_id).collect();
    if scope.len() != c.approved_scope.len() || scope != approved {
        return Err(Error::Conflict);
    }
    Ok(())
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SubmissionItems {
    pub scope: DspScope,
    pub snapshot_id: Uuid,
    pub verification_package_hash: String,
    pub metadata_file: String,
    pub audio_asset_ids: Vec<Uuid>,
    pub artwork_asset_id: Uuid,
    pub profile: String,
    /// Synthetic output is never a commercially executable route.
    pub delivery_enabled: bool,
}

pub fn plan_submissions(
    c: &CanonicalRelease,
    v: &VerificationPackage,
) -> Result<Vec<SubmissionItems>> {
    verify_scope(c, v)?;
    crate::ern::validate_metadata(c)?;
    let audio_asset_ids = crate::ern::ordered_tracks(c)
        .iter()
        .map(|t| t.audio.id)
        .collect::<Vec<_>>();
    Ok(approved_dsps(v)?
        .into_iter()
        .map(|dsp_id| SubmissionItems {
            scope: DspScope { dsp_id },
            snapshot_id: c.snapshot_id,
            verification_package_hash: c.verification_package_hash.clone(),
            metadata_file: "release.xml".into(),
            audio_asset_ids: audio_asset_ids.clone(),
            artwork_asset_id: c.artwork.id,
            profile: crate::ern::SYNTHETIC_PROFILE.into(),
            delivery_enabled: false,
        })
        .collect())
}
