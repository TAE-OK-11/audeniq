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
    if i.title.trim().is_empty() || i.title.len() > 300 || i.disc_number < 1 || i.track_number < 1 {
        return Err(Error::Invalid);
    }
    if i.lyrics.as_ref().map(|s| s.chars().count()).unwrap_or(0) > 20_000 {
        return Err(Error::Invalid);
    }
    if i.version.as_ref().map(|s| s.chars().count()).unwrap_or(0) > 200 {
        return Err(Error::Invalid);
    }
    crate::text_policy::check(&i.title)?;
    if let Some(v) = &i.version {
        crate::text_policy::check(v)?;
    }
    if let Some(l) = &i.lyrics {
        crate::text_policy::check_multiline(l)?;
    }
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
