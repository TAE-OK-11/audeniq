//! 권리 서류 승인 → 실제 권한 (감사 3번, migration 0093).
//!
//! When staff approve a rights proof they record what it proves. Each grant
//! names the work (the release, or specific tracks), the rights holder, the
//! right (MASTER, COMPOSITION …), and either
//!   * DISTRIBUTION — the holder lets the release owner distribute, within
//!     territories / uses / a period (`preparation_model::rights_scope` and
//!     staging read these), or
//!   * ROYALTY — the holder receives `share_bps` of the covered works'
//!     royalties (the settlement entitlement, kept apart from distribution).
//!
//! A grant moves the release's rights epoch (0020 trigger). A release waiting
//! for its first delivery is sent back through Stage 2 at once, so delivery
//! does not stall on a stale epoch; the artist is not asked to correct anything.
use crate::{
    error::{Error, Result},
    operations,
};
use chrono::{NaiveDate, TimeZone, Utc};
use serde::Deserialize;
use serde_json::{Value, json};
use sqlx::{PgConnection, Row};
use uuid::Uuid;

pub const RIGHT_TYPES: &[&str] = &[
    "MASTER",
    "COMPOSITION",
    "LYRICS",
    "PERFORMANCE",
    "ARTWORK",
    "SAMPLE",
    "OTHER",
];

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GrantInput {
    /// DISTRIBUTION | ROYALTY
    pub grant_class: String,
    /// One of [`RIGHT_TYPES`].
    pub right_type: String,
    /// The rights holder as named in the document.
    pub rights_holder: String,
    /// Tracks the grant covers; empty = the whole release.
    #[serde(default)]
    pub track_ids: Vec<Uuid>,
    /// ROYALTY only: the holder's share, basis points (2500 = 25%).
    #[serde(default)]
    pub share_bps: Option<u32>,
    /// ISO 3166-1 alpha-2 codes; empty or WORLD = everywhere.
    #[serde(default)]
    pub territories: Vec<String>,
    /// DDEX UseTypes (OnDemandStream, PermanentDownload …); empty = any use.
    #[serde(default)]
    pub uses: Vec<String>,
    #[serde(default)]
    pub start_date: Option<String>,
    /// Last licensed day (inclusive).
    #[serde(default)]
    pub end_date: Option<String>,
    #[serde(default)]
    pub exclusive: bool,
}

fn date(raw: &Option<String>) -> Result<Option<NaiveDate>> {
    raw.as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(|s| {
            NaiveDate::parse_from_str(s, "%Y-%m-%d")
                .map_err(|_| Error::InvalidCode("GRANT_DATE_INVALID"))
        })
        .transpose()
}

fn territories(list: &[String]) -> Result<Vec<String>> {
    let mut out = Vec::new();
    for t in list {
        let c = t.trim().to_ascii_uppercase();
        if matches!(c.as_str(), "WORLD" | "WW" | "WORLDWIDE") {
            return Ok(vec![]);
        }
        if c.len() != 2 || !c.bytes().all(|b| b.is_ascii_uppercase()) {
            return Err(Error::InvalidCode("GRANT_TERRITORY_INVALID"));
        }
        if !out.contains(&c) {
            out.push(c);
        }
    }
    Ok(out)
}

/// Validated grant, before parties are resolved.
struct Checked<'a> {
    input: &'a GrantInput,
    territories: Vec<String>,
    uses: Vec<String>,
    start: Option<NaiveDate>,
    end: Option<NaiveDate>,
}

fn check(i: &GrantInput) -> Result<Checked<'_>> {
    if !matches!(i.grant_class.as_str(), "DISTRIBUTION" | "ROYALTY") {
        return Err(Error::InvalidCode("GRANT_CLASS_INVALID"));
    }
    if !RIGHT_TYPES.contains(&i.right_type.as_str()) {
        return Err(Error::InvalidCode("GRANT_RIGHT_TYPE_INVALID"));
    }
    let holder = i.rights_holder.trim();
    if holder.is_empty() || holder.chars().count() > 120 {
        return Err(Error::InvalidCode("GRANT_HOLDER_REQUIRED"));
    }
    crate::text_policy::check(holder)?;
    match (i.grant_class.as_str(), i.share_bps) {
        ("ROYALTY", Some(s)) if (1..=10_000).contains(&s) => {}
        ("ROYALTY", _) => return Err(Error::InvalidCode("GRANT_SHARE_REQUIRED")),
        (_, Some(_)) => return Err(Error::InvalidCode("GRANT_SHARE_NOT_ALLOWED")),
        _ => {}
    }
    let uses: Vec<String> = i
        .uses
        .iter()
        .map(|u| u.trim().to_string())
        .filter(|u| !u.is_empty())
        .collect();
    if uses
        .iter()
        .any(|u| u.len() > 40 || !u.bytes().all(|b| b.is_ascii_alphabetic()))
    {
        return Err(Error::InvalidCode("GRANT_USE_INVALID"));
    }
    let (start, end) = (date(&i.start_date)?, date(&i.end_date)?);
    if let (Some(s), Some(e)) = (start, end)
        && e < s
    {
        return Err(Error::InvalidCode("GRANT_DATE_INVALID"));
    }
    if i.track_ids.len() > 500 {
        return Err(Error::Invalid);
    }
    Ok(Checked {
        input: i,
        territories: territories(&i.territories)?,
        uses,
        start,
        end,
    })
}

/// The party that owns the release (its first writer ACL principal).
pub async fn owner_party(c: &mut PgConnection, org: Uuid, release: Uuid) -> Result<Uuid> {
    sqlx::query_scalar(
        "SELECT principal_party_id FROM identity.resource_acl
          WHERE org_id=$1 AND resource_id=$2 AND action='write' AND revoked_at IS NULL
          ORDER BY starts_at LIMIT 1",
    )
    .bind(org)
    .bind(release)
    .fetch_optional(&mut *c)
    .await?
    .ok_or(Error::PolicyGate("RELEASE_OWNER_UNKNOWN"))
}

/// The holder as a party of the org (reused by exact name).
async fn holder_party(c: &mut PgConnection, org: Uuid, name: &str) -> Result<Uuid> {
    if let Some(id) = sqlx::query_scalar::<_, Uuid>(
        "SELECT id FROM identity.parties WHERE org_id=$1 AND display_name=$2 ORDER BY id LIMIT 1",
    )
    .bind(org)
    .bind(name)
    .fetch_optional(&mut *c)
    .await?
    {
        return Ok(id);
    }
    let id = Uuid::new_v4();
    sqlx::query(
        "INSERT INTO identity.parties(id,org_id,kind,display_name) VALUES($1,$2,'PERSON',$3)",
    )
    .bind(id)
    .bind(org)
    .bind(name)
    .execute(&mut *c)
    .await?;
    Ok(id)
}

/// Record the grants an approved rights document proves. Returns their ids.
pub async fn record(
    c: &mut PgConnection,
    org: Uuid,
    release: Uuid,
    document: Uuid,
    grants: &[GrantInput],
) -> Result<Vec<Uuid>> {
    if grants.is_empty() {
        return Err(Error::PolicyGate("GRANTS_REQUIRED"));
    }
    if grants.len() > 50 {
        return Err(Error::Invalid);
    }
    let checked = grants.iter().map(check).collect::<Result<Vec<_>>>()?;
    let tracks: Vec<Uuid> = sqlx::query_scalar(
        "SELECT id FROM catalog.tracks WHERE org_id=$1 AND release_id=$2 AND archived_at IS NULL",
    )
    .bind(org)
    .bind(release)
    .fetch_all(&mut *c)
    .await?;
    let owner = owner_party(c, org, release).await?;
    let mut ids = Vec::new();
    for g in checked {
        if g.input.track_ids.iter().any(|t| !tracks.contains(t)) {
            return Err(Error::InvalidCode("GRANT_TRACK_UNKNOWN"));
        }
        let holder = holder_party(c, org, g.input.rights_holder.trim()).await?;
        let royalty = g.input.grant_class == "ROYALTY";
        // DISTRIBUTION: the holder lets the owner distribute.
        // ROYALTY: the holder is paid; the owner grants the share.
        let (party, grantor) = if royalty {
            (holder, owner)
        } else {
            (owner, holder)
        };
        let targets: Vec<(&str, Uuid)> = if g.input.track_ids.is_empty() {
            vec![("RELEASE", release)]
        } else {
            g.input.track_ids.iter().map(|t| ("TRACK", *t)).collect()
        };
        for (kind, target) in targets {
            if royalty {
                // Shares of one right on one work never exceed 100%.
                let taken: i64 = sqlx::query_scalar(
                    "SELECT COALESCE(sum(share_bps),0) FROM rights.grant_atoms
                      WHERE org_id=$1 AND grant_class='ROYALTY' AND revoked_at IS NULL AND right_type=$4
                        AND ((target_kind=$2 AND target_id=$3) OR (target_kind='RELEASE' AND target_id=$5))",
                )
                .bind(org)
                .bind(kind)
                .bind(target)
                .bind(&g.input.right_type)
                .bind(release)
                .fetch_one(&mut *c)
                .await?;
                if taken + i64::from(g.input.share_bps.unwrap_or(0)) > 10_000 {
                    return Err(Error::PolicyGate("GRANT_SHARE_EXCEEDS_100"));
                }
            }
            let id = Uuid::new_v4();
            sqlx::query(
                "INSERT INTO rights.grant_atoms(id,org_id,party_id,target_kind,target_id,right_type,territory_set,use_set,
                   start_at,end_exclusive,exclusive,grant_class,grantor_party_id,share_bps,source_document_id)
                 VALUES($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14,$15)",
            )
            .bind(id)
            .bind(org)
            .bind(party)
            .bind(kind)
            .bind(target)
            .bind(&g.input.right_type)
            .bind(&g.territories)
            .bind(&g.uses)
            .bind(g.start.map(|d| Utc.from_utc_datetime(&d.and_hms_opt(0, 0, 0).unwrap())))
            .bind(g.end.and_then(|d| d.succ_opt()).map(|d| Utc.from_utc_datetime(&d.and_hms_opt(0, 0, 0).unwrap())))
            .bind(g.input.exclusive && !royalty)
            .bind(&g.input.grant_class)
            .bind(grantor)
            .bind(g.input.share_bps.map(|s| s as i32))
            .bind(document)
            .execute(&mut *c)
            .await?;
            ids.push(id);
        }
    }
    Ok(ids)
}

/// After a grant moved the epoch: a release waiting for its first delivery
/// re-runs Stage 2 now (same path as preparation's return_to_s2), instead of
/// stalling at the send's epoch guard. Delivered releases pick the grants up
/// with their next revision.
pub async fn recheck(
    c: &mut PgConnection,
    org: Uuid,
    release: Uuid,
    request: Uuid,
) -> Result<Option<Value>> {
    let row = sqlx::query(
        "SELECT status, current_revision_id FROM catalog.releases WHERE org_id=$1 AND id=$2 FOR UPDATE",
    )
    .bind(org)
    .bind(release)
    .fetch_one(&mut *c)
    .await?;
    if row.get::<String, _>("status") != "READY_FOR_DELIVERY" {
        return Ok(None);
    }
    sqlx::query("SELECT set_config('app.org_id',$1,true)")
        .bind(org.to_string())
        .execute(&mut *c)
        .await?;
    if crate::post_release::delivered_before(c, org, release).await? {
        return Ok(None);
    }
    let revision: Uuid = row.get("current_revision_id");
    sqlx::query("SELECT set_config('audeniq.rights_recheck','1',true)")
        .execute(&mut *c)
        .await?;
    sqlx::query(
        "UPDATE catalog.releases SET status='STAGE3_CORRECTION', row_version=row_version+1
          WHERE org_id=$1 AND id=$2 AND status='READY_FOR_DELIVERY'",
    )
    .bind(org)
    .bind(release)
    .execute(&mut *c)
    .await?;
    sqlx::query("SELECT set_config('audeniq.rights_recheck','',true)")
        .execute(&mut *c)
        .await?;
    sqlx::query(
        "INSERT INTO operations.jobs(id, queue, kind, payload, pinned_revision_id, idempotency_key, release_id)
         VALUES($1,'rights','stage2',$2,$3,$4,$5)",
    )
    .bind(Uuid::new_v4())
    .bind(json!({"revision_id": revision, "reason": "RIGHTS_GRANTS_RECORDED"}))
    .bind(revision)
    .bind(format!("stage2-rerun:{revision}:grants:{}", Uuid::new_v4()))
    .bind(release)
    .execute(&mut *c)
    .await?;
    operations::audit(
        c,
        None,
        Some(org),
        Some(release),
        "rights.grants_recheck",
        "READY_FOR_DELIVERY->STAGE3_CORRECTION",
        request,
    )
    .await?;
    Ok(Some(
        json!({"status": "STAGE3_CORRECTION", "stage2": "QUEUED"}),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn g(class: &str, share: Option<u32>) -> GrantInput {
        GrantInput {
            grant_class: class.into(),
            right_type: "COMPOSITION".into(),
            rights_holder: "작곡가".into(),
            track_ids: vec![],
            share_bps: share,
            territories: vec!["kr".into()],
            uses: vec!["OnDemandStream".into()],
            start_date: Some("2026-01-01".into()),
            end_date: Some("2028-12-31".into()),
            exclusive: false,
        }
    }

    #[test]
    fn distribution_and_royalty_rules() {
        assert!(check(&g("DISTRIBUTION", None)).is_ok());
        assert!(check(&g("ROYALTY", Some(2500))).is_ok());
        assert!(check(&g("ROYALTY", None)).is_err(), "royalty needs a share");
        assert!(
            check(&g("DISTRIBUTION", Some(10))).is_err(),
            "distribution has no share"
        );
        let dist = g("DISTRIBUTION", None);
        assert_eq!(check(&dist).unwrap().territories, vec!["KR"]);
        let mut bad = g("DISTRIBUTION", None);
        bad.end_date = Some("2025-01-01".into());
        assert!(check(&bad).is_err());
        bad = g("DISTRIBUTION", None);
        bad.territories = vec!["WORLD".into()];
        assert!(check(&bad).unwrap().territories.is_empty());
    }
}
