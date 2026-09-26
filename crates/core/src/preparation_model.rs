//! Submission supplements bound to Muse's immutable canonical snapshot.
//! No snapshot creation, persistence, hash assignment, or worker wiring lives here.
use chrono::NaiveDate;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sqlx::{PgPool, Row};
use uuid::Uuid;

use crate::{
    distribution::CanonicalRelease,
    error::{Error, Result},
    identifiers::validate_upc,
};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct AssetRef {
    pub id: Uuid,
    pub object_key: String,
    pub sha256: String,
    pub size_bytes: i64,
    pub content_type: String,
    /// Measured audio duration in seconds (Stage 1 QC). None when unknown;
    /// the DDEX builder fails closed without it (DDEX_DURATION_UNKNOWN).
    #[serde(default)]
    pub duration_secs: Option<f64>,
    /// ffprobe-measured audio technical specs (Stage 1 QC), persisted to
    /// `catalog.assets`. None for images, for legacy rows probed before
    /// the columns existed, or when probing failed. The DDEX ERN builder
    /// emits these as the real `TechnicalSoundRecordingDetails` and omits
    /// the elements when unknown — never fabricated constants.
    #[serde(default)]
    pub sample_rate: Option<i32>,
    #[serde(default)]
    pub channels: Option<i32>,
    #[serde(default)]
    pub bits_per_sample: Option<i32>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct PreparedTrack {
    pub id: Uuid,
    pub title: String,
    /// Version/designation ("Radio Edit"). Empty = none; emitted as DDEX
    /// SubTitle only when non-empty (ERN 3.8.2 has no VersionTitle element).
    #[serde(default)]
    pub version: String,
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
    /// Drives DDEX ERN ParentalWarningType.
    #[serde(default)]
    pub explicit: bool,
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

/// Load file references from the trusted assets table. Size and media type
/// are not part of the canonical snapshot, so they are read here; id, key
/// and hash must already be pinned on the snapshot (binding is re-checked
/// by `ern::validate_binding` downstream).
///
/// One query for all `asset_ids` (the old per-track loop was N+1). Returns
/// the refs keyed by asset id; the caller checks kind per id so the
/// `PREPARATION_ASSET_KIND_MISMATCH` error still names the right cause.
async fn asset_refs(
    pool: &PgPool,
    org_id: Uuid,
    asset_ids: &[Uuid],
    expected_kinds: &std::collections::HashMap<Uuid, &'static str>,
) -> Result<std::collections::HashMap<Uuid, AssetRef>> {
    let rows = sqlx::query(
        "SELECT id, object_key, sha256, size_bytes, content_type, duration_secs, sample_rate, channels, bits_per_sample FROM catalog.assets WHERE org_id=$1 AND id = ANY($2)",
    )
    .bind(org_id)
    .bind(asset_ids)
    .fetch_all(pool)
    .await?;
    let mut out = std::collections::HashMap::with_capacity(asset_ids.len());
    for row in &rows {
        let id: Uuid = row.get("id");
        let content_type: String = row.get("content_type");
        if let Some(kind) = expected_kinds.get(&id)
            && !content_type.starts_with(*kind)
        {
            return Err(Error::PolicyGate("PREPARATION_ASSET_KIND_MISMATCH"));
        }
        out.insert(
            id,
            AssetRef {
                id,
                object_key: row.get("object_key"),
                sha256: row
                    .get::<Option<String>, _>("sha256")
                    .ok_or(Error::PolicyGate("PREPARATION_ASSET_SHA_MISSING"))?,
                size_bytes: row.get("size_bytes"),
                content_type,
                duration_secs: row.get("duration_secs"),
                sample_rate: row.get("sample_rate"),
                channels: row.get("channels"),
                bits_per_sample: row.get("bits_per_sample"),
            },
        );
    }
    Ok(out)
}

fn draft_field(draft: &Value, key: &str, code: &'static str) -> Result<String> {
    draft
        .get(key)
        .and_then(Value::as_str)
        .filter(|s| !s.trim().is_empty())
        .map(str::to_owned)
        .ok_or(Error::PolicyGate(code))
}

impl PreparedRelease {
    /// Build the DSP-neutral submission model from a frozen canonical
    /// snapshot. Every supplement (UPC, artwork, audio file references,
    /// release metadata) is read from trusted catalog rows; a missing piece
    /// fails closed with a distinct code — nothing is invented.
    ///
    /// `snapshot_id` is the immutable `distribution.canonical_releases` row
    /// id, so the ERN message id and the route plan both bind to the exact
    /// frozen snapshot.
    pub async fn from_canonical(
        pool: &PgPool,
        snapshot_id: Uuid,
        c: &CanonicalRelease,
    ) -> Result<PreparedRelease> {
        let upc = c
            .upc
            .clone()
            .ok_or(Error::PolicyGate("PREPARATION_UPC_MISSING"))?;
        validate_upc(&upc)?;
        let artwork_asset_id = c
            .artwork
            .as_ref()
            .ok_or(Error::PolicyGate("PREPARATION_ARTWORK_MISSING"))?
            .asset_id;
        // One batched asset lookup for the artwork plus every track file
        // (the old per-track loop was N+1 round trips).
        let mut asset_ids = Vec::with_capacity(c.tracks.len() + 1);
        let mut seen = std::collections::HashSet::with_capacity(c.tracks.len() + 1);
        let mut expected_kinds = std::collections::HashMap::with_capacity(c.tracks.len() + 1);
        asset_ids.push(artwork_asset_id);
        seen.insert(artwork_asset_id);
        // Artwork keeps its "image" expectation if the same asset id ever
        // shows up as track audio (pathological data): first wins, matching
        // the old load-artwork-first order.
        expected_kinds.insert(artwork_asset_id, "image");
        for t in &c.tracks {
            if let Some(asset_id) = t.asset_id
                && seen.insert(asset_id)
            {
                asset_ids.push(asset_id);
                expected_kinds.entry(asset_id).or_insert("audio");
            }
        }
        let refs = asset_refs(pool, c.org_id, &asset_ids, &expected_kinds).await?;

        let artwork = refs
            .get(&artwork_asset_id)
            .cloned()
            .ok_or(Error::PolicyGate("PREPARATION_ASSET_MISSING"))?;

        let mut tracks = Vec::with_capacity(c.tracks.len());
        for t in &c.tracks {
            let isrc = t
                .isrc
                .clone()
                .ok_or(Error::PolicyGate("PREPARATION_ISRC_MISSING"))?;
            let asset_id = t
                .asset_id
                .ok_or(Error::PolicyGate("PREPARATION_AUDIO_MISSING"))?;
            let audio = refs
                .get(&asset_id)
                .cloned()
                .ok_or(Error::PolicyGate("PREPARATION_ASSET_MISSING"))?;
            tracks.push(PreparedTrack {
                id: t.track_id,
                title: t.title.clone(),
                version: t.version.clone(),
                artist: t.artist_name.clone(),
                isrc,
                disc_number: u32::try_from(t.disc_number)
                    .map_err(|_| Error::PolicyGate("PREPARATION_TRACK_NUMBER_INVALID"))?,
                track_number: u32::try_from(t.track_number)
                    .map_err(|_| Error::PolicyGate("PREPARATION_TRACK_NUMBER_INVALID"))?,
                audio,
            });
        }

        // Release metadata comes from the submitted revision (frozen, what
        // Stage 1/2 reviewed), never the live draft: the delivered message
        // must describe exactly the reviewed application. Revisions without a
        // frozen draft (hand-built fixtures) fall back to the release row.
        let draft: Value = sqlx::query_scalar(
            "SELECT COALESCE(NULLIF(ar.body -> 'release' -> 'draft', 'null'::jsonb), r.draft)
             FROM catalog.application_revisions ar
             JOIN catalog.releases r ON r.org_id=ar.org_id AND r.id=ar.release_id
             WHERE ar.org_id=$1 AND ar.id=$2",
        )
        .bind(c.org_id)
        .bind(c.revision_id)
        .fetch_optional(pool)
        .await?
        .ok_or(Error::NotFound)?;
        let release_date_raw =
            draft_field(&draft, "release_date", "PREPARATION_RELEASE_DATE_MISSING")?;
        let release_date = NaiveDate::parse_from_str(&release_date_raw, "%Y-%m-%d")
            .map_err(|_| Error::PolicyGate("PREPARATION_RELEASE_DATE_INVALID"))?;

        Ok(PreparedRelease {
            canonical: c.clone(),
            org_id: c.org_id,
            release_id: c.release_id,
            revision_id: c.revision_id,
            revision_hash: c.revision_hash.clone(),
            snapshot_id,
            verification_package_id: c.verification_package_id,
            verification_package_hash: c.verification_package_hash.clone(),
            rights_epoch: c.rights_epoch,
            approved_scope: c
                .approved_dsp_ids
                .iter()
                .map(|dsp_id| DspScope { dsp_id: *dsp_id })
                .collect(),
            title: c.release_title.clone(),
            artist: draft_field(&draft, "artist", "PREPARATION_ARTIST_MISSING")?,
            release_type: c.release_type.clone(),
            release_date,
            language: draft_field(&draft, "language", "PREPARATION_LANGUAGE_MISSING")?,
            p_line: draft_field(&draft, "p_line", "PREPARATION_P_LINE_MISSING")?,
            c_line: draft_field(&draft, "c_line", "PREPARATION_C_LINE_MISSING")?,
            upc,
            tracks,
            artwork,
            explicit: c.explicit,
        })
    }
}

impl VerificationPackage {
    /// Load the trusted Stage 2 row envelope for scope/rights checks.
    pub async fn load(pool: &PgPool, id: Uuid) -> Result<VerificationPackage> {
        let row = sqlx::query(
            "SELECT org_id, revision_id, rights_epoch, package_hash, body FROM distribution.verification_packages WHERE id=$1",
        )
        .bind(id)
        .fetch_optional(pool)
        .await?
        .ok_or(Error::NotFound)?;
        Ok(VerificationPackage {
            id,
            org_id: row.get("org_id"),
            revision_id: row.get("revision_id"),
            rights_epoch: row.get("rights_epoch"),
            package_hash: row.get("package_hash"),
            body: row.get("body"),
        })
    }
}
