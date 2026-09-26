//! Mutable draft operations only. These do not issue revisions or approve content.
use crate::{
    api::AppState,
    auth::{self, Actor},
    catalog::TrackInput,
    error::{Error, Result},
    operations,
};
use serde::Deserialize;
use serde_json::{Value, json};
use sqlx::{PgConnection, Row};
use std::collections::BTreeSet;
use uuid::Uuid;

pub async fn bump(c: &mut PgConnection, org: Uuid, release: Uuid, version: i64) -> Result<()> {
    let n = sqlx::query("UPDATE catalog.releases SET row_version=row_version+1 WHERE org_id=$1 AND id=$2 AND row_version=$3 AND catalog.is_editable_status(status) AND archived_at IS NULL")
        .bind(org).bind(release).bind(version).execute(c).await?.rows_affected();
    if n != 1 {
        return Err(Error::Conflict);
    }
    Ok(())
}
async fn changed(
    c: &mut PgConnection,
    a: &Actor,
    org: Uuid,
    release: Uuid,
    version: i64,
    action: &str,
) -> Result<()> {
    operations::audit(
        c,
        Some(a.user),
        Some(org),
        Some(release),
        action,
        "USER_EDIT",
        a.request,
    )
    .await?;
    operations::event(
        c,
        org,
        release,
        "release.updated",
        &format!("draft:{release}:{version}"),
    )
    .await?;
    Ok(())
}
pub async fn track_refs(c: &mut PgConnection, a: &Actor, org: Uuid, i: &TrackInput) -> Result<()> {
    check_track_fields(
        &i.title,
        i.disc_number,
        i.track_number,
        i.lyrics.as_deref(),
        i.version.as_deref(),
    )?;
    crate::protected_names::enforce(
        c,
        org,
        &[i.title.as_str(), i.version.as_deref().unwrap_or("")],
    )
    .await?;
    auth::authorize(c, a, org, i.artist_id, "artist", false).await?;
    let artist: Option<Uuid> = sqlx::query_scalar("SELECT id FROM catalog.artists WHERE org_id=$1 AND id=$2 AND archived_at IS NULL FOR SHARE")
        .bind(org).bind(i.artist_id).fetch_optional(&mut *c).await?;
    artist.ok_or(Error::Conflict)?;
    if let Some(asset) = i.asset_id {
        auth::authorize(c, a, org, asset, "asset", false).await?;
    }
    Ok(())
}
pub async fn replace_track(
    s: &AppState,
    a: &Actor,
    org: Uuid,
    release: Uuid,
    track: Uuid,
    i: TrackInput,
) -> Result<Value> {
    let mut tx = s.pool.begin().await?;
    auth::authorize(&mut tx, a, org, release, "release", true).await?;
    track_refs(&mut tx, a, org, &i).await?;
    bump(&mut tx, org, release, i.row_version).await?;
    let lyrics = i.lyrics.as_deref().filter(|s| !s.trim().is_empty());
    let version = i
        .version
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or("");
    let n = sqlx::query("UPDATE catalog.tracks SET title=$4,version=$11,disc_number=$5,track_number=$6,artist_id=$7,asset_id=$8,lyrics=$9,parental_advisory=$10 WHERE org_id=$1 AND release_id=$2 AND id=$3 AND archived_at IS NULL")
        .bind(org).bind(release).bind(track).bind(i.title).bind(i.disc_number).bind(i.track_number).bind(i.artist_id).bind(i.asset_id).bind(lyrics).bind(i.parental_advisory.unwrap_or(false)).bind(version).execute(&mut *tx).await?.rows_affected();
    if n != 1 {
        return Err(Error::NotFound);
    }
    changed(
        &mut tx,
        a,
        org,
        release,
        i.row_version + 1,
        "release.track_updated",
    )
    .await?;
    tx.commit().await?;
    Ok(json!({"id":track,"row_version":i.row_version+1}))
}
pub async fn archive_track(
    s: &AppState,
    a: &Actor,
    org: Uuid,
    release: Uuid,
    track: Uuid,
    version: i64,
) -> Result<Value> {
    let mut tx = s.pool.begin().await?;
    auth::authorize(&mut tx, a, org, release, "release", true).await?;
    bump(&mut tx, org, release, version).await?;
    let n = sqlx::query("UPDATE catalog.tracks SET archived_at=now() WHERE org_id=$1 AND release_id=$2 AND id=$3 AND archived_at IS NULL")
        .bind(org).bind(release).bind(track).execute(&mut *tx).await?.rows_affected();
    if n != 1 {
        return Err(Error::NotFound);
    }
    changed(
        &mut tx,
        a,
        org,
        release,
        version + 1,
        "release.track_archived",
    )
    .await?;
    tx.commit().await?;
    Ok(json!({"id":track,"archived":true,"row_version":version+1}))
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Credit {
    pub party_id: Uuid,
    pub role: String,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CreditsInput {
    pub row_version: i64,
    pub credits: Vec<Credit>,
}
pub async fn replace_credits(
    s: &AppState,
    a: &Actor,
    org: Uuid,
    release: Uuid,
    track: Uuid,
    i: CreditsInput,
) -> Result<Value> {
    let mut keys = BTreeSet::new();
    if i.credits.len() > 100
        || i.credits.iter().any(|v| {
            v.role.trim().is_empty()
                || v.role.len() > 80
                || v.role != v.role.trim()
                || !keys.insert((v.party_id, v.role.clone()))
        })
    {
        return Err(Error::Invalid);
    }
    for v in &i.credits {
        crate::text_policy::check(&v.role)?;
    }
    let mut tx = s.pool.begin().await?;
    auth::authorize(&mut tx, a, org, release, "release", true).await?;
    bump(&mut tx, org, release, i.row_version).await?;
    let id: Option<Uuid> = sqlx::query_scalar("SELECT id FROM catalog.tracks WHERE org_id=$1 AND release_id=$2 AND id=$3 AND archived_at IS NULL")
        .bind(org).bind(release).bind(track).fetch_optional(&mut *tx).await?;
    id.ok_or(Error::NotFound)?;
    let parties: Vec<Uuid> = i
        .credits
        .iter()
        .map(|v| v.party_id)
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect();
    let count: i64 =
        sqlx::query_scalar("SELECT count(*) FROM identity.parties WHERE org_id=$1 AND id=ANY($2)")
            .bind(org)
            .bind(&parties)
            .fetch_one(&mut *tx)
            .await?;
    if count != parties.len() as i64 {
        return Err(Error::Forbidden);
    }
    // Credited contributors are published: their names get the same
    // protected-artist block as artist names.
    let names: Vec<String> = sqlx::query_scalar(
        "SELECT display_name FROM identity.parties WHERE org_id=$1 AND id=ANY($2)",
    )
    .bind(org)
    .bind(&parties)
    .fetch_all(&mut *tx)
    .await?;
    let refs: Vec<&str> = names.iter().map(String::as_str).collect();
    crate::protected_names::enforce(&mut tx, org, &refs).await?;
    sqlx::query("DELETE FROM catalog.credits WHERE org_id=$1 AND track_id=$2")
        .bind(org)
        .bind(track)
        .execute(&mut *tx)
        .await?;
    for credit in i.credits {
        sqlx::query(
            "INSERT INTO catalog.credits(org_id,track_id,party_id,role) VALUES($1,$2,$3,$4)",
        )
        .bind(org)
        .bind(track)
        .bind(credit.party_id)
        .bind(credit.role)
        .execute(&mut *tx)
        .await?;
    }
    changed(
        &mut tx,
        a,
        org,
        release,
        i.row_version + 1,
        "release.credits_replaced",
    )
    .await?;
    tx.commit().await?;
    Ok(json!({"id":track,"row_version":i.row_version+1}))
}
pub async fn preflight(s: &AppState, a: &Actor, org: Uuid, release: Uuid) -> Result<Value> {
    let mut tx = s.pool.begin().await?;
    auth::authorize(&mut tx, a, org, release, "release", false).await?;
    let r = sqlx::query("SELECT status,row_version FROM catalog.releases WHERE org_id=$1 AND id=$2 AND archived_at IS NULL FOR SHARE")
        .bind(org).bind(release).fetch_optional(&mut *tx).await?.ok_or(Error::NotFound)?;
    let tracks = sqlx::query("SELECT t.id,t.asset_id,a.state,a.qc_status FROM catalog.tracks t LEFT JOIN catalog.assets a ON a.org_id=t.org_id AND a.id=t.asset_id WHERE t.org_id=$1 AND t.release_id=$2 AND t.archived_at IS NULL ORDER BY t.disc_number,t.track_number")
        .bind(org).bind(release).fetch_all(&mut *tx).await?;
    let mut issues = Vec::new();
    if r.get::<String, _>("status") != "DRAFT" {
        issues.push(json!({"code":"NOT_DRAFT","resource_id":release}));
    }
    if tracks.is_empty() {
        issues.push(json!({"code":"TRACK_REQUIRED","resource_id":release}));
    }
    for t in tracks {
        let id: Uuid = t.get("id");
        let code = if t.get::<Option<Uuid>, _>("asset_id").is_none() {
            Some("AUDIO_REQUIRED")
        } else if t.get::<Option<String>, _>("state").as_deref() != Some("REGISTERED") {
            Some("AUDIO_NOT_REGISTERED")
        } else if t.get::<Option<String>, _>("qc_status").as_deref() != Some("PASS") {
            Some("AUDIO_QC_PENDING_OR_BLOCKED")
        } else {
            None
        };
        if let Some(code) = code {
            issues.push(json!({"code":code,"resource_id":id}));
        }
    }
    let version: i64 = r.get("row_version");
    tx.commit().await?;
    Ok(
        json!({"release_id":release,"row_version":version,"issues":issues,"submission_enabled":false,"ready_to_submit":false,"gates":["CONSENT_POLICY_NOT_IMPLEMENTED","LEGAL_REPRESENTATIVE_POLICY_NOT_IMPLEMENTED","STAGE1_QC_NOT_IMPLEMENTED"]}),
    )
}

/// Largest number of tracks one batch request may add (a box set or a
/// large compilation). Bigger catalogs are split client-side.
pub const MAX_BATCH_TRACKS: usize = 1_000;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BatchTrack {
    pub title: String,
    pub disc_number: i32,
    pub track_number: i32,
    pub artist_id: Uuid,
    pub asset_id: Option<Uuid>,
    pub lyrics: Option<String>,
    pub parental_advisory: Option<bool>,
    pub version: Option<String>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BatchTracksInput {
    pub row_version: i64,
    pub tracks: Vec<BatchTrack>,
}

/// Field checks shared with single-track writes (see [`track_refs`]).
fn check_track_fields(
    title: &str,
    disc: i32,
    track: i32,
    lyrics: Option<&str>,
    version: Option<&str>,
) -> Result<()> {
    if title.trim().is_empty() || title.len() > 300 || disc < 1 || track < 1 {
        return Err(Error::Invalid);
    }
    if lyrics.map(|s| s.chars().count()).unwrap_or(0) > 20_000
        || version.map(|s| s.chars().count()).unwrap_or(0) > 200
    {
        return Err(Error::Invalid);
    }
    crate::text_policy::check(title)?;
    if let Some(v) = version {
        crate::text_policy::check(v)?;
    }
    if let Some(l) = lyrics {
        crate::text_policy::check_multiline(l)?;
    }
    Ok(())
}

/// Resource ids of `kind` in `ids` the actor may read, locking the ACL rows
/// like [`auth::authorize`] does for one id.
async fn readable(
    c: &mut PgConnection,
    a: &Actor,
    org: Uuid,
    kind: &str,
    ids: &[Uuid],
) -> Result<BTreeSet<Uuid>> {
    let rows: Vec<Uuid> = sqlx::query_scalar(
        "SELECT r.id FROM identity.resources r JOIN identity.resource_acl acl ON acl.org_id=r.org_id AND acl.resource_id=r.id
          WHERE r.org_id=$1 AND r.id = ANY($2) AND r.kind=$3 AND acl.principal_party_id=$4 AND acl.action='read'
            AND acl.revoked_at IS NULL AND acl.starts_at<=now() AND (acl.ends_at IS NULL OR acl.ends_at>now())
          FOR SHARE OF acl",
    )
    .bind(org)
    .bind(ids)
    .bind(kind)
    .bind(a.party)
    .fetch_all(&mut *c)
    .await?;
    Ok(rows.into_iter().collect())
}

/// POST /releases/{id}/tracks/batch: add many tracks atomically.
///
/// Same rules as one-by-one creation (release write ACL, DRAFT/editable,
/// optimistic `row_version`, artist/asset read ACL, text policy, protected
/// names), but one transaction, one release version bump, set-based checks
/// and a single multi-row insert. Adding 1,000 tracks one request at a time
/// meant 1,000 serialized round trips that each bumped the release version,
/// so a single concurrent edit forced the client to start over.
pub async fn add_tracks_batch(
    s: &AppState,
    a: &Actor,
    org: Uuid,
    release: Uuid,
    i: BatchTracksInput,
) -> Result<Value> {
    if i.tracks.is_empty() || i.tracks.len() > MAX_BATCH_TRACKS {
        return Err(Error::Invalid);
    }
    let mut positions = BTreeSet::new();
    for t in &i.tracks {
        check_track_fields(
            &t.title,
            t.disc_number,
            t.track_number,
            t.lyrics.as_deref(),
            t.version.as_deref(),
        )?;
        if !positions.insert((t.disc_number, t.track_number)) {
            return Err(Error::InvalidCode("TRACK_POSITION_DUPLICATE"));
        }
    }
    let mut tx = s.pool.begin().await?;
    auth::authorize(&mut tx, a, org, release, "release", true).await?;
    let texts: Vec<&str> = i
        .tracks
        .iter()
        .flat_map(|t| [t.title.as_str(), t.version.as_deref().unwrap_or("")])
        .collect();
    crate::protected_names::enforce(&mut tx, org, &texts).await?;
    let artists: Vec<Uuid> = i
        .tracks
        .iter()
        .map(|t| t.artist_id)
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect();
    if readable(&mut tx, a, org, "artist", &artists).await?.len() != artists.len() {
        return Err(Error::Forbidden);
    }
    let live: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM (SELECT id FROM catalog.artists WHERE org_id=$1 AND id = ANY($2) AND archived_at IS NULL FOR SHARE) x",
    )
    .bind(org)
    .bind(&artists)
    .fetch_one(&mut *tx)
    .await?;
    if live != artists.len() as i64 {
        return Err(Error::Conflict);
    }
    let assets: Vec<Uuid> = i
        .tracks
        .iter()
        .filter_map(|t| t.asset_id)
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect();
    if !assets.is_empty()
        && readable(&mut tx, a, org, "asset", &assets).await?.len() != assets.len()
    {
        return Err(Error::Forbidden);
    }
    bump(&mut tx, org, release, i.row_version).await?;
    let n = i.tracks.len();
    let ids: Vec<Uuid> = (0..n).map(|_| Uuid::new_v4()).collect();
    let mut titles = Vec::with_capacity(n);
    let mut versions = Vec::with_capacity(n);
    let mut discs = Vec::with_capacity(n);
    let mut numbers = Vec::with_capacity(n);
    let mut artist_ids = Vec::with_capacity(n);
    let mut asset_ids: Vec<Option<Uuid>> = Vec::with_capacity(n);
    let mut lyrics: Vec<Option<String>> = Vec::with_capacity(n);
    let mut advisories = Vec::with_capacity(n);
    for t in i.tracks {
        titles.push(t.title);
        versions.push(
            t.version
                .as_deref()
                .map(str::trim)
                .unwrap_or("")
                .to_string(),
        );
        discs.push(t.disc_number);
        numbers.push(t.track_number);
        artist_ids.push(t.artist_id);
        asset_ids.push(t.asset_id);
        lyrics.push(t.lyrics.filter(|s| !s.trim().is_empty()));
        advisories.push(t.parental_advisory.unwrap_or(false));
    }
    sqlx::query(
        "INSERT INTO catalog.tracks(id,org_id,release_id,title,version,disc_number,track_number,artist_id,asset_id,lyrics,parental_advisory)
         SELECT u.id,$1,$2,u.title,u.version,u.disc,u.num,u.artist,u.asset,u.lyrics,u.pa
           FROM UNNEST($3::uuid[],$4::text[],$5::text[],$6::int[],$7::int[],$8::uuid[],$9::uuid[],$10::text[],$11::bool[])
                AS u(id,title,version,disc,num,artist,asset,lyrics,pa)",
    )
    .bind(org)
    .bind(release)
    .bind(&ids)
    .bind(&titles)
    .bind(&versions)
    .bind(&discs)
    .bind(&numbers)
    .bind(&artist_ids)
    .bind(&asset_ids)
    .bind(&lyrics)
    .bind(&advisories)
    .execute(&mut *tx)
    .await?;
    operations::audit(
        &mut tx,
        Some(a.user),
        Some(org),
        Some(release),
        "release.tracks_batch_added",
        "USER_EDIT",
        a.request,
    )
    .await?;
    operations::event(
        &mut tx,
        org,
        release,
        "release.updated",
        &format!("draft:{release}:{}", i.row_version + 1),
    )
    .await?;
    tx.commit().await?;
    Ok(json!({"ids":ids,"count":n,"row_version":i.row_version+1}))
}
