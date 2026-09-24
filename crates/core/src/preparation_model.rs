//! Submission supplements bound to Muse's immutable canonical snapshot.
//! No snapshot creation, persistence, hash assignment, or worker wiring lives here.
use chrono::NaiveDate;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct AssetRef {
    pub id: Uuid,
    pub object_key: String,
    pub sha256: String,
    pub size_bytes: i64,
    pub content_type: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct PreparedTrack {
    pub id: Uuid,
    pub title: String,
    pub artist: String,
    pub isrc: String,
    pub disc_number: u32,
    pub track_number: u32,
    pub audio: AssetRef,
}

/// F3 currently approves DSP IDs, not a new region/use grant. Never infer one.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(deny_unknown_fields)]
pub struct DspScope {
    pub dsp_id: Uuid,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PreparedRelease {
    pub canonical: crate::distribution::CanonicalRelease,
    pub org_id: Uuid,
    pub release_id: Uuid,
    pub revision_id: Uuid,
    pub revision_hash: String,
    pub snapshot_id: Uuid,
    pub verification_package_id: Uuid,
    pub verification_package_hash: String,
    pub rights_epoch: i64,
    pub approved_scope: Vec<DspScope>,
    pub title: String,
    pub artist: String,
    pub release_type: String,
    pub release_date: NaiveDate,
    pub language: String,
    pub p_line: String,
    pub c_line: String,
    pub upc: String,
    pub tracks: Vec<PreparedTrack>,
    pub artwork: AssetRef,
}

/// Trusted DB row envelope plus the original F3 JSON bytes' semantic value.
/// Hash is SHA-256 of serde_json serialization, matching review.rs.
#[derive(Debug, Clone)]
pub struct VerificationPackage {
    pub id: Uuid,
    pub org_id: Uuid,
    pub revision_id: Uuid,
    pub rights_epoch: i64,
    pub package_hash: String,
    pub body: serde_json::Value,
}
