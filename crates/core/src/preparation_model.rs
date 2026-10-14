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
    /// Engine-measured audio technical specs (Stage 1 QC), persisted to
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
    /// 재발매·이전 음원의 최초 발매일 (`originalDate`). 없으면 신규 발매.
    /// Omitted from the serialized form when absent so snapshots prepared
    /// before this field keep their hash.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub original_release_date: Option<NaiveDate>,
    pub language: String,
    pub p_line: String,
    pub c_line: String,
    pub upc: String,
    pub tracks: Vec<PreparedTrack>,
    pub artwork: AssetRef,
    /// Drives DDEX ERN ParentalWarningType.
    #[serde(default)]
    pub explicit: bool,
    /// Deal territories from the submitted draft. Worldwide is omitted from
    /// the serialized form so snapshots prepared before this field keep
    /// their hash.
    #[serde(
        default,
        skip_serializing_if = "crate::territory::Territories::is_worldwide"
    )]
    pub territories: crate::territory::Territories,
    /// Simultaneous worldwide release time (KST, "HH:MM") from the submitted
    /// draft; `None` = each country's local midnight on the release date.
    /// Omitted when unset so earlier snapshots keep their hash.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub release_time: Option<String>,
    /// Pre-release start (pre-save page, previews, store pre-orders), before
    /// the release date. Omitted when unset.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub preorder_start: Option<NaiveDate>,
    /// Per track (by id): where previews and short-form clips start, in
    /// seconds. Omitted when empty.
    #[serde(default, skip_serializing_if = "std::collections::BTreeMap::is_empty")]
    pub preview_starts: std::collections::BTreeMap<Uuid, u32>,
    /// The main artist's own page id per platform slug (0122), e.g.
    /// "spotify" → "spotify:artist:…". Omitted when none is known.
    #[serde(default, skip_serializing_if = "std::collections::BTreeMap::is_empty")]
    pub artist_platform_ids: std::collections::BTreeMap<String, String>,
    /// Last licensed day (inclusive): the earliest end of the rights grants
    /// and the agreement's scope. The ERN deal ends on it. Omitted when open.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rights_end: Option<NaiveDate>,
    /// DDEX UseTypes the rights grants allow. Empty means not restricted; a
    /// DSP whose deal needs another use is blocked at staging
    /// (RIGHTS_SCOPE_USE). Omitted when empty.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub permitted_uses: Vec<String>,
}

/// One registered rights grant, as far as delivery scope is concerned.
#[derive(Debug, Clone)]
pub struct ScopeGrant {
    pub territories: Vec<String>,
    pub uses: Vec<String>,
    pub start: Option<NaiveDate>,
    /// First unlicensed day.
    pub end_exclusive: Option<NaiveDate>,
}

/// Where and until when the release may be delivered, and for which uses.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RightsScope {
    pub territories: crate::territory::Territories,
    /// Last licensed day (inclusive).
    pub rights_end: Option<NaiveDate>,
    pub permitted_uses: Vec<String>,
}

/// Narrow the application's territories to what the release's rights grants
/// license on the release date, then to the agreement's scope. Without grants
/// or a scope the application decides alone. A grant's empty territory/use
/// set means "not restricted"; `WW`/`WORLD` is everywhere. Fails closed when
/// nothing is licensed on the release date or in any requested territory.
pub fn rights_scope(
    requested: crate::territory::Territories,
    grants: &[ScopeGrant],
    contract: Option<&crate::contract_scope::ContractScope>,
    release_date: NaiveDate,
) -> Result<RightsScope> {
    use crate::territory::Territories;
    let mut territories = requested;
    let mut rights_end = None;
    let mut permitted_uses = vec![];
    if !grants.is_empty() {
        let live: Vec<&ScopeGrant> = grants
            .iter()
            .filter(|g| {
                g.start.is_none_or(|s| s <= release_date)
                    && g.end_exclusive.is_none_or(|e| e > release_date)
            })
            .collect();
        if live.is_empty() {
            return Err(Error::PolicyGate("PREPARATION_RIGHTS_PERIOD"));
        }
        // Union of the licensed territories.
        let mut licensed = std::collections::BTreeSet::new();
        let mut world = false;
        for g in &live {
            match Territories::from_codes(&g.territories)
                .map_err(|_| Error::PolicyGate("PREPARATION_RIGHTS_TERRITORY"))?
            {
                Territories::Only(x) => licensed.extend(x),
                _ => world = true,
            }
        }
        if !world {
            territories = territories
                .narrow(&Territories::Only(licensed.into_iter().collect()))
                .ok_or(Error::PolicyGate("PREPARATION_RIGHTS_TERRITORY"))?;
        }
        rights_end = live
            .iter()
            .filter_map(|g| g.end_exclusive)
            .min()
            .map(|e| e.pred_opt().unwrap_or(e));
        if !live.iter().any(|g| g.uses.is_empty()) {
            permitted_uses = live
                .iter()
                .flat_map(|g| g.uses.iter().cloned())
                .collect::<std::collections::BTreeSet<_>>()
                .into_iter()
                .collect();
        }
    }
    if let Some(c) = contract {
        territories = territories
            .narrow(&c.territories)
            .ok_or(Error::PolicyGate("PREPARATION_CONTRACT_TERRITORY"))?;
        // A contract ending before the release date is a per-DSP blocker at
        // staging (CONTRACT_ENDS_BEFORE_RELEASE); the deal never ends before
        // it starts.
        if let Some(last) = c.end.filter(|last| *last >= release_date) {
            rights_end = Some(rights_end.map_or(last, |e: NaiveDate| e.min(last)));
        }
    }
    Ok(RightsScope {
        territories,
        rights_end,
        permitted_uses,
    })
}

/// The release's active rights grants (release- or track-level).
async fn scope_grants(pool: &PgPool, c: &CanonicalRelease) -> Result<Vec<ScopeGrant>> {
    let tracks: Vec<Uuid> = c.tracks.iter().map(|t| t.track_id).collect();
    type At = Option<chrono::DateTime<chrono::Utc>>;
    let rows: Vec<(Vec<String>, Vec<String>, At, At)> = sqlx::query_as(
        "SELECT territory_set, use_set, start_at, end_exclusive FROM rights.grant_atoms
         WHERE org_id=$1 AND revoked_at IS NULL AND grant_class='DISTRIBUTION'
           AND ((target_kind='RELEASE' AND target_id=$2) OR (target_kind='TRACK' AND target_id=ANY($3)))",
    )
    .bind(c.org_id)
    .bind(c.release_id)
    .bind(&tracks)
    .fetch_all(pool)
    .await?;
    // Licence instants are compared as KST dates (release dates are KST).
    let kst = |d: chrono::DateTime<chrono::Utc>| d + chrono::Duration::hours(9);
    Ok(rows
        .into_iter()
        .map(|(territories, uses, start, end)| ScopeGrant {
            territories,
            uses,
            start: start.map(|d| kst(d).date_naive()),
            // An exclusive instant later than midnight still licenses that day.
            end_exclusive: end.map(|d| {
                let d = kst(d);
                let day = d.date_naive();
                if d.time() == chrono::NaiveTime::MIN {
                    day
                } else {
                    day.succ_opt().unwrap_or(day)
                }
            }),
        })
        .collect())
}

/// Pre-release start in the draft: absent/empty, or a date 1–90 days before
/// the release date.
pub fn preorder_start(draft: &Value, release_date: NaiveDate) -> Result<Option<NaiveDate>> {
    let bad = || Error::InvalidCode("PREORDER_START_INVALID");
    match draft.get("preorder_start") {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(s)) if s.is_empty() => Ok(None),
        Some(Value::String(s)) => {
            let d = NaiveDate::parse_from_str(s, "%Y-%m-%d").map_err(|_| bad())?;
            let lead = (release_date - d).num_days();
            if !(1..=90).contains(&lead) {
                return Err(bad());
            }
            Ok(Some(d))
        }
        _ => Err(bad()),
    }
}

/// Preview start points the studio keeps on each draft track (`serverId`,
/// `previewStart` seconds, 0–3600). Unknown or empty entries are skipped.
pub fn preview_starts(draft: &Value) -> Result<std::collections::BTreeMap<Uuid, u32>> {
    let mut out = std::collections::BTreeMap::new();
    for t in draft
        .get("draftTracks")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        let Some(id) = t
            .get("serverId")
            .and_then(Value::as_str)
            .and_then(|v| v.parse::<Uuid>().ok())
        else {
            continue;
        };
        let secs = match t.get("previewStart") {
            None | Some(Value::Null) => continue,
            Some(Value::String(s)) if s.trim().is_empty() => continue,
            Some(Value::Number(n)) => n.as_u64(),
            Some(Value::String(s)) => s.trim().parse::<u64>().ok(),
            _ => None,
        };
        match secs {
            Some(v) if v <= 3600 => {
                out.insert(id, v as u32);
            }
            _ => return Err(Error::InvalidCode("PREVIEW_START_INVALID")),
        }
    }
    Ok(out)
}

/// The draft's `release_time`: absent/empty, or "HH:MM" KST on the hour or
/// half hour (what the studio offers).
pub fn release_time(draft: &Value) -> Result<Option<String>> {
    let bad = || Error::InvalidCode("RELEASE_TIME_INVALID");
    match draft.get("release_time") {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(s)) if s.is_empty() => Ok(None),
        Some(Value::String(s)) => {
            let t = chrono::NaiveTime::parse_from_str(s, "%H:%M").map_err(|_| bad())?;
            if s.len() != 5 || !matches!(chrono::Timelike::minute(&t), 0 | 30) {
                return Err(bad());
            }
            Ok(Some(s.clone()))
        }
        _ => Err(bad()),
    }
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
        crate::upload_safety::require_verified(pool, org_id, id).await?;
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

/// The wizard's `originalDate` (최초 발매일, 재발매일 때만). It must not be
/// after the new release date; an empty value means a first release.
pub fn original_release_date(draft: &Value, release_date: NaiveDate) -> Result<Option<NaiveDate>> {
    let Some(raw) = draft
        .get("originalDate")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
    else {
        return Ok(None);
    };
    let d = NaiveDate::parse_from_str(raw, "%Y-%m-%d")
        .map_err(|_| Error::PolicyGate("PREPARATION_ORIGINAL_DATE_INVALID"))?;
    if d > release_date {
        return Err(Error::PolicyGate("PREPARATION_ORIGINAL_DATE_INVALID"));
    }
    Ok(Some(d))
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
        let artist_name = draft_field(&draft, "artist", "PREPARATION_ARTIST_MISSING")?;
        let artist_platform_ids = {
            let mut conn = pool.acquire().await?;
            crate::artist_pages::platform_ids(&mut conn, c.org_id, &artist_name).await?
        };
        let original_release_date = original_release_date(&draft, release_date)?;
        let grants = scope_grants(pool, c).await?;
        let contract = {
            let mut conn = pool.acquire().await?;
            crate::contract_scope::load(&mut conn, c.org_id, c.release_id).await?
        };
        let scope = rights_scope(
            crate::territory::Territories::from_draft(&draft)?,
            &grants,
            contract.as_ref(),
            release_date,
        )?;

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
            artist: artist_name,
            release_type: c.release_type.clone(),
            release_date,
            original_release_date,
            language: draft_field(&draft, "language", "PREPARATION_LANGUAGE_MISSING")?,
            p_line: draft_field(&draft, "p_line", "PREPARATION_P_LINE_MISSING")?,
            c_line: draft_field(&draft, "c_line", "PREPARATION_C_LINE_MISSING")?,
            upc,
            tracks,
            artwork,
            explicit: c.explicit,
            territories: scope.territories,
            rights_end: scope.rights_end,
            permitted_uses: scope.permitted_uses,
            release_time: release_time(&draft)?,
            preorder_start: preorder_start(&draft, release_date)?,
            artist_platform_ids,
            preview_starts: {
                let ids: std::collections::BTreeSet<Uuid> =
                    c.tracks.iter().map(|t| t.track_id).collect();
                preview_starts(&draft)?
                    .into_iter()
                    .filter(|(id, _)| ids.contains(id))
                    .collect()
            },
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

#[cfg(test)]
mod release_time_tests {
    use super::release_time;
    use serde_json::json;

    #[test]
    fn hour_or_half_hour_or_nothing() {
        assert_eq!(release_time(&json!({})).unwrap(), None);
        assert_eq!(release_time(&json!({"release_time": ""})).unwrap(), None);
        assert_eq!(
            release_time(&json!({"release_time": "18:00"}))
                .unwrap()
                .as_deref(),
            Some("18:00")
        );
        assert_eq!(
            release_time(&json!({"release_time": "00:30"}))
                .unwrap()
                .as_deref(),
            Some("00:30")
        );
        for bad in [
            json!("18:15"),
            json!("24:00"),
            json!("8:00"),
            json!("18:00:00"),
            json!(18),
        ] {
            assert!(
                release_time(&json!({"release_time": bad})).is_err(),
                "{bad}"
            );
        }
    }
}

#[cfg(test)]
mod prerelease_tests {
    use super::{preorder_start, preview_starts};
    use chrono::NaiveDate;
    use serde_json::json;

    #[test]
    fn preorder_within_ninety_days_before_release() {
        let rd = NaiveDate::from_ymd_opt(2027, 3, 1).unwrap();
        assert_eq!(preorder_start(&json!({}), rd).unwrap(), None);
        assert_eq!(
            preorder_start(&json!({"preorder_start": "2027-02-15"}), rd).unwrap(),
            NaiveDate::from_ymd_opt(2027, 2, 15)
        );
        for bad in ["2027-03-01", "2027-03-02", "2026-11-01", "soon"] {
            assert!(
                preorder_start(&json!({"preorder_start": bad}), rd).is_err(),
                "{bad}"
            );
        }
    }

    #[test]
    fn preview_starts_by_server_track() {
        let id = uuid::Uuid::new_v4();
        let d = json!({"draftTracks": [{"serverId": id.to_string(), "previewStart": "45"}, {"previewStart": 10}, {"serverId": "x"}]});
        assert_eq!(preview_starts(&d).unwrap().get(&id), Some(&45));
        let d = json!({"draftTracks": [{"serverId": id.to_string(), "previewStart": -3}]});
        assert!(preview_starts(&d).is_err());
    }
}

#[cfg(test)]
mod scope_tests {
    use super::*;
    use crate::{contract_scope::ContractScope, territory::Territories};

    fn day(y: i32, m: u32, d: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(y, m, d).unwrap()
    }
    fn grant(t: &[&str], u: &[&str], end: Option<NaiveDate>) -> ScopeGrant {
        ScopeGrant {
            territories: t.iter().map(|s| s.to_string()).collect(),
            uses: u.iter().map(|s| s.to_string()).collect(),
            start: None,
            end_exclusive: end,
        }
    }
    fn only(c: &[&str]) -> Territories {
        Territories::Only(c.iter().map(|s| s.to_string()).collect())
    }

    #[test]
    fn grants_narrow_territory_period_and_uses() {
        let release = day(2027, 3, 1);
        let w = Territories::Worldwide;
        // No grants, no contract: the application alone decides.
        let s = rights_scope(w.clone(), &[], None, release).unwrap();
        assert!(s.territories.is_worldwide() && s.rights_end.is_none());
        assert!(s.permitted_uses.is_empty());
        // Worldwide application, grants for KR+JP streaming until 2028.
        let g = [grant(
            &["KR", "JP"],
            &["OnDemandStream"],
            Some(day(2028, 1, 1)),
        )];
        let s = rights_scope(w.clone(), &g, None, release).unwrap();
        assert_eq!(s.territories, only(&["JP", "KR"]));
        assert_eq!(s.rights_end, Some(day(2027, 12, 31)));
        assert_eq!(s.permitted_uses, vec!["OnDemandStream"]);
        // Requested KR+US with KR+JP licensed: only KR is sent.
        let s = rights_scope(only(&["KR", "US"]), &g, None, release).unwrap();
        assert_eq!(s.territories, only(&["KR"]));
        // Worldwide except KR with KR+JP licensed: JP.
        let s = rights_scope(
            Territories::WorldExcept(vec!["KR".into()]),
            &g,
            None,
            release,
        );
        assert_eq!(s.unwrap().territories, only(&["JP"]));
        // Nothing licensed where requested, or on the release date: fail closed.
        assert!(matches!(
            rights_scope(only(&["US"]), &g, None, release),
            Err(Error::PolicyGate("PREPARATION_RIGHTS_TERRITORY"))
        ));
        assert!(matches!(
            rights_scope(
                w.clone(),
                &[grant(&["WW"], &[], Some(day(2027, 1, 1)))],
                None,
                release
            ),
            Err(Error::PolicyGate("PREPARATION_RIGHTS_PERIOD"))
        ));
        // A worldwide grant keeps the requested territories.
        let s = rights_scope(only(&["KR"]), &[grant(&["WW"], &[], None)], None, release).unwrap();
        assert_eq!(s.territories, only(&["KR"]));
    }

    #[test]
    fn the_contract_narrows_on_top_of_the_grants() {
        let release = day(2027, 3, 1);
        let contract = ContractScope {
            territories: Territories::WorldExcept(vec!["CN".into()]),
            end: Some(day(2027, 6, 30)),
            dsps: None,
            ugc: false,
        };
        let s = rights_scope(Territories::Worldwide, &[], Some(&contract), release).unwrap();
        assert_eq!(s.territories, Territories::WorldExcept(vec!["CN".into()]));
        assert_eq!(s.rights_end, Some(day(2027, 6, 30)));
        // The earlier of the grant end and the contract end wins.
        let g = [grant(&[], &[], Some(day(2027, 5, 1)))];
        let s = rights_scope(Territories::Worldwide, &g, Some(&contract), release).unwrap();
        assert_eq!(s.rights_end, Some(day(2027, 4, 30)));
        assert!(matches!(
            rights_scope(only(&["CN"]), &[], Some(&contract), release),
            Err(Error::PolicyGate("PREPARATION_CONTRACT_TERRITORY"))
        ));
        // Ending before the release date: no deal end (staging blocks it).
        let s = rights_scope(
            Territories::Worldwide,
            &[],
            Some(&contract),
            day(2027, 7, 1),
        );
        assert_eq!(s.unwrap().rights_end, None);
    }
}
