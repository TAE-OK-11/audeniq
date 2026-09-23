use crate::{
    api::AppState,
    auth::{self, Actor},
    drafts,
    error::{Error, Result},
    operations,
};
use serde::Deserialize;
use serde_json::{Value, json};
use sqlx::PgConnection;
use uuid::Uuid;
#[derive(Clone, Copy)]
pub enum Kind {
    Artist,
    Label,
    Release,
}
impl Kind {
    pub fn parse(s: &str) -> Result<Self> {
        match s {
            "artists" => Ok(Self::Artist),
            "labels" => Ok(Self::Label),
            "releases" => Ok(Self::Release),
            _ => Err(Error::NotFound),
        }
    }
    pub fn resource(self) -> &'static str {
        match self {
            Self::Artist => "artist",
            Self::Label => "label",
            Self::Release => "release",
        }
    }
    pub fn table(self) -> &'static str {
        match self {
            Self::Artist => "catalog.artists",
            Self::Label => "catalog.labels",
            Self::Release => "catalog.releases",
        }
    }
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Input {
    pub name: String,
    #[serde(default)]
    pub profile: Value,
    pub party_id: Option<Uuid>,
    pub label_id: Option<Uuid>,
    pub release_type: Option<String>,
    pub row_version: Option<i64>,
}
fn validate(i: &Input) -> Result<()> {
    if i.name.trim().is_empty()
        || i.name.len() > 200
        || (!i.profile.is_null() && !i.profile.is_object())
    {
        Err(Error::Invalid)
    } else {
        Ok(())
    }
}
async fn refs(c: &mut PgConnection, a: &Actor, org: Uuid, i: &Input) -> Result<()> {
    if let Some(id) = i.label_id {
        auth::authorize(c, a, org, id, "label", false).await?;
    }
    if let Some(id) = i.party_id {
        let found: bool = sqlx::query_scalar(
            "SELECT EXISTS(SELECT 1 FROM identity.parties WHERE org_id=$1 AND id=$2)",
        )
        .bind(org)
        .bind(id)
        .fetch_one(&mut *c)
        .await?;
        if !found {
            return Err(Error::Forbidden);
        }
    }
    Ok(())
}
pub async fn create(s: &AppState, a: &Actor, org: Uuid, kind: Kind, i: Input) -> Result<Value> {
    validate(&i)?;
    let mut tx = s.pool.begin().await?;
    let id = Uuid::new_v4();
    auth::create_resource(&mut tx, a, org, id, kind.resource()).await?;
    refs(&mut tx, a, org, &i).await?;
    match kind {
        Kind::Artist => {
            sqlx::query("INSERT INTO catalog.artists(id,org_id,name,profile,party_id,label_id) VALUES($1,$2,$3,$4,$5,$6)").bind(id).bind(org).bind(i.name).bind(i.profile).bind(i.party_id).bind(i.label_id).execute(&mut *tx).await?;
        }
        Kind::Label => {
            sqlx::query("INSERT INTO catalog.labels(id,org_id,name,profile,party_id) VALUES($1,$2,$3,$4,$5)").bind(id).bind(org).bind(i.name).bind(i.profile).bind(i.party_id.ok_or(Error::Invalid)?).execute(&mut *tx).await?;
        }
        Kind::Release => {
            sqlx::query("INSERT INTO catalog.releases(id,org_id,title,draft,release_type) VALUES($1,$2,$3,$4,$5)").bind(id).bind(org).bind(i.name).bind(i.profile).bind(i.release_type.ok_or(Error::Invalid)?).execute(&mut *tx).await?;
        }
    }
    operations::audit(
        &mut tx,
        Some(a.user),
        Some(org),
        Some(id),
        &format!("{}.created", kind.resource()),
        "USER_EDIT",
        a.request,
    )
    .await?;
    operations::event(
        &mut tx,
        org,
        id,
        &format!("{}.created", kind.resource()),
        &format!("create:{id}"),
    )
    .await?;
    tx.commit().await?;
    Ok(json!({"id":id,"row_version":0}))
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Page {
    pub after: Option<Uuid>,
    pub limit: Option<i64>,
}
pub async fn list(s: &AppState, a: &Actor, org: Uuid, kind: Kind, page: Page) -> Result<Value> {
    let limit = page.limit.unwrap_or(50);
    if !(1..=100).contains(&limit) {
        return Err(Error::Invalid);
    }
    let mut tx = s.pool.begin().await?;
    auth::membership(&mut tx, a, org, false).await?;
    let query = format!(
        "SELECT to_jsonb(t) AS body FROM {} t WHERE t.org_id=$1 AND t.archived_at IS NULL AND EXISTS(SELECT 1 FROM identity.resource_acl acl WHERE acl.org_id=t.org_id AND acl.resource_id=t.id AND acl.principal_party_id=$2 AND acl.action='read' AND acl.revoked_at IS NULL AND acl.starts_at<=now() AND (acl.ends_at IS NULL OR acl.ends_at>now())) AND ($3::uuid IS NULL OR t.id>$3) ORDER BY t.id LIMIT $4",
        kind.table()
    );
    let mut rows: Vec<Value> = sqlx::query_scalar(&query)
        .bind(org)
        .bind(a.party)
        .bind(page.after)
        .bind(limit + 1)
        .fetch_all(&mut *tx)
        .await?;
    tx.commit().await?;
    let has_more = rows.len() > limit as usize;
    rows.truncate(limit as usize);
    let next_cursor = if has_more {
        rows.last().map(|v| v["id"].clone())
    } else {
        None
    };
    Ok(json!({"items":rows,"limit":limit,"next_cursor":next_cursor}))
}
pub async fn get(s: &AppState, a: &Actor, org: Uuid, kind: Kind, id: Uuid) -> Result<Value> {
    let mut tx = s.pool.begin().await?;
    auth::authorize(&mut tx, a, org, id, kind.resource(), false).await?;
    let q = format!(
        "SELECT to_jsonb(t) FROM {} t WHERE org_id=$1 AND id=$2 AND archived_at IS NULL",
        kind.table()
    );
    let mut v: Value = sqlx::query_scalar(&q)
        .bind(org)
        .bind(id)
        .fetch_optional(&mut *tx)
        .await?
        .ok_or(Error::NotFound)?;
    if matches!(kind, Kind::Release) {
        let tracks:Vec<Value>=sqlx::query_scalar("SELECT to_jsonb(t) || jsonb_build_object('credits',COALESCE((SELECT jsonb_agg(jsonb_build_object('party_id',c.party_id,'role',c.role) ORDER BY c.party_id,c.role) FROM catalog.credits c WHERE c.org_id=t.org_id AND c.track_id=t.id),'[]'::jsonb)) FROM catalog.tracks t WHERE org_id=$1 AND release_id=$2 AND archived_at IS NULL ORDER BY disc_number,track_number").bind(org).bind(id).fetch_all(&mut *tx).await?;
        v["tracks"] = json!(tracks);
        v["delivery_status_by_dsp"] = json!([]);
        v["live_status_by_dsp"] = json!([]);
        v["submission_enabled"] = json!(false);
    }
    tx.commit().await?;
    Ok(v)
}
pub async fn update(
    s: &AppState,
    a: &Actor,
    org: Uuid,
    kind: Kind,
    id: Uuid,
    i: Input,
) -> Result<Value> {
    validate(&i)?;
    let expected = i.row_version.ok_or(Error::Invalid)?;
    let mut tx = s.pool.begin().await?;
    auth::authorize(&mut tx, a, org, id, kind.resource(), true).await?;
    refs(&mut tx, a, org, &i).await?;
    let n=match kind{
 Kind::Artist=>sqlx::query("UPDATE catalog.artists SET name=$3,profile=$4,party_id=$6,label_id=$7,row_version=row_version+1 WHERE org_id=$1 AND id=$2 AND row_version=$5 AND archived_at IS NULL").bind(org).bind(id).bind(i.name).bind(i.profile).bind(expected).bind(i.party_id).bind(i.label_id).execute(&mut *tx).await?.rows_affected(),
 Kind::Label=>sqlx::query("UPDATE catalog.labels SET name=$3,profile=$4,party_id=$6,row_version=row_version+1 WHERE org_id=$1 AND id=$2 AND row_version=$5 AND archived_at IS NULL").bind(org).bind(id).bind(i.name).bind(i.profile).bind(expected).bind(i.party_id.ok_or(Error::Invalid)?).execute(&mut *tx).await?.rows_affected(),
 Kind::Release=>sqlx::query("UPDATE catalog.releases SET title=$3,draft=$4,release_type=$6,row_version=row_version+1 WHERE org_id=$1 AND id=$2 AND row_version=$5 AND status='DRAFT' AND archived_at IS NULL").bind(org).bind(id).bind(i.name).bind(i.profile).bind(expected).bind(i.release_type.ok_or(Error::Invalid)?).execute(&mut *tx).await?.rows_affected(),
 };
    if n != 1 {
        return Err(Error::Conflict);
    }
    operations::audit(
        &mut tx,
        Some(a.user),
        Some(org),
        Some(id),
        &format!("{}.updated", kind.resource()),
        "USER_EDIT",
        a.request,
    )
    .await?;
    operations::event(
        &mut tx,
        org,
        id,
        &format!("{}.updated", kind.resource()),
        &format!("update:{id}:{}", expected + 1),
    )
    .await?;
    tx.commit().await?;
    Ok(json!({"id":id,"row_version":expected+1}))
}
pub async fn archive(
    s: &AppState,
    a: &Actor,
    org: Uuid,
    kind: Kind,
    id: Uuid,
    version: i64,
) -> Result<Value> {
    let mut tx = s.pool.begin().await?;
    auth::authorize(&mut tx, a, org, id, kind.resource(), true).await?;
    let extra = if matches!(kind, Kind::Release) {
        " AND status='DRAFT'"
    } else {
        ""
    };
    let q = format!(
        "UPDATE {} SET archived_at=now(),row_version=row_version+1 WHERE org_id=$1 AND id=$2 AND row_version=$3 AND archived_at IS NULL{extra}",
        kind.table()
    );
    if sqlx::query(&q)
        .bind(org)
        .bind(id)
        .bind(version)
        .execute(&mut *tx)
        .await?
        .rows_affected()
        != 1
    {
        return Err(Error::Conflict);
    }
    operations::audit(
        &mut tx,
        Some(a.user),
        Some(org),
        Some(id),
        &format!("{}.archived", kind.resource()),
        "USER_EDIT",
        a.request,
    )
    .await?;
    operations::event(
        &mut tx,
        org,
        id,
        &format!("{}.archived", kind.resource()),
        &format!("archive:{id}"),
    )
    .await?;
    tx.commit().await?;
    Ok(json!({"archived":true}))
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TrackInput {
    pub title: String,
    pub disc_number: i32,
    pub track_number: i32,
    pub artist_id: Uuid,
    pub asset_id: Option<Uuid>,
    pub row_version: i64,
}
pub async fn track(
    s: &AppState,
    a: &Actor,
    org: Uuid,
    release: Uuid,
    i: TrackInput,
) -> Result<Value> {
    let mut tx = s.pool.begin().await?;
    auth::authorize(&mut tx, a, org, release, "release", true).await?;
    drafts::track_refs(&mut tx, a, org, &i).await?;
    drafts::bump(&mut tx, org, release, i.row_version).await?;
    let id = Uuid::new_v4();
    sqlx::query("INSERT INTO catalog.tracks(id,org_id,release_id,title,disc_number,track_number,artist_id,asset_id) VALUES($1,$2,$3,$4,$5,$6,$7,$8)")
 .bind(id).bind(org).bind(release).bind(i.title).bind(i.disc_number).bind(i.track_number).bind(i.artist_id).bind(i.asset_id).execute(&mut *tx).await?;
    operations::audit(
        &mut tx,
        Some(a.user),
        Some(org),
        Some(release),
        "release.track_added",
        "USER_EDIT",
        a.request,
    )
    .await?;
    operations::event(
        &mut tx,
        org,
        release,
        "release.updated",
        &format!("track:{id}"),
    )
    .await?;
    tx.commit().await?;
    Ok(json!({"id":id,"row_version":i.row_version+1}))
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MemberInput {
    pub user_id: Uuid,
    pub role: String,
    pub status: String,
}
pub async fn member(s: &AppState, a: &Actor, org: Uuid, i: MemberInput) -> Result<Value> {
    if !matches!(i.role.as_str(), "EDITOR" | "VIEWER")
        || !matches!(i.status.as_str(), "ACTIVE" | "REVOKED")
        || i.user_id == a.user
    {
        return Err(Error::Invalid);
    }
    let mut tx = s.pool.begin().await?;
    if auth::membership(&mut tx, a, org, true).await? != "OWNER" {
        return Err(Error::Forbidden);
    }
    sqlx::query("INSERT INTO identity.memberships(org_id,user_id,role,status) VALUES($1,$2,$3,$4) ON CONFLICT(org_id,user_id) DO UPDATE SET role=EXCLUDED.role,status=EXCLUDED.status WHERE identity.memberships.role<>'OWNER'").bind(org).bind(i.user_id).bind(i.role).bind(i.status).execute(&mut *tx).await?;
    operations::audit(
        &mut tx,
        Some(a.user),
        Some(org),
        Some(i.user_id),
        "membership.changed",
        "OWNER_REQUEST",
        a.request,
    )
    .await?;
    tx.commit().await?;
    Ok(json!({"updated":true}))
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AclInput {
    pub user_id: Uuid,
    pub action: String,
    pub revoked: bool,
}
pub async fn acl(s: &AppState, a: &Actor, org: Uuid, resource: Uuid, i: AclInput) -> Result<Value> {
    if !matches!(i.action.as_str(), "read" | "write") {
        return Err(Error::Invalid);
    }
    let mut tx = s.pool.begin().await?;
    if auth::membership(&mut tx, a, org, true).await? != "OWNER" {
        return Err(Error::Forbidden);
    }
    // Owner must also possess resource write ACL; no tenant-wide ACL bypass.
    let kind: String =
        sqlx::query_scalar("SELECT kind FROM identity.resources WHERE org_id=$1 AND id=$2")
            .bind(org)
            .bind(resource)
            .fetch_optional(&mut *tx)
            .await?
            .ok_or(Error::Forbidden)?;
    auth::authorize(&mut tx, a, org, resource, &kind, true).await?;
    let party:Uuid=sqlx::query_scalar("SELECT u.party_id FROM identity.users u JOIN identity.memberships m ON m.user_id=u.id WHERE m.org_id=$1 AND u.id=$2 AND m.status='ACTIVE' FOR SHARE OF m")
 .bind(org).bind(i.user_id).fetch_optional(&mut *tx).await?.ok_or(Error::Forbidden)?;
    sqlx::query("INSERT INTO identity.resource_acl(org_id,resource_id,principal_party_id,action,revoked_at) VALUES($1,$2,$3,$4,CASE WHEN $5 THEN now() END) ON CONFLICT(org_id,resource_id,principal_party_id,action) DO UPDATE SET revoked_at=EXCLUDED.revoked_at")
 .bind(org).bind(resource).bind(party).bind(i.action).bind(i.revoked).execute(&mut *tx).await?;
    operations::audit(
        &mut tx,
        Some(a.user),
        Some(org),
        Some(resource),
        "acl.changed",
        "OWNER_REQUEST",
        a.request,
    )
    .await?;
    tx.commit().await?;
    Ok(json!({"updated":true}))
}
