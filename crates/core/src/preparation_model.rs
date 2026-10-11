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
    /// Territories the release may be offered in (ISO 3166-1 alpha-2): the
    /// application's 배급 지역, narrowed to the registered rights grants.
    /// Empty means Worldwide. Skipped when empty so earlier snapshots keep
    /// their hash.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub territories: Vec<String>,
    /// Territories the agreement excludes from a Worldwide deal (DDEX
    /// ExcludedTerritoryCode). Only used when `territories` is empty.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub excluded_territories: Vec<String>,
    /// The grants' licence end (exclusive): the deal's EndDate.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rights_end: Option<NaiveDate>,
    /// DDEX UseTypes the grants allow. Empty means not restricted; a DSP
    /// whose deal needs another use is blocked at staging (RIGHTS_SCOPE_USE).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub permitted_uses: Vec<String>,
}

/// Contract scope for delivery (감사 3번: Worldwide 고정 제거).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RightsScope {
    pub territories: Vec<String>,
    pub excluded_territories: Vec<String>,
    pub rights_end: Option<NaiveDate>,
    pub permitted_uses: Vec<String>,
}

const WORLD: &[&str] = &["WORLD", "WW", "WORLDWIDE"];

fn territory_code(raw: &str) -> Result<Option<String>> {
    let code = raw.trim().to_ascii_uppercase();
    if WORLD.contains(&code.as_str()) {
        return Ok(None);
    }
    if code.len() == 2 && code.bytes().all(|b| b.is_ascii_uppercase()) {
        Ok(Some(code))
    } else {
        Err(Error::PolicyGate("PREPARATION_TERRITORY_INVALID"))
    }
}

/// The application's 배급 지역 (`territories`): `None` = Worldwide. A legacy
/// draft without the field is Worldwide (the wizard always sent `WORLD`);
/// an explicitly empty list licenses nowhere and fails closed.
pub fn application_territories(
    draft: &Value,
) -> Result<Option<std::collections::BTreeSet<String>>> {
    let Some(list) = draft.get("territories") else {
        return Ok(None);
    };
    let list = list
        .as_array()
        .ok_or(Error::PolicyGate("PREPARATION_TERRITORY_INVALID"))?;
    if list.is_empty() {
        return Err(Error::PolicyGate("PREPARATION_TERRITORY_MISSING"));
    }
    let mut codes = std::collections::BTreeSet::new();
    for v in list {
        match territory_code(
            v.as_str()
                .ok_or(Error::PolicyGate("PREPARATION_TERRITORY_INVALID"))?,
        )? {
            None => return Ok(None),
            Some(c) => {
                codes.insert(c);
            }
        }
    }
    Ok(Some(codes))
}

/// One registered rights grant, as far as delivery scope is concerned.
#[derive(Debug, Clone)]
pub struct ScopeGrant {
    pub territories: Vec<String>,
    pub uses: Vec<String>,
    pub start: Option<NaiveDate>,
    pub end_exclusive: Option<NaiveDate>,
}

/// Narrow the application's territories to what the release's rights grants
/// allow on the release date. Without grants the application decides alone.
/// A grant's empty territory/use set means "not restricted"; `WW`/`WORLD`
/// is everywhere. Fails closed when nothing is licensed on the release date
/// or in any requested territory.
pub fn rights_scope(
    requested: Option<std::collections::BTreeSet<String>>,
    grants: &[ScopeGrant],
    release_date: NaiveDate,
) -> Result<RightsScope> {
    use std::collections::BTreeSet;
    if grants.is_empty() {
        return Ok(RightsScope {
            territories: requested
                .map(|s| s.into_iter().collect())
                .unwrap_or_default(),
            ..RightsScope::default()
        });
    }
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
    // Union of the licensed territories (None = everywhere).
    let mut licensed: Option<BTreeSet<String>> = Some(BTreeSet::new());
    for g in &live {
        let mut world = g.territories.is_empty();
        let mut set = BTreeSet::new();
        for t in &g.territories {
            match territory_code(t)? {
                None => world = true,
                Some(c) => {
                    set.insert(c);
                }
            }
        }
        if world {
            licensed = None;
            break;
        }
        if let Some(l) = licensed.as_mut() {
            l.extend(set);
        }
    }
    let territories: Vec<String> = match (requested, licensed) {
        (None, None) => vec![],
        (Some(r), None) => r.into_iter().collect(),
        (None, Some(l)) => l.into_iter().collect(),
        (Some(r), Some(l)) => r.intersection(&l).cloned().collect(),
    };
    if territories.is_empty() && grants_restrict_territory(&live) {
        return Err(Error::PolicyGate("PREPARATION_RIGHTS_TERRITORY"));
    }
    let rights_end = live.iter().filter_map(|g| g.end_exclusive).min();
    let uses = if live.iter().any(|g| g.uses.is_empty()) {
        vec![]
    } else {
        live.iter()
            .flat_map(|g| g.uses.iter().cloned())
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect()
    };
    Ok(RightsScope {
        territories,
        excluded_territories: vec![],
        rights_end,
        permitted_uses: uses,
    })
}

/// Apply the agreement's scope (contract_scope) on top of the application and
/// the rights grants: only the contract's territories, never its excluded
/// ones, and no later than its last day.
pub fn with_contract(
    mut scope: RightsScope,
    contract: Option<&crate::contract_scope::ContractScope>,
) -> Result<RightsScope> {
    let Some(c) = contract else {
        return Ok(scope);
    };
    let allowed = c.effective_territories();
    if scope.territories.is_empty() {
        match allowed {
            Some(a) => scope.territories = a.into_iter().collect(),
            None => scope.excluded_territories = c.excluded.iter().cloned().collect(),
        }
    } else {
        scope
            .territories
            .retain(|t| !c.excluded.contains(t) && allowed.as_ref().is_none_or(|a| a.contains(t)));
        if scope.territories.is_empty() {
            return Err(Error::PolicyGate("PREPARATION_CONTRACT_TERRITORY"));
        }
    }
    if let Some(last) = c.end {
        let end = last.succ_opt().unwrap_or(last);
        scope.rights_end = Some(scope.rights_end.map_or(end, |e| e.min(end)));
    }
    Ok(scope)
}

fn grants_restrict_territory(live: &[&ScopeGrant]) -> bool {
    !live.iter().any(|g| {
        g.territories.is_empty()
            || g.territories
                .iter()
                .any(|t| WORLD.contains(&t.trim().to_ascii_uppercase().as_str()))
    })
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
        let original_release_date = original_release_date(&draft, release_date)?;
        let grants = scope_grants(pool, c).await?;
        let contract = {
            let mut conn = pool.acquire().await?;
            crate::contract_scope::load(&mut conn, c.org_id, c.release_id).await?
        };
        let scope = with_contract(
            rights_scope(application_territories(&draft)?, &grants, release_date)?,
            contract.as_ref(),
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
            artist: draft_field(&draft, "artist", "PREPARATION_ARTIST_MISSING")?,
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
            excluded_territories: scope.excluded_territories,
            rights_end: scope.rights_end,
            permitted_uses: scope.permitted_uses,
        })
    }
}

/// The release's active rights grants (release- or track-level).
async fn scope_grants(pool: &PgPool, c: &CanonicalRelease) -> Result<Vec<ScopeGrant>> {
    let tracks: Vec<Uuid> = c.tracks.iter().map(|t| t.track_id).collect();
    type At = Option<chrono::DateTime<chrono::Utc>>;
    let rows: Vec<(Vec<String>, Vec<String>, At, At)> =
        sqlx::query_as(
            "SELECT territory_set, use_set, start_at, end_exclusive FROM rights.grant_atoms
             WHERE org_id=$1 AND revoked_at IS NULL AND grant_class='DISTRIBUTION'
               AND ((target_kind='RELEASE' AND target_id=$2) OR (target_kind='TRACK' AND target_id=ANY($3)))",
        )
        .bind(c.org_id)
        .bind(c.release_id)
        .bind(&tracks)
        .fetch_all(pool)
        .await?;
    Ok(rows
        .into_iter()
        .map(|(territories, uses, start, end)| ScopeGrant {
            territories,
            uses,
            start: start.map(|d| d.date_naive()),
            // An exclusive instant later than midnight still licenses that day.
            end_exclusive: end.map(|d| {
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
mod scope_tests {
    use super::*;
    use serde_json::json;

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

    #[test]
    fn application_territories_are_world_or_iso_codes() {
        assert_eq!(application_territories(&json!({})).unwrap(), None);
        assert_eq!(
            application_territories(&json!({"territories":["WORLD"]})).unwrap(),
            None
        );
        assert_eq!(
            application_territories(&json!({"territories":["kr","JP","KR"]})).unwrap(),
            Some(["JP".to_string(), "KR".to_string()].into())
        );
        assert!(matches!(
            application_territories(&json!({"territories":[]})),
            Err(Error::PolicyGate("PREPARATION_TERRITORY_MISSING"))
        ));
        assert!(application_territories(&json!({"territories":["Korea"]})).is_err());
    }

    #[test]
    fn grants_narrow_territory_period_and_uses() {
        let release = day(2027, 3, 1);
        // No grants: the application alone decides.
        let s = rights_scope(None, &[], release).unwrap();
        assert!(s.territories.is_empty() && s.rights_end.is_none() && s.permitted_uses.is_empty());
        // Worldwide application, grants for KR+JP streaming until 2028.
        let g = [grant(
            &["KR", "JP"],
            &["OnDemandStream"],
            Some(day(2028, 1, 1)),
        )];
        let s = rights_scope(None, &g, release).unwrap();
        assert_eq!(s.territories, vec!["JP", "KR"]);
        assert_eq!(s.rights_end, Some(day(2028, 1, 1)));
        assert_eq!(s.permitted_uses, vec!["OnDemandStream"]);
        // Requested KR+US with KR+JP licensed: only KR is sent.
        let s = rights_scope(Some(["KR".into(), "US".into()].into()), &g, release).unwrap();
        assert_eq!(s.territories, vec!["KR"]);
        // Nothing licensed where requested, or on the release date: fail closed.
        assert!(matches!(
            rights_scope(Some(["US".into()].into()), &g, release),
            Err(Error::PolicyGate("PREPARATION_RIGHTS_TERRITORY"))
        ));
        assert!(matches!(
            rights_scope(None, &[grant(&["WW"], &[], Some(day(2027, 1, 1)))], release),
            Err(Error::PolicyGate("PREPARATION_RIGHTS_PERIOD"))
        ));
        // A worldwide grant keeps the requested territories.
        let s = rights_scope(
            Some(["KR".into()].into()),
            &[grant(&["WW"], &[], None)],
            release,
        )
        .unwrap();
        assert_eq!(s.territories, vec!["KR"]);
    }
}
